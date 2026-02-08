//! BMAD auto-discovery — detects BMAD installation, version, and modules.
//!
//! Scans the project's `_bmad/` directory structure to determine what
//! BMAD components are available. Used at daemon startup and by `status` command.

use std::fmt;
use std::path::{Path, PathBuf};

/// Known BMAD module directories to look for under `_bmad/`.
const KNOWN_MODULES: &[(&str, &str)] = &[
    ("bmm", "BMAD Method Module"),
    ("core", "Core Engine"),
    ("_config", "Configuration"),
    ("_memory", "Agent Memory"),
];

/// Result of scanning a project for BMAD installation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BmadDiscovery {
    /// BMAD version string if detected (from config.yaml or package metadata).
    pub bmad_version: Option<String>,
    /// List of installed module names (e.g., `["bmm", "core", "_config"]`).
    pub installed_modules: Vec<String>,
    /// Path to the BMAD config file if found.
    pub config_path: Option<PathBuf>,
    /// The project root that was scanned.
    pub project_root: PathBuf,
    /// Whether a valid `_bmad` directory was found at all.
    pub bmad_detected: bool,
}

impl BmadDiscovery {
    /// Scan the project root for BMAD installation details.
    ///
    /// This function never fails — missing directories or unreadable files
    /// result in `None`/empty values, not errors. The daemon should always
    /// start even if BMAD discovery finds nothing.
    pub fn discover(project_root: &Path) -> Self {
        let bmad_dir = project_root.join("_bmad");
        let bmad_detected = bmad_dir.is_dir();

        if !bmad_detected {
            return Self {
                bmad_version: None,
                installed_modules: Vec::new(),
                config_path: None,
                project_root: project_root.to_path_buf(),
                bmad_detected: false,
            };
        }

        // Detect installed modules
        let installed_modules: Vec<String> = KNOWN_MODULES
            .iter()
            .filter(|(dir, _)| bmad_dir.join(dir).is_dir())
            .map(|(dir, _)| (*dir).to_string())
            .collect();

        // Try to find and parse BMAD config for version info
        let config_path = bmad_dir.join("bmm/config.yaml");
        let (bmad_version, config_path) = if config_path.is_file() {
            let version = Self::extract_version(&config_path);
            (version, Some(config_path))
        } else {
            (None, None)
        };

        Self {
            bmad_version,
            installed_modules,
            config_path,
            project_root: project_root.to_path_buf(),
            bmad_detected: true,
        }
    }

    /// Extract version from BMAD config.yaml.
    ///
    /// Looks for a line like `# Version: X.Y.Z` in comments or a `version:` field.
    fn extract_version(config_path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(config_path).ok()?;

        // Try comment-style version first: "# Version: X.Y.Z"
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("# Version:") {
                let version = rest.trim();
                if !version.is_empty() {
                    return Some(version.to_string());
                }
            }
        }

        // Fallback: try YAML field `bmad_version:` or `version:`
        // Use simple string parsing to avoid full YAML parse dependency
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("bmad_version:") {
                let version = rest.trim().trim_matches('"').trim_matches('\'');
                if !version.is_empty() {
                    return Some(version.to_string());
                }
            }
        }

        None
    }

    /// Returns a human-readable description of each installed module.
    #[allow(dead_code)] // Utility for future stories
    pub fn module_descriptions(&self) -> Vec<(&str, &str)> {
        self.installed_modules
            .iter()
            .filter_map(|m| {
                KNOWN_MODULES
                    .iter()
                    .find(|(name, _)| *name == m.as_str())
                    .copied()
            })
            .collect()
    }
}

