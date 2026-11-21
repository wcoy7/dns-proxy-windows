use std::path::PathBuf;

use anyhow::{Context, Result};

/// Default config directory: `%LOCALAPPDATA%\DNS Proxy`
pub fn default_config_dir() -> Result<PathBuf> {
    let base = dirs::data_local_dir().context("could not resolve LOCALAPPDATA")?;
    Ok(base.join("DNS Proxy"))
}

/// Full path to `config.json`.
pub fn config_path() -> Result<PathBuf> {
    Ok(default_config_dir()?.join("config.json"))
}

/// Create the config directory if it does not exist.
pub fn ensure_config_dir() -> Result<PathBuf> {
    let dir = default_config_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create config dir {}", dir.display()))?;
    Ok(dir)
}
