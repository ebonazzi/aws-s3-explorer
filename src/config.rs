//! Application configuration loaded from a JSON file on disk.
//!
//! Platform paths:
//!   Linux  : `~/.config/aws-s3-explorer/config.json`
//!   Windows: `%APPDATA%\aws-s3-explorer\config.json`
//!
//! This is separate from eframe's persistence (which stores UI layout state).
//! This file stores "deployment-time" settings: things changed infrequently
//! and worth editing by hand in a text editor.
//!
//! On first run, the file is created with sane defaults.
//! Missing keys in an existing file take their default value (forward-compatible).
//! Parse errors leave the bad file on disk and fall back to defaults with a warning.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::types::UploadStorageClass;

// ── Config struct ──────────────────────────────────────────────────────────────

/// User-editable application configuration.
///
/// All fields carry `#[serde(default)]` so adding new fields in future
/// versions never breaks existing config files on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Storage class applied to every `PutObject` (upload) call.
    ///
    /// Mirrors `aws s3 sync --storage-class` / `aws s3 cp --storage-class`.
    /// Default: `STANDARD_IA` — matches the user's existing bucket setup.
    ///
    /// Edit the JSON value to change. Valid values (case-sensitive in JSON):
    ///   `"STANDARD"`, `"STANDARD_IA"`, `"ONEZONE_IA"`,
    ///   `"INTELLIGENT_TIERING"`, `"GLACIER"`, `"GLACIER_IR"`, `"DEEP_ARCHIVE"`
    #[serde(default = "default_storage_class")]
    pub upload_storage_class: UploadStorageClass,

    /// Show a confirmation dialog before executing any delete action.
    /// Default: `true` (safe). Set to `false` only if you find it annoying.
    #[serde(default = "default_true")]
    pub confirm_before_delete: bool,

    /// Maximum number of completed transfer rows kept visible in the
    /// transfer panel. Oldest rows are pruned when this limit is exceeded.
    /// Default: 500.
    #[serde(default = "default_max_completed")]
    pub max_completed_transfers_shown: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            upload_storage_class: default_storage_class(),
            confirm_before_delete: true,
            max_completed_transfers_shown: 500,
        }
    }
}

// serde requires free functions (not closures) for `default = "..."` attributes.
fn default_storage_class() -> UploadStorageClass {
    UploadStorageClass::StandardIa
}
fn default_true() -> bool {
    true
}
fn default_max_completed() -> usize {
    500
}

// ── Path resolution ────────────────────────────────────────────────────────────

/// Returns the config file path, creating parent directories if needed.
///
/// Does not create the file itself — that is done in `AppConfig::load_or_create`.
///
/// # Errors
///
/// Returns an error if the platform config directory cannot be determined or
/// if the `aws-s3-explorer` subdirectory cannot be created.
pub fn config_file_path() -> Result<PathBuf> {
    let base = dirs::config_dir()
        .context("Could not determine platform config directory (HOME not set?)")?;

    let dir = base.join("aws-s3-explorer");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Could not create config directory: {}", dir.display()))?;

    Ok(dir.join("config.json"))
}

// ── Load / save ────────────────────────────────────────────────────────────────

impl AppConfig {
    /// Load config from disk, or create a default config file if none exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the config directory cannot be created or the
    /// config file cannot be written on first run.
    pub fn load_or_create() -> Result<Self> {
        let path = config_file_path()?;

        if !path.exists() {
            let config = Self::default();
            config.save()?;
            info!("Created default config at {}", path.display());
            return Ok(config);
        }

        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("Could not read config file: {}", path.display()))?;

        match serde_json::from_str::<Self>(&raw) {
            Ok(config) => {
                info!("Loaded config from {}", path.display());
                Ok(config)
            }
            Err(e) => {
                warn!(
                    "Config file {} could not be parsed ({e}); using defaults",
                    path.display()
                );
                Ok(Self::default())
            }
        }
    }

    /// Write the current config to disk (pretty-printed JSON).
    ///
    /// # Errors
    ///
    /// Returns an error if the config file cannot be serialised or written.
    pub fn save(&self) -> Result<()> {
        let path = config_file_path()?;
        let json = serde_json::to_string_pretty(self).context("Could not serialise AppConfig")?;
        std::fs::write(&path, json)
            .with_context(|| format!("Could not write config file: {}", path.display()))?;
        Ok(())
    }
}
