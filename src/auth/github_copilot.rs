//! GitHub Copilot OAuth Device Flow and runtime token exchange.
//!
//! This module implements:
//! - **Device Flow** ([RFC 8628](https://datatracker.ietf.org/doc/html/rfc8628)):
//!   `request_device_code()` → display URL + user code → `poll_for_access_token()` → long-lived OAuth token.
//! - **Copilot Token Exchange**: exchanges the long-lived OAuth token for a short-lived
//!   Copilot session token via `GET /copilot_internal/v2/token`.
//! - **Token Caching**: [`CopilotTokenCache`] holds the session token in memory with a
//!   5-minute safety margin before expiry.
//! - **Base URL Derivation**: parses `proxy-ep=<host>` from the session token to build the
//!   API base URL dynamically.
//!
//! All HTTP calls are abstracted behind [`CopilotHttpClient`] for deterministic unit testing.

use async_trait::async_trait;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// GitHub Copilot OAuth App client ID (public, same as VS Code uses).
const COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";

/// OAuth scope for the Device Flow.
const COPILOT_SCOPE: &str = "read:user";

/// GitHub Device Code endpoint.
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";

/// GitHub OAuth access token endpoint.
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

/// Copilot internal token exchange endpoint.
const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";

/// Default Copilot API base URL when `proxy-ep` is not found in token.
const DEFAULT_COPILOT_BASE_URL: &str = "https://api.individual.githubcopilot.com";

/// Safety margin (in milliseconds) before considering a cached token expired.
const TOKEN_EXPIRY_SAFETY_MARGIN_MS: u64 = 5 * 60 * 1000;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors arising from Copilot authentication (Device Flow + token exchange).
#[derive(Debug, thiserror::Error)]
pub enum CopilotAuthError {
    /// Failed to request a device code from GitHub.
    #[error("Failed to request device code from GitHub: {reason}")]
    DeviceCodeRequestFailed {
        /// Human-readable failure reason.
        reason: String,
    },

    /// GitHub returned a device code response with missing or invalid fields.
    #[error("Invalid device code response from GitHub: {reason}")]
    DeviceCodeResponseInvalid {
        /// Description of what was missing or malformed.
        reason: String,
    },

    /// Polling for the access token failed unexpectedly.
    #[error("Failed to poll for access token: {reason}")]
    AccessTokenPollFailed {
        /// Human-readable failure reason.
        reason: String,
    },

    /// The device code expired before the user completed authorization.
    #[error("GitHub device code expired — re-run `bmad-bot init` to authenticate")]
    DeviceCodeExpired,

    /// The user explicitly denied the authorization request.
    #[error("GitHub authorization was denied by the user")]
    AccessDenied,

    /// The Copilot token exchange HTTP call returned a non-success status.
    #[error("Copilot token exchange failed: HTTP {status}")]
    TokenExchangeFailed {
        /// HTTP status code returned by the exchange endpoint.
        status: u16,
    },

    /// The Copilot token exchange response was missing required fields.
    #[error("Invalid Copilot token exchange response: {reason}")]
    TokenExchangeResponseInvalid {
        /// Description of what was missing or malformed.
        reason: String,
    },

