use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use comrak::{markdown_to_html, ComrakOptions};
use html2md::parse_html;
use regex::Regex;
use reqwest::header::CONTENT_TYPE;
use rss::Channel;
use sha2::{Digest, Sha256};
use url::Url;

pub fn default_store_dir() -> PathBuf {
    Path::new("data/articles").to_path_buf()
}

fn default_image_dir(store_dir: &Path) -> PathBuf {
    store_dir.join("images")
}

#[derive(Clone)]
pub struct Database {
    store_dir: PathBuf,
    index_path: PathBuf,
    image_dir: PathBuf,
}

impl Database {
    pub async fn initialize(store_dir: &Path) -> Result<Self> {
        fs::create_dir_all(store_dir).context("Failed to create article store directory")?;
        let image_dir = default_image_dir(store_dir);
        fs::create_dir_all(&image_dir).context("Failed to create image store directory")?;
        let index_path = store_dir.join("index.csv");

        let needs_header = match fs::metadata(&index_path) {
            Ok(meta) => meta.len() == 0,
            Err(err) if err.kind() == ErrorKind::NotFound => true,
            Err(err) => return Err(err.into()),
        };

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&index_path)
            .context("Failed to open index.csv")?;

        let mut writer = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(file);

        if needs_header {
            writer
                .write_record(["time", "article_name", "rss_subscription_name", "path"])
                .context("Failed to write index.csv header")?;
            writer.flush().context("Failed to flush index.csv header")?;
        }

        Ok(Self {
            store_dir: store_dir.to_path_buf(),
            index_path,
            image_dir,
        })
    }

    pub async fn store_channel(
        &self,
        feed_name: &str,
        feed_url: &str,
        channel: &Channel,
    ) -> Result<()> {
        for item in channel.items() {
            self.store_item(feed_name, feed_url, item).await?;
        }

        Ok(())
    }

    pub async fn store_item(
        &self,
        feed_name: &str,
        feed_url: &str,
        item: &rss::Item,
    ) -> Result<String> {
        let title = item.title().unwrap_or("No Title");
        let link = item.link().unwrap_or("");
        let published_at = parse_pub_date(item.pub_date());
        let time_for_hash = published_at.clone().unwrap_or_default();
        let time_for_csv = published_at.unwrap_or_else(|| Utc::now().to_rfc3339());
        let filename = item_filename(feed_name, feed_url, title, link, &time_for_hash);
        let file_path = self.store_dir.join(&filename);

        if file_path.exists() {
            let existing = fs::read_to_string(&file_path).unwrap_or_default();
            return Ok(existing);
        }

        let content_markdown = extract_markdown(item);
        let content_markdown = self.localize_images(&content_markdown).await?;

        fs::write(&file_path, content_markdown.as_bytes())
            .context("Failed to write markdown file")?;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.index_path)
            .context("Failed to open index.csv for append")?;
        let mut writer = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(file);
        writer
            .write_record([
                time_for_csv,
                title.to_string(),
                feed_name.to_string(),
                file_path.to_string_lossy().to_string(),
            ])
            .context("Failed to append index.csv row")?;
        writer.flush().context("Failed to flush index.csv")?;

        Ok(content_markdown)
    }

    pub fn read_item_markdown(
        &self,
        feed_name: &str,
        feed_url: &str,
        item: &rss::Item,
    ) -> Option<String> {
        let title = item.title().unwrap_or("No Title");
        let link = item.link().unwrap_or("");
        let published_at = parse_pub_date(item.pub_date()).unwrap_or_default();
        let filename = item_filename(feed_name, feed_url, title, link, &published_at);
        let file_path = self.store_dir.join(&filename);
        fs::read_to_string(&file_path).ok()
    }
}

pub fn extract_markdown(item: &rss::Item) -> String {
    if let Some(content) = item.content() {
        html_to_markdown(content)
    } else if let Some(description) = item.description() {
        html_to_markdown(description)
    } else {
        String::new()
    }
}

fn html_to_markdown(html: &str) -> String {
    parse_html(html)
}

pub fn render_markdown_html(markdown: &str) -> String {
    markdown_to_html(markdown, &ComrakOptions::default())
}

fn parse_pub_date(input: Option<&str>) -> Option<String> {
    input.and_then(|raw| {
        DateTime::parse_from_rfc2822(raw)
            .or_else(|_| DateTime::parse_from_rfc3339(raw))
            .ok()
            .map(|dt| dt.with_timezone(&Utc).to_rfc3339())
    })
}

