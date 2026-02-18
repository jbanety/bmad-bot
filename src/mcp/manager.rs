//! MCP server lifecycle manager.
//!
//! Connects to external MCP servers at daemon startup, discovers their tools,
//! and provides them to the agent builder. Failures are logged but never crash
//! the daemon — graceful degradation is the default.

use std::time::Duration;

use rmcp::model::Tool;
use rmcp::service::{RoleClient, RunningService, ServerSink, ServiceExt};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use tokio::process::Command;

use crate::config::McpServerConfig;

// ---------------------------------------------------------------------------
// McpError
// ---------------------------------------------------------------------------

/// Errors that can occur during MCP server lifecycle operations.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    /// The MCP server child process failed to spawn.
    #[error("MCP server '{name}' failed to spawn: {source}")]
    SpawnFailed {
        /// Server name from config.
        name: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// The MCP handshake timed out.
    #[error("MCP server '{name}' handshake timed out")]
    HandshakeTimeout {
        /// Server name from config.
        name: String,
    },

    /// The MCP handshake failed with an error.
    #[error("MCP server '{name}' handshake failed: {reason}")]
    HandshakeFailed {
        /// Server name from config.
        name: String,
        /// Error details.
        reason: String,
    },

    /// Tool discovery failed after a successful handshake.
    #[error("MCP server '{name}' tool discovery failed: {reason}")]
    ToolDiscoveryFailed {
        /// Server name from config.
        name: String,
        /// Error details.
        reason: String,
    },

    /// An error occurred while shutting down an MCP server.
    #[error("MCP server '{name}' shutdown error: {reason}")]
    ShutdownError {
        /// Server name from config.
        name: String,
        /// Error details.
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// McpServerHandle
// ---------------------------------------------------------------------------

/// Holds a connected MCP server's running service and discovered tools.
struct McpServerHandle {
    /// Human-readable name for this MCP server.
    name: String,
    /// The running MCP service — owns the child process and background task.
    /// Derefs to `Peer<RoleClient>` (= `ServerSink`).
    /// MUST be stored to keep the background task alive.
    service: RunningService<RoleClient, ()>,
    /// Tools discovered via `list_all_tools()` at init time.
    tools: Vec<Tool>,
}

// ---------------------------------------------------------------------------
// McpManager
// ---------------------------------------------------------------------------

/// Manages the lifecycle of external MCP servers.
///
/// Created during daemon startup via [`McpManager::init()`]. Provides
/// discovered tools to the agent builder via [`McpManager::tools_for_builder()`].
/// Must be shut down cleanly via [`McpManager::shutdown()`] on daemon exit.
pub struct McpManager {
    /// Connected MCP servers wrapped in a mutex so shutdown can drain them
    /// while the manager is behind an `Arc`.
    servers: tokio::sync::Mutex<Vec<McpServerHandle>>,
}

impl std::fmt::Debug for McpManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpManager")
            .field("servers", &"<locked>")
            .finish()
    }
}

/// Default handshake timeout when `timeout_secs` is not configured.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

impl McpManager {
    /// Creates an `McpManager` with zero servers.
    ///
    /// Used as fallback when no MCP servers are configured or when init
    /// encounters an unrecoverable situation (which shouldn't happen since
    /// init is infallible).
    pub fn empty() -> Self {
        Self {
            servers: tokio::sync::Mutex::new(vec![]),
        }
    }

    /// Connects to all enabled MCP servers from the provided configurations.
    ///
    /// This method is **infallible** — individual server failures are logged
    /// via `tracing::warn!()` and skipped. The daemon continues startup with
    /// whatever servers successfully connected (possibly zero).
    pub async fn init(configs: &[McpServerConfig]) -> Self {
        // Filter out disabled servers.
        let enabled: Vec<&McpServerConfig> = configs.iter().filter(|c| c.enabled).collect();

        if enabled.is_empty() {
            tracing::info!("MCP initialization complete — no servers configured");
            return Self::empty();
        }

        let mut servers = Vec::with_capacity(enabled.len());
        let mut failed_count: usize = 0;
        let mut total_tool_count: usize = 0;

        for config in &enabled {
            match Self::connect_server(config).await {
                Ok(handle) => {
                    let tool_count = handle.tools.len();
                    tracing::info!(
                        server = %handle.name,
                        tool_count,
                        "MCP server connected"
                    );
                    total_tool_count += tool_count;
                    servers.push(handle);
                }
                Err(e) => {
                    tracing::warn!(
                        server = %config.name,
                        error = %e,
                        "MCP server failed — skipping"
                    );
                    failed_count += 1;
                }
            }
        }

        let connected = servers.len();
        tracing::info!(
            connected,
            failed = failed_count,
            total_tools = total_tool_count,
            "MCP initialization complete"
        );

        Self {
            servers: tokio::sync::Mutex::new(servers),
        }
    }

