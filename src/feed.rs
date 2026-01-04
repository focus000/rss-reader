use std::io::Cursor;

use anyhow::{Context, Result};
use rss::Channel;
use url::Url;

use crate::config::Feed;

fn normalize_route(route: &str) -> String {
    if route.starts_with('/') {
        route.to_string()
    } else {
        format!("/{}", route)
    }
}

pub fn build_rsshub_url(host: &str, route: &str) -> Result<String> {
    let base = Url::parse(host).context("Invalid host URL")?;
    let route = normalize_route(route);
    Ok(base.join(&route)?.to_string())
}

pub fn build_feed_url(feed: &Feed) -> Result<String> {
    if feed.is_rsshub {
        let host = feed
            .rsshub_host
            .as_deref()
            .context("RSSHub host missing for feed")?;
        build_rsshub_url(host, &feed.url)
    } else {
        Ok(feed.url.clone())
    }
}

pub async fn fetch_channel(url: &str) -> Result<Channel> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .context("Failed to fetch RSS feed")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch RSS feed: {}",
            response.status()
        ));
    }

    let content = response
        .bytes()
        .await
        .context("Failed to read response body")?;

    Channel::read_from(Cursor::new(content)).context("Failed to parse RSS feed")
}

pub async fn fetch_configured_feed(feed: &Feed) -> Result<Channel> {
    let url = build_feed_url(feed)?;
    fetch_channel(&url).await
}
