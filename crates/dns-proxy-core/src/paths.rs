use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Shared machine-wide config directory: `%ProgramData%\DNS Proxy`
///
/// Used by both the Windows service (LocalSystem) and any interactive login
/// so settings are not tied to a single user's `%LOCALAPPDATA%`.
pub fn default_config_dir() -> Result<PathBuf> {
    Ok(program_data_root()?.join("DNS Proxy"))
}

/// Legacy per-user directory from 0.1.x: `%LOCALAPPDATA%\DNS Proxy`
pub fn legacy_config_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|p| p.join("DNS Proxy"))
}

/// Full path to shared `config.json`.
pub fn config_path() -> Result<PathBuf> {
    Ok(default_config_dir()?.join("config.json"))
}

/// Create the shared config directory if needed, grant Users modify ACL, and
/// migrate `config.json` from the legacy LOCALAPPDATA path once.
pub fn ensure_config_dir() -> Result<PathBuf> {
    let dir = default_config_dir()?;
    let created = !dir.exists();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create config dir {}", dir.display()))?;

    if created {
        // Best-effort: allow standard Users to read/write shared config.
        // Requires the creating process to have rights to set ACLs (typically elevated).
        if let Err(err) = grant_users_modify_acl(&dir) {
            tracing::warn!(
                "could not grant Users modify ACL on {}: {err}. \
                 An administrator may need to run: icacls \"{}\" /grant *S-1-5-32-545:(OI)(CI)M",
                dir.display(),
                dir.display()
            );
        }
    }

    migrate_legacy_config_once(&dir)?;
    Ok(dir)
}

fn program_data_root() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("ProgramData") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    if let Ok(p) = std::env::var("PROGRAMDATA") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    // Fallback used on atypical Windows installs.
    Ok(PathBuf::from(r"C:\ProgramData"))
}

/// Copy legacy files into ProgramData when the shared copies are missing.
fn migrate_legacy_config_once(shared_dir: &Path) -> Result<()> {
    let Some(legacy_dir) = legacy_config_dir() else {
        return Ok(());
    };
    if !legacy_dir.exists() {
        return Ok(());
    }

    for name in ["config.json", "dns-backup.json", "ui-settings.json"] {
        let dest = shared_dir.join(name);
        if dest.exists() {
            continue;
        }
        let src = legacy_dir.join(name);
        if src.exists() {
            std::fs::copy(&src, &dest).with_context(|| {
                format!(
                    "failed to migrate {} → {}",
                    src.display(),
                    dest.display()
                )
            })?;
            tracing::info!(
                "migrated {} from legacy LOCALAPPDATA path to {}",
                name,
                dest.display()
            );
        }
    }
    Ok(())
}

/// Grant BUILTIN\Users (S-1-5-32-545) modify on the directory (OI)(CI).
fn grant_users_modify_acl(dir: &Path) -> Result<()> {
    let dir_str = dir
        .to_str()
        .context("config dir path is not valid UTF-8")?;
    let output = Command::new("icacls")
        .args([dir_str, "/grant", "*S-1-5-32-545:(OI)(CI)M"])
        .output()
        .context("failed to run icacls")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("icacls failed: {stderr}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_ends_with_programdata_style() {
        let p = config_path().unwrap();
        assert!(p.ends_with(r"DNS Proxy\config.json") || p.ends_with("DNS Proxy/config.json"));
    }
}
