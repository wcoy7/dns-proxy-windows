//! Shared configuration, upstream presets, and domain blocking for DNS Proxy.

mod blocklist;
mod config;
mod paths;
mod upstream;

pub use blocklist::{BlockCategory, BlockEngine};
pub use config::{AppConfig, BlockMode, BlockSettings, UpstreamChoice};
pub use paths::{config_path, default_config_dir, ensure_config_dir, legacy_config_dir};
pub use upstream::{UpstreamPreset, UPSTREAM_PRESETS};

/// Human-readable product name.
pub const APP_NAME: &str = "DNS Proxy";

/// Windows service internal name (sc.exe / services.msc).
pub const SERVICE_NAME: &str = "DnsProxy";

/// Windows service display name.
pub const SERVICE_DISPLAY_NAME: &str = "DNS Proxy";

/// Windows service description.
pub const SERVICE_DESCRIPTION: &str =
    "Local DNS proxy with selectable upstream resolvers and domain blocking.";
