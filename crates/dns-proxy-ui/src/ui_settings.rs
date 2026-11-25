//! Persisted UI preferences (separate from the service `config.json`).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiSettings {
    /// When true, the control panel starts hidden in the system tray.
    #[serde(default = "default_true")]
    pub start_minimized_to_tray: bool,
    /// When quitting, restore adapter DNS from the last backup if we had switched it.
    #[serde(default = "default_true")]
    pub restore_dns_on_quit: bool,
}

fn default_true() -> bool {
    true
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            start_minimized_to_tray: true,
            restore_dns_on_quit: true,
        }
    }
}

fn settings_path() -> Result<PathBuf> {
    Ok(dns_proxy_core::default_config_dir()?.join("ui-settings.json"))
}

impl UiSettings {
    pub fn load_or_default() -> Self {
        match Self::try_load() {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!("ui-settings load failed, using defaults: {err}");
                Self::default()
            }
        }
    }

    fn try_load() -> Result<Self> {
        let path = settings_path()?;
        if !path.exists() {
            let s = Self::default();
            s.save()?;
            return Ok(s);
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let s: Self = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(s)
    }

    pub fn save(&self) -> Result<()> {
        dns_proxy_core::ensure_config_dir()?;
        let path = settings_path()?;
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(&path, raw + "\n")
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }
}
