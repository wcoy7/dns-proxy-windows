use serde::{Deserialize, Serialize};

/// Built-in public DNS resolver presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpstreamPreset {
    Google,
    Cloudflare,
    OpenDns,
}

impl UpstreamPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::Cloudflare => "Cloudflare",
            Self::OpenDns => "OpenDNS",
        }
    }

    pub fn primary(self) -> &'static str {
        match self {
            Self::Google => "8.8.8.8",
            Self::Cloudflare => "1.1.1.1",
            Self::OpenDns => "208.67.222.222",
        }
    }

    pub fn secondary(self) -> &'static str {
        match self {
            Self::Google => "8.8.4.4",
            Self::Cloudflare => "1.0.0.1",
            Self::OpenDns => "208.67.220.220",
        }
    }

    pub fn servers(self) -> [&'static str; 2] {
        [self.primary(), self.secondary()]
    }
}

/// All built-in presets in UI order.
pub const UPSTREAM_PRESETS: [UpstreamPreset; 3] = [
    UpstreamPreset::Cloudflare,
    UpstreamPreset::Google,
    UpstreamPreset::OpenDns,
];
