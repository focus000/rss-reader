use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub rsshub: RssHubConfig,
    #[serde(default)]
    pub rss: Vec<FeedItem>,
    #[serde(default)]
    pub rsshub_feeds: Vec<FeedItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RssHubConfig {
    pub host: String,
}

impl Default for RssHubConfig {
    fn default() -> Self {
        Self {
            host: "https://rsshub.app".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FeedItem {
    pub name: String,
    pub url: String,
}

// Unified struct for internal use
#[derive(Debug, Clone)]
pub struct Feed {
    pub name: String,
    pub url: String,
    pub is_rsshub: bool,
    pub rsshub_host: Option<String>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content =
            fs::read_to_string(path).context(format!("Failed to read config file: {:?}", path))?;
        let config: Config = toml::from_str(&content).context("Failed to parse config file")?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(path, content).context(format!("Failed to write config file: {:?}", path))?;
        Ok(())
    }

    pub fn get_all_feeds(&self) -> Vec<Feed> {
        let mut feeds = Vec::new();

        for item in &self.rss {
            feeds.push(Feed {
                name: item.name.clone(),
                url: item.url.clone(),
                is_rsshub: false,
                rsshub_host: None,
            });
        }

        for item in &self.rsshub_feeds {
            feeds.push(Feed {
                name: item.name.clone(),
                url: item.url.clone(),
                is_rsshub: true,
                rsshub_host: Some(self.rsshub.host.clone()),
            });
        }

        feeds
    }
}

pub fn create_default_config(path: &Path) -> Result<()> {
    let config = Config {
        rsshub: RssHubConfig {
            host: "https://rsshub.app".to_string(),
        },
        rss: vec![FeedItem {
            name: "Hacker News".to_string(),
            url: "https://news.ycombinator.com/rss".to_string(),
        }],
        rsshub_feeds: vec![FeedItem {
            name: "GitHub Trending".to_string(),
            url: "/github/trending/daily".to_string(),
        }],
    };
    config.save(path)?;
    Ok(())
}