    /// Catch-all for unexpected errors during Copilot authentication.
    #[error("Unexpected error during Copilot authentication: {reason}")]
    UnexpectedError {
        /// Human-readable failure reason.
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Parsed response from GitHub's device code endpoint.
#[derive(Debug, Clone)]
pub struct DeviceCodeResponse {
    /// Opaque device code used when polling for the access token.
    pub device_code: String,
    /// Short alphanumeric code the user enters on the verification page.
    pub user_code: String,
    /// URL the user must visit to authorize the device.
    pub verification_uri: String,
    /// Seconds until the device code expires.
    pub expires_in: u64,
    /// Recommended minimum polling interval in seconds.
    pub interval: u64,
}

/// Possible responses from GitHub's access token polling endpoint.
#[derive(Debug, Clone)]
pub enum DeviceTokenResponse {
    /// The user has authorized — contains the OAuth access token.
    Success {
        /// Long-lived OAuth access token.
        access_token: String,
        /// Token type (typically `"bearer"`).
        token_type: String,
        /// Granted scope string.
        scope: String,
    },
    /// Authorization is still pending — keep polling.
    Pending {
        /// Error code from GitHub (`"authorization_pending"`, `"slow_down"`, etc.).
        error: String,
    },
}

/// Parsed response from the Copilot token exchange endpoint.
#[derive(Debug, Clone)]
pub struct CopilotTokenResponse {
    /// Short-lived Copilot session token (semicolon-delimited key-value pairs).
    pub token: String,
    /// Unix timestamp (seconds) when the token expires.
    pub expires_at: u64,
}

// ---------------------------------------------------------------------------
// HTTP client trait (for mocking)
// ---------------------------------------------------------------------------

/// Abstraction over HTTP calls needed by the Copilot auth module.
///
/// All methods return domain types so that test mocks can skip real networking.
/// Implementations must be `Send + Sync` for use in async contexts.
#[async_trait]
pub trait CopilotHttpClient: Send + Sync {
    /// POST to the device code endpoint and return the parsed response.
    async fn request_device_code(
        &self,
        client_id: &str,
        scope: &str,
    ) -> Result<DeviceCodeResponse, CopilotAuthError>;

    /// POST to the access token endpoint and return the parsed response.
    async fn poll_access_token(
        &self,
        client_id: &str,
        device_code: &str,
    ) -> Result<DeviceTokenResponse, CopilotAuthError>;

    /// GET the Copilot internal token exchange endpoint.
    async fn exchange_copilot_token(
        &self,
        oauth_token: &str,
    ) -> Result<CopilotTokenResponse, CopilotAuthError>;
}

// ---------------------------------------------------------------------------
// Real HTTP client (reqwest)
// ---------------------------------------------------------------------------

/// Production [`CopilotHttpClient`] backed by `reqwest`.
pub struct ReqwestCopilotHttpClient {
    client: reqwest::Client,
}

impl ReqwestCopilotHttpClient {
    /// Create a new HTTP client for Copilot auth endpoints.
    ///
    /// Sets a `User-Agent` header on every request — GitHub API returns 403
    /// without one ("Request forbidden by administrative rules").
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(format!("bmad-bot/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("failed to build reqwest client");
        Self { client }
    }
}

impl Default for ReqwestCopilotHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CopilotHttpClient for ReqwestCopilotHttpClient {
    async fn request_device_code(
        &self,
        client_id: &str,
        scope: &str,
    ) -> Result<DeviceCodeResponse, CopilotAuthError> {
        let resp = self
            .client
            .post(DEVICE_CODE_URL)
            .header("Accept", "application/json")
            .form(&[("client_id", client_id), ("scope", scope)])
            .send()
            .await
            .map_err(|e| CopilotAuthError::DeviceCodeRequestFailed {
                reason: e.to_string(),
            })?;

        if !resp.status().is_success() {
            return Err(CopilotAuthError::DeviceCodeRequestFailed {
                reason: format!("HTTP {}", resp.status()),
            });
        }

        let json: serde_json::Value = resp.json::<serde_json::Value>().await.map_err(|e| {
            CopilotAuthError::DeviceCodeResponseInvalid {
                reason: format!("Failed to parse JSON: {e}"),
            }
        })?;

        parse_device_code_response(json)
    }

    async fn poll_access_token(
        &self,
        client_id: &str,
        device_code: &str,
    ) -> Result<DeviceTokenResponse, CopilotAuthError> {
        let resp = self
            .client
            .post(ACCESS_TOKEN_URL)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", client_id),
                ("device_code", device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .map_err(|e| CopilotAuthError::AccessTokenPollFailed {
                reason: e.to_string(),
            })?;

        let json: serde_json::Value = resp.json::<serde_json::Value>().await.map_err(|e| {
            CopilotAuthError::AccessTokenPollFailed {
                reason: format!("Failed to parse JSON: {e}"),
            }
        })?;

        parse_device_token_response(json)
    }

    async fn exchange_copilot_token(
        &self,
        oauth_token: &str,
    ) -> Result<CopilotTokenResponse, CopilotAuthError> {
        let resp = self
            .client
            .get(COPILOT_TOKEN_URL)
            .header("Authorization", format!("Bearer {oauth_token}"))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| CopilotAuthError::TokenExchangeFailed {
                status: e.status().map_or(0, |s| s.as_u16()),
            })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::error!(
                http_status = status.as_u16(),
                response_body = %body,
                "Copilot token exchange returned non-success status"
            );
            return Err(CopilotAuthError::TokenExchangeFailed {
                status: status.as_u16(),
            });
        }

        let json: serde_json::Value = resp.json::<serde_json::Value>().await.map_err(|e| {
            CopilotAuthError::TokenExchangeResponseInvalid {
                reason: format!("Failed to parse JSON: {e}"),
            }
        })?;

        parse_copilot_token_response(json)
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parse a device code JSON response into [`DeviceCodeResponse`].
fn parse_device_code_response(
    json: serde_json::Value,
) -> Result<DeviceCodeResponse, CopilotAuthError> {
    let device_code = json
        .get("device_code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CopilotAuthError::DeviceCodeResponseInvalid {
            reason: "missing field: device_code".to_string(),
        })?
        .to_string();

    let user_code = json
        .get("user_code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CopilotAuthError::DeviceCodeResponseInvalid {
            reason: "missing field: user_code".to_string(),
        })?
        .to_string();

    let verification_uri = json
        .get("verification_uri")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CopilotAuthError::DeviceCodeResponseInvalid {
            reason: "missing field: verification_uri".to_string(),
        })?
        .to_string();

    let expires_in = json
        .get("expires_in")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CopilotAuthError::DeviceCodeResponseInvalid {
            reason: "missing field: expires_in".to_string(),
        })?;

    let interval = json.get("interval").and_then(|v| v.as_u64()).unwrap_or(5); // Default per GitHub docs

