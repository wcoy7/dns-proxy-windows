use std::collections::HashSet;

use crate::config::{AppConfig, BlockSettings};

/// Built-in block categories shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockCategory {
    Youtube,
    Roblox,
    Social,
}

impl BlockCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Youtube => "YouTube",
            Self::Roblox => "Roblox",
            Self::Social => "Social media",
        }
    }

    pub fn domains(self) -> &'static [&'static str] {
        match self {
            Self::Youtube => &[
                "youtube.com",
                "youtu.be",
                "youtube-nocookie.com",
                "googlevideo.com",
                "ytimg.com",
                "youtubei.googleapis.com",
                "yt3.ggpht.com",
            ],
            Self::Roblox => &[
                "roblox.com",
                "roblox.cn",
                "rbxcdn.com",
                "roblox.qq.com",
                "simulpong.com",
                "rbx.com",
            ],
            Self::Social => &[
                "facebook.com",
                "fb.com",
                "fbcdn.net",
                "instagram.com",
                "cdninstagram.com",
                "twitter.com",
                "x.com",
                "t.co",
                "twimg.com",
                "tiktok.com",
                "tiktokv.com",
                "snapchat.com",
                "reddit.com",
                "redd.it",
                "redditmedia.com",
                "discord.com",
                "discordapp.com",
                "discord.gg",
            ],
        }
    }
}

/// Fast domain matcher built from current settings.
#[derive(Debug, Clone, Default)]
pub struct BlockEngine {
    /// Exact or suffix domains (lowercase, no leading dot).
    domains: HashSet<String>,
}

impl BlockEngine {
    pub fn from_config(cfg: &AppConfig) -> Self {
        Self::from_settings(&cfg.block)
    }

    pub fn from_settings(settings: &BlockSettings) -> Self {
        let mut domains = HashSet::new();
        if settings.youtube {
            for d in BlockCategory::Youtube.domains() {
                domains.insert((*d).to_ascii_lowercase());
            }
        }
        if settings.roblox {
            for d in BlockCategory::Roblox.domains() {
                domains.insert((*d).to_ascii_lowercase());
            }
        }
        if settings.social {
            for d in BlockCategory::Social.domains() {
                domains.insert((*d).to_ascii_lowercase());
            }
        }
        for d in &settings.custom {
            let cleaned = normalize_domain(d);
            if !cleaned.is_empty() {
                domains.insert(cleaned);
            }
        }
        Self { domains }
    }

    pub fn is_blocked(&self, name: &str) -> bool {
        let name = normalize_domain(name);
        if name.is_empty() {
            return false;
        }
        if self.domains.contains(&name) {
            return true;
        }
        // Suffix match: `ads.youtube.com` matches `youtube.com`
        for d in &self.domains {
            if name.ends_with(&format!(".{d}")) {
                return true;
            }
        }
        false
    }

    pub fn len(&self) -> usize {
        self.domains.len()
    }

    pub fn is_empty(&self) -> bool {
        self.domains.is_empty()
    }
}

fn normalize_domain(raw: &str) -> String {
    raw.trim()
        .trim_end_matches('.')
        .trim_start_matches("*.")
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_subdomains() {
        let mut settings = BlockSettings::default();
        settings.youtube = true;
        settings.roblox = false;
        settings.social = false;
        let engine = BlockEngine::from_settings(&settings);
        assert!(engine.is_blocked("www.youtube.com"));
        assert!(engine.is_blocked("YOUTUBE.COM."));
        assert!(!engine.is_blocked("example.com"));
    }
}