impl fmt::Display for BmadDiscovery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.bmad_detected {
            return write!(f, "BMAD: Not detected (no _bmad/ directory found)");
        }

        writeln!(f, "BMAD: Detected")?;
        if let Some(ref version) = self.bmad_version {
            writeln!(f, "  Version: {version}")?;
        } else {
            writeln!(f, "  Version: unknown")?;
        }
        writeln!(
            f,
            "  Modules: {}",
            if self.installed_modules.is_empty() {
                "none".to_string()
            } else {
                self.installed_modules.join(", ")
            }
        )?;
        if let Some(ref path) = self.config_path {
            writeln!(f, "  Config: {}", path.display())?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_discover_with_valid_bmad_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let bmad_dir = tmp.path().join("_bmad");
        fs::create_dir_all(bmad_dir.join("bmm")).unwrap();
        fs::create_dir_all(bmad_dir.join("core")).unwrap();

        // Create a config.yaml with version comment
        let config_content = "# Version: 6.0.0-Beta.7\nproject_name: test\n";
        fs::write(bmad_dir.join("bmm/config.yaml"), config_content).unwrap();

        let discovery = BmadDiscovery::discover(tmp.path());
        assert!(discovery.bmad_detected);
        assert_eq!(discovery.bmad_version, Some("6.0.0-Beta.7".to_string()));
        assert!(discovery.installed_modules.contains(&"bmm".to_string()));
        assert!(discovery.installed_modules.contains(&"core".to_string()));
        assert!(discovery.config_path.is_some());
    }

    #[test]
    fn test_discover_without_bmad_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let discovery = BmadDiscovery::discover(tmp.path());
        assert!(!discovery.bmad_detected);
        assert!(discovery.bmad_version.is_none());
        assert!(discovery.installed_modules.is_empty());
        assert!(discovery.config_path.is_none());
    }

    #[test]
    fn test_discover_with_partial_bmad_no_config() {
        let tmp = tempfile::tempdir().unwrap();
        let bmad_dir = tmp.path().join("_bmad");
        fs::create_dir_all(bmad_dir.join("core")).unwrap();

        let discovery = BmadDiscovery::discover(tmp.path());
        assert!(discovery.bmad_detected);
        assert!(discovery.bmad_version.is_none());
        assert!(discovery.installed_modules.contains(&"core".to_string()));
        assert!(!discovery.installed_modules.contains(&"bmm".to_string()));
        assert!(discovery.config_path.is_none());
    }

    #[test]
    fn test_extract_version_from_comment() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.yaml");
        fs::write(&config_path, "# Version: 5.1.2\nkey: value\n").unwrap();
        assert_eq!(
            BmadDiscovery::extract_version(&config_path),
            Some("5.1.2".to_string())
        );
    }

    #[test]
    fn test_extract_version_returns_none_for_missing_version() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.yaml");
        fs::write(&config_path, "key: value\n").unwrap();
        assert_eq!(BmadDiscovery::extract_version(&config_path), None);
    }

    #[test]
    fn test_extract_version_from_bmad_version_field() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.yaml");
        fs::write(&config_path, "bmad_version: \"7.0.0\"\nkey: value\n").unwrap();
        assert_eq!(
            BmadDiscovery::extract_version(&config_path),
            Some("7.0.0".to_string())
        );
    }

    #[test]
    fn test_extract_version_returns_none_for_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("nonexistent.yaml");
        assert_eq!(BmadDiscovery::extract_version(&config_path), None);
    }

    #[test]
    fn test_display_with_bmad_detected() {
        let discovery = BmadDiscovery {
            bmad_version: Some("6.0.0".to_string()),
            installed_modules: vec!["bmm".to_string(), "core".to_string()],
            config_path: Some(PathBuf::from("_bmad/bmm/config.yaml")),
            project_root: PathBuf::from("."),
            bmad_detected: true,
        };
        let output = format!("{discovery}");
        assert!(output.contains("Detected"));
        assert!(output.contains("6.0.0"));
        assert!(output.contains("bmm, core"));
    }

    #[test]
    fn test_display_without_bmad() {
        let discovery = BmadDiscovery {
            bmad_version: None,
            installed_modules: Vec::new(),
            config_path: None,
            project_root: PathBuf::from("."),
            bmad_detected: false,
        };
        let output = format!("{discovery}");
        assert!(output.contains("Not detected"));
    }

    #[test]
    fn test_display_unknown_version() {
        let discovery = BmadDiscovery {
            bmad_version: None,
            installed_modules: vec!["core".to_string()],
            config_path: None,
            project_root: PathBuf::from("."),
            bmad_detected: true,
        };
        let output = format!("{discovery}");
        assert!(output.contains("unknown"));
    }

    #[test]
    fn test_display_no_modules() {
        let discovery = BmadDiscovery {
            bmad_version: None,
            installed_modules: Vec::new(),
            config_path: None,
            project_root: PathBuf::from("."),
            bmad_detected: true,
        };
        let output = format!("{discovery}");
        assert!(output.contains("none"));
    }

    #[test]
    fn test_module_descriptions_returns_known_modules() {
        let discovery = BmadDiscovery {
            bmad_version: None,
            installed_modules: vec!["bmm".to_string(), "core".to_string()],
            config_path: None,
            project_root: PathBuf::from("."),
            bmad_detected: true,
        };
        let descs = discovery.module_descriptions();
        assert_eq!(descs.len(), 2);
        assert_eq!(descs[0], ("bmm", "BMAD Method Module"));
        assert_eq!(descs[1], ("core", "Core Engine"));
    }

    #[test]
    fn test_module_descriptions_ignores_unknown_modules() {
        let discovery = BmadDiscovery {
            bmad_version: None,
            installed_modules: vec!["unknown_module".to_string()],
            config_path: None,
            project_root: PathBuf::from("."),
            bmad_detected: true,
        };
        let descs = discovery.module_descriptions();
        assert!(descs.is_empty());
    }

    #[test]
    fn test_discover_detects_config_and_memory_modules() {
        let tmp = tempfile::tempdir().unwrap();
        let bmad_dir = tmp.path().join("_bmad");
        fs::create_dir_all(bmad_dir.join("_config")).unwrap();
        fs::create_dir_all(bmad_dir.join("_memory")).unwrap();

        let discovery = BmadDiscovery::discover(tmp.path());
        assert!(discovery.bmad_detected);
        assert!(discovery.installed_modules.contains(&"_config".to_string()));
        assert!(discovery.installed_modules.contains(&"_memory".to_string()));
        assert!(!discovery.installed_modules.contains(&"bmm".to_string()));
    }
}