    /// Returns tools and server sinks for each connected MCP server.
    ///
    /// Each tuple contains the tools discovered from a single server and a
    /// cloned `ServerSink` that the agent builder can use to invoke those tools.
    pub async fn tools_for_builder(&self) -> Vec<(Vec<Tool>, ServerSink)> {
        let servers = self.servers.lock().await;
        servers
            .iter()
            .map(|handle| {
                let tools = handle.tools.clone();
                // RunningService<RoleClient, ()> derefs to Peer<RoleClient> (= ServerSink).
                // Peer<RoleClient> is Clone.
                let sink: ServerSink = (*handle.service).clone();
                (tools, sink)
            })
            .collect()
    }

    /// Gracefully shuts down all connected MCP servers.
    ///
    /// Sends MCP close notifications and waits for child processes to exit.
    /// Errors are logged but never propagated — shutdown is best-effort.
    pub async fn shutdown(&self) {
        let mut servers = self.servers.lock().await;
        let handles: Vec<McpServerHandle> = servers.drain(..).collect();
        drop(servers); // Release lock before async work.

        for mut handle in handles {
            let name = handle.name.clone();
            match handle.service.close().await {
                Ok(_) => {
                    tracing::info!(server = %name, "MCP server disconnected");
                }
                Err(e) => {
                    tracing::warn!(
                        server = %name,
                        error = %e,
                        "MCP server shutdown error"
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Attempts to connect to a single MCP server: spawn → handshake → discover tools.
    async fn connect_server(config: &McpServerConfig) -> Result<McpServerHandle, McpError> {
        let name = config.name.clone();
        let timeout = Duration::from_secs(config.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS));

        // 1. Spawn child process via stdio transport.
        let transport = TokioChildProcess::new(Command::new(&config.command).configure(|cmd| {
            cmd.args(&config.args);
        }))
        .map_err(|e| McpError::SpawnFailed {
            name: name.clone(),
            source: e,
        })?;

        // 2. MCP handshake — ().serve(transport) performs the initialize exchange.
        let service: RunningService<RoleClient, ()> =
            tokio::time::timeout(timeout, ().serve(transport))
                .await
                .map_err(|_| McpError::HandshakeTimeout { name: name.clone() })?
                .map_err(|e| McpError::HandshakeFailed {
                    name: name.clone(),
                    reason: format!("{e}"),
                })?;

        // 3. Tool discovery — pagination-safe via list_all_tools().
        let tools = service
            .list_all_tools()
            .await
            .map_err(|e| McpError::ToolDiscoveryFailed {
                name: name.clone(),
                reason: format!("{e}"),
            })?;

        Ok(McpServerHandle {
            name,
            service,
            tools,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::McpTransport;

    #[test]
    fn test_mcp_error_display_spawn_failed() {
        let err = McpError::SpawnFailed {
            name: "playwright".to_string(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
        };
        let msg = format!("{err}");
        assert!(msg.contains("playwright"));
        assert!(msg.contains("failed to spawn"));
    }

    #[test]
    fn test_mcp_error_display_handshake_timeout() {
        let err = McpError::HandshakeTimeout {
            name: "test-server".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("test-server"));
        assert!(msg.contains("timed out"));
    }

    #[test]
    fn test_mcp_error_display_handshake_failed() {
        let err = McpError::HandshakeFailed {
            name: "test-server".to_string(),
            reason: "protocol mismatch".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("test-server"));
        assert!(msg.contains("protocol mismatch"));
    }

    #[test]
    fn test_mcp_error_display_tool_discovery_failed() {
        let err = McpError::ToolDiscoveryFailed {
            name: "test-server".to_string(),
            reason: "timeout".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("test-server"));
        assert!(msg.contains("tool discovery failed"));
    }

    #[test]
    fn test_mcp_error_display_shutdown_error() {
        let err = McpError::ShutdownError {
            name: "test-server".to_string(),
            reason: "already closed".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("test-server"));
        assert!(msg.contains("shutdown error"));
    }

    #[tokio::test]
    async fn test_mcp_manager_empty_returns_no_servers() {
        let manager = McpManager::empty();
        let tools = manager.tools_for_builder().await;
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_mcp_manager_init_empty_configs() {
        let manager = McpManager::init(&[]).await;
        let tools = manager.tools_for_builder().await;
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_mcp_manager_init_all_disabled() {
        let configs = vec![
            McpServerConfig {
                name: "disabled-server".to_string(),
                command: "nonexistent-cmd".to_string(),
                args: vec![],
                transport: McpTransport::Stdio,
                enabled: false,
                timeout_secs: None,
            },
            McpServerConfig {
                name: "also-disabled".to_string(),
                command: "another-nonexistent".to_string(),
                args: vec![],
                transport: McpTransport::Stdio,
                enabled: false,
                timeout_secs: None,
            },
        ];
        let manager = McpManager::init(&configs).await;
        let tools = manager.tools_for_builder().await;
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_mcp_manager_init_filters_disabled() {
        // One enabled (will fail to spawn) + one disabled.
        // Both should result in zero connected servers — the enabled one
        // fails because the command doesn't exist, the disabled one is skipped.
        let configs = vec![
            McpServerConfig {
                name: "will-fail".to_string(),
                command: "this-command-does-not-exist-bmad-test".to_string(),
                args: vec![],
                transport: McpTransport::Stdio,
                enabled: true,
                timeout_secs: Some(1),
            },
            McpServerConfig {
                name: "skipped".to_string(),
                command: "also-nonexistent".to_string(),
                args: vec![],
                transport: McpTransport::Stdio,
                enabled: false,
                timeout_secs: None,
            },
        ];
        let manager = McpManager::init(&configs).await;
        let tools = manager.tools_for_builder().await;
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_mcp_manager_init_spawn_failure_continues() {
        // Verify that a spawn failure doesn't crash — manager is created with zero servers.
        let configs = vec![McpServerConfig {
            name: "bad-command".to_string(),
            command: "this-command-absolutely-does-not-exist-12345".to_string(),
            args: vec!["--some-arg".to_string()],
            transport: McpTransport::Stdio,
            enabled: true,
            timeout_secs: Some(1),
        }];
        let manager = McpManager::init(&configs).await;
        let tools = manager.tools_for_builder().await;
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_mcp_manager_shutdown_empty() {
        // Shutdown on empty manager should be a no-op.
        let manager = McpManager::empty();
        manager.shutdown().await;
        // No panic = success.
    }

    #[test]
    fn test_mcp_manager_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<McpManager>();
    }

    #[test]
    fn test_mcp_server_config_enabled_filtering() {
        let configs = vec![
            McpServerConfig {
                name: "a".to_string(),
                command: "cmd-a".to_string(),
                args: vec![],
                transport: McpTransport::Stdio,
                enabled: true,
                timeout_secs: None,
            },
            McpServerConfig {
                name: "b".to_string(),
                command: "cmd-b".to_string(),
                args: vec![],
                transport: McpTransport::Stdio,
                enabled: false,
                timeout_secs: None,
            },
            McpServerConfig {
                name: "c".to_string(),
                command: "cmd-c".to_string(),
                args: vec![],
                transport: McpTransport::Stdio,
                enabled: true,
                timeout_secs: None,
            },
        ];
        let enabled: Vec<&McpServerConfig> = configs.iter().filter(|c| c.enabled).collect();
        assert_eq!(enabled.len(), 2);
        assert_eq!(enabled[0].name, "a");
        assert_eq!(enabled[1].name, "c");
    }
}