fn hash_string(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

impl Database {
    async fn localize_images(&self, markdown: &str) -> Result<String> {
        let urls = extract_image_urls(markdown);
        if urls.is_empty() {
            return Ok(markdown.to_string());
        }

        let mut replacements = HashMap::new();
        for url in urls {
            if replacements.contains_key(&url) {
                continue;
            }
            if let Some(local) = self.download_image(&url).await? {
                replacements.insert(url, local);
            }
        }

        let mut updated = replace_html_img_tags(markdown, &replacements);
        for (url, local) in replacements {
            updated = updated.replace(&url, &local);
        }
        Ok(updated)
    }

    async fn download_image(&self, url: &str) -> Result<Option<String>> {
        let parsed = match Url::parse(url) {
            Ok(parsed) => parsed,
            Err(_) => return Ok(None),
        };
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Ok(None);
        }

        let filename = image_filename(url, None);
        let target_path = self.image_dir.join(&filename);
        if target_path.exists() {
            return Ok(Some(format!("/images/{}", filename)));
        }

        let client = reqwest::Client::new();
        let response = client.get(url).send().await?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string());
        let bytes = response.bytes().await?;

        let filename = image_filename(url, content_type.as_deref());
        let target_path = self.image_dir.join(&filename);
        if !target_path.exists() {
            fs::write(&target_path, &bytes).context("Failed to write image file")?;
        }

        Ok(Some(format!("/images/{}", filename)))
    }
}

fn extract_image_urls(markdown: &str) -> Vec<String> {
    let mut urls = HashSet::new();
    let md_re = Regex::new(r"!\[[^\]]*]\(([^)]+)\)").unwrap();
    let html_re = Regex::new(r#"<img[^>]+src=["']([^"']+)["'][^>]*>"#).unwrap();

    for caps in md_re.captures_iter(markdown) {
        if let Some(url) = caps.get(1) {
            urls.insert(url.as_str().to_string());
        }
    }

    for caps in html_re.captures_iter(markdown) {
        if let Some(url) = caps.get(1) {
            urls.insert(url.as_str().to_string());
        }
    }

    urls.into_iter().collect()
}

fn replace_html_img_tags(markdown: &str, replacements: &HashMap<String, String>) -> String {
    let img_tag = Regex::new(r#"<img[^>]*>"#).unwrap();
    let src_attr = Regex::new(r#"src=["']([^"']+)["']"#).unwrap();
    let alt_attr = Regex::new(r#"alt=["']([^"']*)["']"#).unwrap();

    img_tag
        .replace_all(markdown, |caps: &regex::Captures<'_>| {
            let tag = caps.get(0).map(|m| m.as_str()).unwrap_or("");
            let src = src_attr
                .captures(tag)
                .and_then(|c| c.get(1).map(|m| m.as_str()))
                .unwrap_or("");
            let alt = alt_attr
                .captures(tag)
                .and_then(|c| c.get(1).map(|m| m.as_str()))
                .unwrap_or("");
            let target = replacements.get(src).map(|s| s.as_str()).unwrap_or(src);
            format!("![{}]({})", alt, target)
        })
        .to_string()
}

fn image_filename(url: &str, content_type: Option<&str>) -> String {
    let ext = image_extension(url, content_type).unwrap_or("img");
    format!("{}.{}", hash_string(url), ext)
}

fn item_filename(feed_name: &str, feed_url: &str, title: &str, link: &str, time: &str) -> String {
    let hash_input = format!("{}|{}|{}|{}|{}", feed_name, feed_url, title, link, time);
    format!("{}.md", hash_string(&hash_input))
}

fn image_extension(url: &str, content_type: Option<&str>) -> Option<&'static str> {
    if let Ok(parsed) = Url::parse(url) {
        if let Some(ext) = Path::new(parsed.path())
            .extension()
            .and_then(|e| e.to_str())
        {
            return Some(owned_extension(ext));
        }
    }

    content_type_extension(content_type)
}

fn owned_extension(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "png",
        "jpeg" => "jpg",
        "jpg" => "jpg",
        "webp" => "webp",
        "gif" => "gif",
        "svg" => "svg",
        "svgz" => "svg",
        _ => "img",
    }
}

fn content_type_extension(content_type: Option<&str>) -> Option<&'static str> {
    match content_type {
        Some(ct) if ct.contains("image/png") => Some("png"),
        Some(ct) if ct.contains("image/jpeg") => Some("jpg"),
        Some(ct) if ct.contains("image/jpg") => Some("jpg"),
        Some(ct) if ct.contains("image/webp") => Some("webp"),
        Some(ct) if ct.contains("image/gif") => Some("gif"),
        Some(ct) if ct.contains("image/svg+xml") => Some("svg"),
        _ => None,
    }
}