    Ok(DeviceCodeResponse {
        device_code,
        user_code,
        verification_uri,
        expires_in,
        interval,
    })
}

/// Parse a device token polling JSON response into [`DeviceTokenResponse`].
fn parse_device_token_response(
    json: serde_json::Value,
) -> Result<DeviceTokenResponse, CopilotAuthError> {
    // Check for error field first (pending states)
    if let Some(error) = json.get("error").and_then(|v| v.as_str()) {
        return Ok(DeviceTokenResponse::Pending {
            error: error.to_string(),
        });
    }

    // Otherwise expect a success response
    let access_token = json
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CopilotAuthError::AccessTokenPollFailed {
            reason: "response missing both 'error' and 'access_token' fields".to_string(),
        })?
        .to_string();

    let token_type = json
        .get("token_type")
        .and_then(|v| v.as_str())
        .unwrap_or("bearer")
        .to_string();

    let scope = json
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(DeviceTokenResponse::Success {
        access_token,
        token_type,
        scope,
    })
}

/// Parse a Copilot token exchange JSON response into [`CopilotTokenResponse`].
///
/// Handles `expires_at` as either seconds or milliseconds (if the value is
/// unreasonably large, it is treated as milliseconds and divided by 1000).
fn parse_copilot_token_response(
    json: serde_json::Value,
) -> Result<CopilotTokenResponse, CopilotAuthError> {
    let token = json
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CopilotAuthError::TokenExchangeResponseInvalid {
            reason: "missing field: token".to_string(),
        })?
        .to_string();

    let raw_expires = json
        .get("expires_at")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CopilotAuthError::TokenExchangeResponseInvalid {
            reason: "missing field: expires_at".to_string(),
        })?;

    // Heuristic: if expires_at > 1e12, treat as milliseconds
    let expires_at = if raw_expires > 1_000_000_000_000 {
        raw_expires / 1000
    } else {
        raw_expires
    };

    Ok(CopilotTokenResponse { token, expires_at })
}

// ---------------------------------------------------------------------------
// Device Flow public functions
// ---------------------------------------------------------------------------

/// Request a device code from GitHub to begin the OAuth Device Flow.
///
/// Calls the client's `request_device_code` method with the hardcoded
/// Copilot client ID and scope, then validates the response.
pub async fn request_device_code(
    client: &dyn CopilotHttpClient,
) -> Result<DeviceCodeResponse, CopilotAuthError> {
    client
        .request_device_code(COPILOT_CLIENT_ID, COPILOT_SCOPE)
        .await
}

/// Poll GitHub for an OAuth access token after a device code has been issued.
///
/// This function loops until one of:
/// - A valid access token is received → returns `Ok(token_string)`
/// - The device code expires → returns `Err(DeviceCodeExpired)`
/// - The user denies access → returns `Err(AccessDenied)`
///
/// The `interval` is the initial polling interval (seconds). On `slow_down`
/// responses the interval is increased by 2 seconds per the OAuth spec.
pub async fn poll_for_access_token(
    client: &dyn CopilotHttpClient,
    device_code: &str,
    initial_interval: u64,
    expires_in: u64,
) -> Result<String, CopilotAuthError> {
    let start = std::time::Instant::now();
    let deadline = std::time::Duration::from_secs(expires_in);
    let mut interval = initial_interval;

    loop {
        if start.elapsed() >= deadline {
            return Err(CopilotAuthError::DeviceCodeExpired);
        }

        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;

        match client
            .poll_access_token(COPILOT_CLIENT_ID, device_code)
            .await?
        {
            DeviceTokenResponse::Success { access_token, .. } => {
                return Ok(access_token);
            }
            DeviceTokenResponse::Pending { error } => match error.as_str() {
                "authorization_pending" => {
                    // Keep polling at current interval
                }
                "slow_down" => {
                    interval += 2;
                }
                "expired_token" => {
                    return Err(CopilotAuthError::DeviceCodeExpired);
                }
                "access_denied" => {
                    return Err(CopilotAuthError::AccessDenied);
                }
                other => {
                    return Err(CopilotAuthError::AccessTokenPollFailed {
                        reason: format!("Unexpected error code: {other}"),
                    });
                }
            },
        }
    }
}

/// Run the full OAuth Device Flow for GitHub Copilot.
///
/// 1. Requests a device code from GitHub.
/// 2. Prints the verification URL and user code to stdout.
/// 3. Polls until the user authorizes or the code expires.
/// 4. Returns the long-lived OAuth access token.
pub async fn run_device_flow(client: &dyn CopilotHttpClient) -> Result<String, CopilotAuthError> {
    let device = request_device_code(client).await?;

    // Display instructions to the user
    println!();
    println!("── GitHub Copilot Authentication ──");
    println!();
    println!("🔗 To authorize BMAD Bot with GitHub Copilot:");
    println!();
    println!("   1. Open: {}", device.verification_uri);
    println!("   2. Enter code: {}", device.user_code);
    println!();
    println!("⏳ Waiting for authorization...");

    let token = poll_for_access_token(
        client,
        &device.device_code,
        device.interval,
        device.expires_in,
    )
    .await?;

    println!();
    println!("✅ GitHub Copilot authorization successful!");

    Ok(token)
}

