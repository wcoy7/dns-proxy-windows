use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths::{config_path, ensure_config_dir};
use crate::upstream::UpstreamPreset;

/// How blocked domains are answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BlockMode {
    /// Return NXDOMAIN (name does not exist).
    #[default]
    NxDomain,
    /// Return an A/AAAA record pointing at the sinkhole IP.
    Sinkhole,
}

/// Selected upstream: a built-in preset or a custom host list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum UpstreamChoice {
    Preset { preset: UpstreamPreset },
    Custom { servers: Vec<String> },
}

impl Default for UpstreamChoice {
    fn default() -> Self {
        Self::Preset {
            preset: UpstreamPreset::Cloudflare,
        }
    }
}

impl UpstreamChoice {
    /// Resolve to one or more IPv4/IPv6 host strings (no ports).
    pub fn hosts(&self) -> Vec<String> {
        match self {
            Self::Preset { preset } => preset
                .servers()
                .into_iter()
                .map(str::to_string)
                .collect(),
            Self::Custom { servers } => servers.clone(),
        }
    }

    /// Upstream sockets on port 53.
    pub fn sockets(&self) -> Result<Vec<SocketAddr>> {
        let mut out = Vec::new();
        for host in self.hosts() {
            let addr = if host.contains(':') && !host.starts_with('[') {
                // Already host:port or bare IPv6 — try parse as SocketAddr first.
                if let Ok(sa) = SocketAddr::from_str(&host) {
                    out.push(sa);
                    continue;
                }
                SocketAddr::new(IpAddr::from_str(&host).with_context(|| {
                    format!("invalid upstream address '{host}'")
                })?, 53)
            } else {
                SocketAddr::new(
                    IpAddr::from_str(&host)
                        .with_context(|| format!("invalid upstream address '{host}'"))?,
                    53,
                )
            };
            out.push(addr);
        }
        if out.is_empty() {
            bail!("upstream server list is empty");
        }
        Ok(out)
    }
}

/// Domain blocking settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockSettings {
    pub youtube: bool,
    pub roblox: bool,
    pub social: bool,
    /// Extra hostnames / suffixes to block (e.g. `example.com`).
    pub custom: Vec<String>,
    pub mode: BlockMode,
    /// Used when `mode` is [`BlockMode::Sinkhole`].
    pub sinkhole_ip: String,
}

impl Default for BlockSettings {
    fn default() -> Self {
        Self {
            youtube: true,
            roblox: true,
            social: true,
            custom: Vec::new(),
            mode: BlockMode::NxDomain,
            sinkhole_ip: "0.0.0.0".into(),
        }
    }
}

/// On-disk application configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    /// Local listen address (default `127.0.0.1:53`).
    pub listen: String,
    pub upstream: UpstreamChoice,
    pub block: BlockSettings,
    /// When true, the service process also logs query decisions at debug level.
    pub verbose_logging: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:53".into(),
            upstream: UpstreamChoice::default(),
            block: BlockSettings::default(),
            verbose_logging: false,
        }
    }
}

impl AppConfig {
    pub fn listen_addr(&self) -> Result<SocketAddr> {
        SocketAddr::from_str(&self.listen)
            .with_context(|| format!("invalid listen address '{}'", self.listen))
    }

    pub fn load_or_default() -> Result<Self> {
        // Creates ProgramData dir, applies Users ACL when new, migrates legacy LOCALAPPDATA once.
        ensure_config_dir()?;
        let path = config_path()?;
        if path.exists() {
            Self::load_from(&path)
        } else {
            let cfg = Self::default();
            cfg.save()?;
            Ok(cfg)
        }
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let cfg: Self = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        ensure_config_dir()?;
        self.save_to(&config_path()?)
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(path, raw + "\n")
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let _ = self.listen_addr()?;
        let _ = self.upstream.sockets()?;
        if matches!(self.block.mode, BlockMode::Sinkhole) {
            IpAddr::from_str(&self.block.sinkhole_ip).with_context(|| {
                format!("invalid sinkhole_ip '{}'", self.block.sinkhole_ip)
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrip() {
        let cfg = AppConfig::default();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
        cfg.validate().unwrap();
    }
}
