use std::{path::Path, time::Duration};

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, Utc};
use html2text::from_read;
use rss::Channel;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqlitePool,
};

pub fn default_db_path() -> std::path::PathBuf {
    Path::new("rss_reader.db").to_path_buf()
}

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn initialize(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .context("Failed to connect to SQLite database")?;

        let db = Self { pool };
        db.run_migrations().await?;
        Ok(db)
    }

    async fn run_migrations(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS articles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                feed_name TEXT NOT NULL,
                feed_url TEXT NOT NULL,
                item_title TEXT NOT NULL,
                item_link TEXT,
                published_at TEXT,
                content_markdown TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(feed_url, item_link, item_title, published_at)
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create articles table")?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_articles_published_at ON articles(published_at);
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create published_at index")?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_articles_feed_name ON articles(feed_name);
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create feed_name index")?;

        Ok(())
    }

    pub async fn store_channel(
        &self,
        feed_name: &str,
        feed_url: &str,
        channel: &Channel,
    ) -> Result<()> {
        for item in channel.items() {
            let title = item.title().unwrap_or("No Title");
            let link = item.link().map(|l| l.to_string());
            let published_at = parse_pub_date(item.pub_date());
            let content_markdown = extract_markdown(item);
            let created_at = Utc::now().to_rfc3339();

            sqlx::query(
                r#"
                INSERT OR IGNORE INTO articles (
                    feed_name,
                    feed_url,
                    item_title,
                    item_link,
                    published_at,
                    content_markdown,
                    created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);
                "#,
            )
            .bind(feed_name)
            .bind(feed_url)
            .bind(title)
            .bind(link)
            .bind(published_at)
            .bind(content_markdown)
            .bind(created_at)
            .execute(&self.pool)
            .await
            .context("Failed to insert article into database")?;
        }
        Ok(())
    }
}

fn extract_markdown(item: &rss::Item) -> String {
    if let Some(content) = item.content() {
        html_to_markdown(content)
    } else if let Some(description) = item.description() {
        html_to_markdown(description)
    } else {
        String::from("")
    }
}

fn html_to_markdown(html: &str) -> String {
    from_read(html.as_bytes(), 80).unwrap_or_else(|_| html.to_string())
}

fn parse_pub_date(input: Option<&str>) -> Option<String> {
    input.and_then(|raw| {
        DateTime::parse_from_rfc2822(raw)
            .or_else(|_| DateTime::parse_from_rfc3339(raw))
            .ok()
            .map(|dt| dt.with_timezone(&Utc).to_rfc3339())
    })
}