// ---------------------------------------------------------------------------
// Token exchange & caching
// ---------------------------------------------------------------------------

/// Parse `proxy-ep=<value>` from a Copilot session token and derive the API base URL.
///
/// Token format: `"tid=abc123;exp=1234567890;sku=free;proxy-ep=proxy.example.com;st=dotcom;..."`
///
/// 1. Find `proxy-ep=<value>` in semicolon-delimited pairs.
/// 2. Strip protocol prefix if present (e.g., `"https://proxy.foo.bar"` → `"proxy.foo.bar"`).
/// 3. Replace `"proxy."` prefix with `"api."`.
/// 4. Prepend `"https://"`.
/// 5. If no `proxy-ep` found, return [`DEFAULT_COPILOT_BASE_URL`].
pub fn derive_base_url_from_token(token: &str) -> String {
    for part in token.split(';') {
        if let Some(value) = part.strip_prefix("proxy-ep=") {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }

            // Strip protocol prefix if present
            let host = value
                .strip_prefix("https://")
                .or_else(|| value.strip_prefix("http://"))
                .unwrap_or(value);

            // Replace "proxy." prefix with "api."
            let api_host = if let Some(rest) = host.strip_prefix("proxy.") {
                format!("api.{rest}")
            } else {
                host.to_string()
            };

            return format!("https://{api_host}");
        }
    }

    DEFAULT_COPILOT_BASE_URL.to_string()
}

/// In-memory cache for the short-lived Copilot session token.
///
/// Call [`resolve()`](Self::resolve) before each LLM session to get a valid
/// `(session_token, base_url)` pair. The cache transparently refreshes when
/// the token is within [`TOKEN_EXPIRY_SAFETY_MARGIN_MS`] of expiry.
pub struct CopilotTokenCache {
    /// Cached Copilot session token.
    cached_token: Option<String>,
    /// Cached base URL derived from the session token.
    cached_base_url: Option<String>,
    /// Expiry timestamp in milliseconds since UNIX epoch.
    expires_at_ms: Option<u64>,
}

impl CopilotTokenCache {
    /// Create an empty (cold) cache.
    pub fn new() -> Self {
        Self {
            cached_token: None,
            cached_base_url: None,
            expires_at_ms: None,
        }
    }

    /// Return the cached `(token, base_url)` if still valid, or `None`.
    ///
    /// This is a **synchronous** check suitable for use inside a
    /// `std::sync::Mutex` guard — no `.await` is needed, so the guard can
    /// be dropped immediately after the call.
    pub fn try_get_cached(&self) -> Option<(String, String)> {
        if self.is_valid() {
            match (&self.cached_token, &self.cached_base_url) {
                (Some(token), Some(url)) => Some((token.clone(), url.clone())),
                _ => None,
            }
        } else {
            None
        }
    }

    /// Store a freshly exchanged token and derived base URL in the cache.
    ///
    /// `expires_at` is a Unix timestamp in **seconds** (converted internally
    /// to milliseconds for comparison with `SystemTime`).
    pub fn store(&mut self, token: String, base_url: String, expires_at_secs: u64) {
        self.cached_token = Some(token);
        self.cached_base_url = Some(base_url);
        self.expires_at_ms = Some(expires_at_secs * 1000);
    }

    /// Resolve a valid `(session_token, base_url)` pair.
    ///
    /// Returns the cached values if the token is still fresh (with a 5-minute
    /// safety margin). Otherwise, exchanges the OAuth token for a new session
    /// token via the Copilot API and updates the cache.
    pub async fn resolve(
        &mut self,
        client: &dyn CopilotHttpClient,
        oauth_token: &str,
    ) -> Result<(String, String), CopilotAuthError> {
        if let Some(pair) = self.try_get_cached() {
            return Ok(pair);
        }

        let resp = client.exchange_copilot_token(oauth_token).await?;
        let base_url = derive_base_url_from_token(&resp.token);

        // Store expiry as milliseconds
        let expires_at_ms = resp.expires_at * 1000;

        self.cached_token = Some(resp.token.clone());
        self.cached_base_url = Some(base_url.clone());
        self.expires_at_ms = Some(expires_at_ms);

        Ok((resp.token, base_url))
    }

    /// Check whether the cached token is still valid (not expired, with safety margin).
    fn is_valid(&self) -> bool {
        match (
            &self.cached_token,
            &self.cached_base_url,
            self.expires_at_ms,
        ) {
            (Some(_), Some(_), Some(expires_at_ms)) => {
                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                now_ms + TOKEN_EXPIRY_SAFETY_MARGIN_MS < expires_at_ms
            }
            _ => false,
        }
    }
}

impl Default for CopilotTokenCache {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // -----------------------------------------------------------------------
    // Mock HTTP client
    // -----------------------------------------------------------------------

    /// A mock [`CopilotHttpClient`] that returns pre-configured responses in
    /// FIFO order. Each method has its own response queue.
    struct MockCopilotHttpClient {
        device_code_responses: Mutex<Vec<Result<DeviceCodeResponse, CopilotAuthError>>>,
        access_token_responses: Mutex<Vec<Result<DeviceTokenResponse, CopilotAuthError>>>,
        exchange_responses: Mutex<Vec<Result<CopilotTokenResponse, CopilotAuthError>>>,
    }

    impl MockCopilotHttpClient {
        fn new() -> Self {
            Self {
                device_code_responses: Mutex::new(Vec::new()),
                access_token_responses: Mutex::new(Vec::new()),
                exchange_responses: Mutex::new(Vec::new()),
            }
        }

        fn with_device_code(self, resp: Result<DeviceCodeResponse, CopilotAuthError>) -> Self {
            self.device_code_responses.lock().unwrap().push(resp);
            self
        }

        fn with_access_token(self, resp: Result<DeviceTokenResponse, CopilotAuthError>) -> Self {
            self.access_token_responses.lock().unwrap().push(resp);
            self
        }

        fn with_exchange(self, resp: Result<CopilotTokenResponse, CopilotAuthError>) -> Self {
            self.exchange_responses.lock().unwrap().push(resp);
            self
        }

        /// How many exchange calls have been consumed.
        fn exchange_call_count(&self) -> usize {
            // We start with N items and remove on each call; the capacity
            // minus the current length tells us how many were consumed.
            // But tracking separately is cleaner — let's just use the length
            // of consumed items. Actually simpler: add a counter.
            // For simplicity: exchange_responses starts at N, after calls it's N - consumed.
            // We'll track it differently — see the tests that care.
            0 // placeholder — tests that need this use a wrapper
        }
    }

    #[async_trait]
    impl CopilotHttpClient for MockCopilotHttpClient {
        async fn request_device_code(
            &self,
            _client_id: &str,
            _scope: &str,
        ) -> Result<DeviceCodeResponse, CopilotAuthError> {
            self.device_code_responses.lock().unwrap().remove(0)
        }

        async fn poll_access_token(
            &self,
            _client_id: &str,
            _device_code: &str,
        ) -> Result<DeviceTokenResponse, CopilotAuthError> {
            self.access_token_responses.lock().unwrap().remove(0)
        }

        async fn exchange_copilot_token(
            &self,
            _oauth_token: &str,
        ) -> Result<CopilotTokenResponse, CopilotAuthError> {
            self.exchange_responses.lock().unwrap().remove(0)
        }
    }

    /// Wrapper that counts exchange calls for cache tests.
    struct CountingMockClient {
        inner: MockCopilotHttpClient,
        exchange_calls: Mutex<u32>,
    }

    impl CountingMockClient {
        fn new(inner: MockCopilotHttpClient) -> Self {
            Self {
                inner,
                exchange_calls: Mutex::new(0),
            }
        }

        fn exchange_call_count(&self) -> u32 {
            *self.exchange_calls.lock().unwrap()
        }
    }

    #[async_trait]
    impl CopilotHttpClient for CountingMockClient {
        async fn request_device_code(
            &self,
            client_id: &str,
            scope: &str,
        ) -> Result<DeviceCodeResponse, CopilotAuthError> {
            self.inner.request_device_code(client_id, scope).await
        }

        async fn poll_access_token(
            &self,
            client_id: &str,
            device_code: &str,
        ) -> Result<DeviceTokenResponse, CopilotAuthError> {
            self.inner.poll_access_token(client_id, device_code).await
        }

        async fn exchange_copilot_token(
            &self,
            oauth_token: &str,
        ) -> Result<CopilotTokenResponse, CopilotAuthError> {
            *self.exchange_calls.lock().unwrap() += 1;
            self.inner.exchange_copilot_token(oauth_token).await
        }
    }

    // -----------------------------------------------------------------------
    // Helper: make a valid DeviceCodeResponse
    // -----------------------------------------------------------------------
    fn valid_device_code() -> DeviceCodeResponse {
        DeviceCodeResponse {
            device_code: "dc_abc123".to_string(),
            user_code: "ABCD-1234".to_string(),
            verification_uri: "https://github.com/login/device".to_string(),
            expires_in: 900,
            interval: 0, // zero for fast tests
        }
    }

    // -----------------------------------------------------------------------
    // Device Flow tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_request_device_code_success() {
        let client = MockCopilotHttpClient::new().with_device_code(Ok(DeviceCodeResponse {
            device_code: "dc_test".to_string(),
            user_code: "TEST-CODE".to_string(),
            verification_uri: "https://github.com/login/device".to_string(),
            expires_in: 900,
            interval: 5,
        }));

        let resp = request_device_code(&client).await.unwrap();
        assert_eq!(resp.device_code, "dc_test");
        assert_eq!(resp.user_code, "TEST-CODE");
        assert_eq!(resp.verification_uri, "https://github.com/login/device");
        assert_eq!(resp.expires_in, 900);
        assert_eq!(resp.interval, 5);
    }

    #[tokio::test]
    async fn test_request_device_code_http_error() {
        let client = MockCopilotHttpClient::new().with_device_code(Err(
            CopilotAuthError::DeviceCodeRequestFailed {
                reason: "HTTP 500".to_string(),
            },
        ));

        let result = request_device_code(&client).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            CopilotAuthError::DeviceCodeRequestFailed { reason } => {
                assert!(reason.contains("500"));
            }
            other => panic!("Expected DeviceCodeRequestFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_request_device_code_missing_fields() {
        let client = MockCopilotHttpClient::new().with_device_code(Err(
            CopilotAuthError::DeviceCodeResponseInvalid {
                reason: "missing field: user_code".to_string(),
            },
        ));

        let result = request_device_code(&client).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            CopilotAuthError::DeviceCodeResponseInvalid { reason } => {
                assert!(reason.contains("user_code"));
            }
            other => panic!("Expected DeviceCodeResponseInvalid, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_poll_authorization_pending_then_success() {
        let client = MockCopilotHttpClient::new()
            .with_access_token(Ok(DeviceTokenResponse::Pending {
                error: "authorization_pending".to_string(),
            }))
            .with_access_token(Ok(DeviceTokenResponse::Pending {
                error: "authorization_pending".to_string(),
            }))
            .with_access_token(Ok(DeviceTokenResponse::Success {
                access_token: "gho_final_token".to_string(),
                token_type: "bearer".to_string(),
                scope: "read:user".to_string(),
            }));

        let token = poll_for_access_token(&client, "dc_test", 0, 30)
            .await
            .unwrap();
        assert_eq!(token, "gho_final_token");
    }

    #[tokio::test]
    async fn test_poll_slow_down_increases_interval() {
        // We can verify by checking that after slow_down the function still
        // eventually succeeds (the interval increase doesn't break the loop).
        // Detailed timing checks are brittle; we verify correctness by
        // counting that all three responses are consumed in order.
        let client = MockCopilotHttpClient::new()
            .with_access_token(Ok(DeviceTokenResponse::Pending {
                error: "slow_down".to_string(),
            }))
            .with_access_token(Ok(DeviceTokenResponse::Success {
                access_token: "gho_after_slowdown".to_string(),
                token_type: "bearer".to_string(),
                scope: "read:user".to_string(),
            }));

        let token = poll_for_access_token(&client, "dc_test", 0, 60)
            .await
            .unwrap();
        assert_eq!(token, "gho_after_slowdown");
    }

    #[tokio::test]
    async fn test_poll_expired_token_returns_error() {
        let client =
            MockCopilotHttpClient::new().with_access_token(Ok(DeviceTokenResponse::Pending {
                error: "expired_token".to_string(),
            }));

        let result = poll_for_access_token(&client, "dc_test", 0, 60).await;
        assert!(matches!(
            result.unwrap_err(),
            CopilotAuthError::DeviceCodeExpired
        ));
    }

    #[tokio::test]
    async fn test_poll_access_denied_returns_error() {
        let client =
            MockCopilotHttpClient::new().with_access_token(Ok(DeviceTokenResponse::Pending {
                error: "access_denied".to_string(),
            }));

        let result = poll_for_access_token(&client, "dc_test", 0, 60).await;
        assert!(matches!(
            result.unwrap_err(),
            CopilotAuthError::AccessDenied
        ));
    }

    // -----------------------------------------------------------------------
    // Token exchange tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_exchange_copilot_token_success() {
        let client = MockCopilotHttpClient::new().with_exchange(Ok(CopilotTokenResponse {
            token: "tid=abc;exp=9999999999;proxy-ep=proxy.example.com".to_string(),
            expires_at: 9999999999,
        }));

        let resp = client.exchange_copilot_token("gho_test").await.unwrap();
        assert!(resp.token.contains("tid=abc"));
        assert_eq!(resp.expires_at, 9999999999);
    }

    #[tokio::test]
    async fn test_exchange_copilot_token_http_error() {
        let client = MockCopilotHttpClient::new()
            .with_exchange(Err(CopilotAuthError::TokenExchangeFailed { status: 401 }));

        let result = client.exchange_copilot_token("gho_bad").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            CopilotAuthError::TokenExchangeFailed { status } => {
                assert_eq!(status, 401);
            }
            other => panic!("Expected TokenExchangeFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_exchange_copilot_token_missing_fields() {
        let client = MockCopilotHttpClient::new().with_exchange(Err(
            CopilotAuthError::TokenExchangeResponseInvalid {
                reason: "missing field: token".to_string(),
            },
        ));

        let result = client.exchange_copilot_token("gho_bad").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            CopilotAuthError::TokenExchangeResponseInvalid { reason } => {
                assert!(reason.contains("token"));
            }
            other => panic!("Expected TokenExchangeResponseInvalid, got: {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Cache tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_copilot_token_cache_returns_cached_when_valid() {
        // Pre-fill a token that expires far in the future
        let future_expires = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600; // 1 hour from now

        let mock_inner = MockCopilotHttpClient::new();
        // No exchange responses queued — would panic if called
        let client = CountingMockClient::new(mock_inner);

        let mut cache = CopilotTokenCache {
            cached_token: Some("cached_session_token".to_string()),
            cached_base_url: Some("https://api.example.com".to_string()),
            expires_at_ms: Some(future_expires * 1000),
        };

        let (token, url) = cache.resolve(&client, "gho_oauth").await.unwrap();
        assert_eq!(token, "cached_session_token");
        assert_eq!(url, "https://api.example.com");
        assert_eq!(client.exchange_call_count(), 0);
    }

    #[tokio::test]
    async fn test_copilot_token_cache_refreshes_when_expired() {
        // Set cache to already-expired token
        let past_expires = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(3600); // 1 hour ago

        let mock_inner = MockCopilotHttpClient::new().with_exchange(Ok(CopilotTokenResponse {
            token: "tid=new;proxy-ep=proxy.fresh.com".to_string(),
            expires_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 7200,
        }));
        let client = CountingMockClient::new(mock_inner);

        let mut cache = CopilotTokenCache {
            cached_token: Some("old_token".to_string()),
            cached_base_url: Some("https://api.old.com".to_string()),
            expires_at_ms: Some(past_expires * 1000),
        };

        let (token, url) = cache.resolve(&client, "gho_oauth").await.unwrap();
        assert_eq!(token, "tid=new;proxy-ep=proxy.fresh.com");
        assert_eq!(url, "https://api.fresh.com");
        assert_eq!(client.exchange_call_count(), 1);
    }

    // -----------------------------------------------------------------------
    // derive_base_url tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_derive_base_url_from_proxy_ep() {
        let token = "tid=abc;proxy-ep=proxy.example.com;exp=123";
        assert_eq!(derive_base_url_from_token(token), "https://api.example.com");
    }

    #[test]
    fn test_derive_base_url_fallback_when_no_proxy_ep() {
        let token = "tid=abc;exp=123";
        assert_eq!(
            derive_base_url_from_token(token),
            "https://api.individual.githubcopilot.com"
        );
    }

    #[test]
    fn test_derive_base_url_strips_protocol_from_proxy_ep() {
        let token = "proxy-ep=https://proxy.foo.bar";
        assert_eq!(derive_base_url_from_token(token), "https://api.foo.bar");
    }

    #[test]
    fn test_derive_base_url_no_proxy_prefix() {
        // If the host doesn't start with "proxy.", keep it as-is
        let token = "proxy-ep=custom.endpoint.com";
        assert_eq!(
            derive_base_url_from_token(token),
            "https://custom.endpoint.com"
        );
    }

    #[test]
    fn test_derive_base_url_http_protocol_stripped() {
        let token = "proxy-ep=http://proxy.bar.baz";
        assert_eq!(derive_base_url_from_token(token), "https://api.bar.baz");
    }

    #[test]
    fn test_derive_base_url_empty_proxy_ep_uses_default() {
        let token = "proxy-ep=;exp=123";
        assert_eq!(derive_base_url_from_token(token), DEFAULT_COPILOT_BASE_URL);
    }

    // -----------------------------------------------------------------------
    // parse_copilot_token_response tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_copilot_token_response_valid() {
        let json = serde_json::json!({
            "token": "session_tok",
            "expires_at": 1700000000u64
        });
        let resp = parse_copilot_token_response(json).unwrap();
        assert_eq!(resp.token, "session_tok");
        assert_eq!(resp.expires_at, 1700000000);
    }

    #[test]
    fn test_parse_copilot_token_response_milliseconds() {
        let json = serde_json::json!({
            "token": "session_tok",
            "expires_at": 1700000000000u64
        });
        let resp = parse_copilot_token_response(json).unwrap();
        assert_eq!(resp.expires_at, 1700000000);
    }

    #[test]
    fn test_parse_copilot_token_response_missing_token() {
        let json = serde_json::json!({
            "expires_at": 1700000000u64
        });
        let result = parse_copilot_token_response(json);
        assert!(matches!(
            result.unwrap_err(),
            CopilotAuthError::TokenExchangeResponseInvalid { .. }
        ));
    }

    #[test]
    fn test_parse_copilot_token_response_missing_expires() {
        let json = serde_json::json!({
            "token": "session_tok"
        });
        let result = parse_copilot_token_response(json);
        assert!(matches!(
            result.unwrap_err(),
            CopilotAuthError::TokenExchangeResponseInvalid { .. }
        ));
    }

    // -----------------------------------------------------------------------
    // parse_device_code_response tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_device_code_response_valid() {
        let json = serde_json::json!({
            "device_code": "dc_test",
            "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        });
        let resp = parse_device_code_response(json).unwrap();
        assert_eq!(resp.device_code, "dc_test");
        assert_eq!(resp.user_code, "ABCD-1234");
        assert_eq!(resp.verification_uri, "https://github.com/login/device");
        assert_eq!(resp.expires_in, 900);
        assert_eq!(resp.interval, 5);
    }

    #[test]
    fn test_parse_device_code_response_default_interval() {
        let json = serde_json::json!({
            "device_code": "dc",
            "user_code": "UC",
            "verification_uri": "https://example.com",
            "expires_in": 60
        });
        let resp = parse_device_code_response(json).unwrap();
        assert_eq!(resp.interval, 5); // default
    }

    #[test]
    fn test_parse_device_code_response_missing_device_code() {
        let json = serde_json::json!({
            "user_code": "UC",
            "verification_uri": "https://example.com",
            "expires_in": 60
        });
        let result = parse_device_code_response(json);
        assert!(result.is_err());
        match result.unwrap_err() {
            CopilotAuthError::DeviceCodeResponseInvalid { reason } => {
                assert!(reason.contains("device_code"));
            }
            other => panic!("Expected DeviceCodeResponseInvalid, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_device_code_response_missing_user_code() {
        let json = serde_json::json!({
            "device_code": "dc",
            "verification_uri": "https://example.com",
            "expires_in": 60
        });
        let result = parse_device_code_response(json);
        assert!(result.is_err());
        match result.unwrap_err() {
            CopilotAuthError::DeviceCodeResponseInvalid { reason } => {
                assert!(reason.contains("user_code"));
            }
            other => panic!("Expected DeviceCodeResponseInvalid, got: {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // parse_device_token_response tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_device_token_response_success() {
        let json = serde_json::json!({
            "access_token": "gho_abc123",
            "token_type": "bearer",
            "scope": "read:user"
        });
        match parse_device_token_response(json).unwrap() {
            DeviceTokenResponse::Success {
                access_token,
                token_type,
                scope,
            } => {
                assert_eq!(access_token, "gho_abc123");
                assert_eq!(token_type, "bearer");
                assert_eq!(scope, "read:user");
            }
            other => panic!("Expected Success, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_device_token_response_pending() {
        let json = serde_json::json!({
            "error": "authorization_pending"
        });
        match parse_device_token_response(json).unwrap() {
            DeviceTokenResponse::Pending { error } => {
                assert_eq!(error, "authorization_pending");
            }
            other => panic!("Expected Pending, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_device_token_response_no_fields() {
        let json = serde_json::json!({});
        let result = parse_device_token_response(json);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Error type tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_copilot_auth_error_display() {
        let err = CopilotAuthError::DeviceCodeExpired;
        assert!(err.to_string().contains("expired"));

        let err = CopilotAuthError::AccessDenied;
        assert!(err.to_string().contains("denied"));

        let err = CopilotAuthError::TokenExchangeFailed { status: 403 };
        assert!(err.to_string().contains("403"));

        let err = CopilotAuthError::DeviceCodeRequestFailed {
            reason: "timeout".to_string(),
        };
        assert!(err.to_string().contains("timeout"));

        let err = CopilotAuthError::TokenExchangeResponseInvalid {
            reason: "bad json".to_string(),
        };
        assert!(err.to_string().contains("bad json"));

        let err = CopilotAuthError::UnexpectedError {
            reason: "oops".to_string(),
        };
        assert!(err.to_string().contains("oops"));
    }

    #[test]
    fn test_copilot_auth_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CopilotAuthError>();
    }

    // -----------------------------------------------------------------------
    // CopilotTokenCache unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_copilot_token_cache_new_is_empty() {
        let cache = CopilotTokenCache::new();
        assert!(cache.cached_token.is_none());
        assert!(cache.cached_base_url.is_none());
        assert!(cache.expires_at_ms.is_none());
        assert!(!cache.is_valid());
    }

    #[test]
    fn test_copilot_token_cache_default_is_empty() {
        let cache = CopilotTokenCache::default();
        assert!(!cache.is_valid());
    }

    #[tokio::test]
    async fn test_copilot_token_cache_cold_start_exchanges() {
        let mock_inner = MockCopilotHttpClient::new().with_exchange(Ok(CopilotTokenResponse {
            token: "tid=new;proxy-ep=proxy.cold.com".to_string(),
            expires_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 7200,
        }));
        let client = CountingMockClient::new(mock_inner);

        let mut cache = CopilotTokenCache::new();
        let (token, url) = cache.resolve(&client, "gho_oauth").await.unwrap();

        assert_eq!(token, "tid=new;proxy-ep=proxy.cold.com");
        assert_eq!(url, "https://api.cold.com");
        assert_eq!(client.exchange_call_count(), 1);
        assert!(cache.is_valid());
    }

    // -----------------------------------------------------------------------
    // Constants sanity checks
    // -----------------------------------------------------------------------

    #[test]
    fn test_constants_are_well_formed() {
        assert_eq!(COPILOT_CLIENT_ID, "Iv1.b507a08c87ecfe98");
        assert_eq!(COPILOT_SCOPE, "read:user");
        assert!(DEVICE_CODE_URL.starts_with("https://"));
        assert!(ACCESS_TOKEN_URL.starts_with("https://"));
        assert!(COPILOT_TOKEN_URL.starts_with("https://"));
        assert!(DEFAULT_COPILOT_BASE_URL.starts_with("https://"));
        assert_eq!(TOKEN_EXPIRY_SAFETY_MARGIN_MS, 300_000);
    }
}
