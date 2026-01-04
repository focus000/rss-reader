use anyhow::{Context, Result};
use clap::{ArgAction, Parser, Subcommand};
use rss::Channel;
use std::io::Cursor;
use std::path::PathBuf;
use url::Url;

mod config;
mod server;
mod tui;

#[derive(Parser)]
#[command(name = "rss_reader")]
#[command(about = "A simple RSS reader CLI in Rust", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Subscribe/Read a direct RSS URL
    Read {
        /// The URL of the RSS feed
        url: String,
        /// Number of items to show
        #[arg(short, long, default_value_t = 5)]
        limit: usize,
        /// Open in TUI mode
        #[arg(long, default_value_t = false)]
        tui: bool,
    },
    /// Read from RSSHub
    Rsshub {
        /// The route (e.g., /bilibili/user/video/2267573)
        route: String,
        /// Optional RSSHub instance URL (default: https://rsshub.app)
        #[arg(long, default_value = "https://rsshub.app")]
        host: String,
        /// Number of items to show
        #[arg(short, long, default_value_t = 5)]
        limit: usize,
        /// Open in TUI mode
        #[arg(long, default_value_t = false)]
        tui: bool,
    },
    /// Open the TUI reader with feeds from config file
    Ui {
        /// Path to config file (default: feeds.toml)
        #[arg(short, long, default_value = "feeds.toml")]
        config: PathBuf,
    },
    /// Run the web server and open a browser UI
    Server {
        /// Path to config file (default: feeds.toml)
        #[arg(short, long, default_value = "feeds.toml")]
        config: PathBuf,
        /// Host to bind (default: 127.0.0.1)
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind (default: 7878)
        #[arg(long, default_value_t = 7878)]
        port: u16,
        /// Disable auto-opening the browser
        #[arg(long, action = ArgAction::SetFalse, default_value_t = true)]
        open: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Read { url, limit, tui } => {
            println!("Fetching RSS from: {}", url);
            let channel = fetch_feed(&url).await?;
            process_channel(channel, limit, tui).await?;
        }
        Commands::Rsshub {
            route,
            host,
            limit,
            tui,
        } => {
            let base = Url::parse(&host).context("Invalid host URL")?;
            let route_clean = if !route.starts_with('/') {
                format!("/{}", route)
            } else {
                route
            };
            let url = base.join(&route_clean).context("Invalid route")?;
            let url_str = url.to_string();
            println!(
                "Fetching RSSHub route: {} (full URL: {})",
                route_clean, url_str
            );
            let channel = fetch_feed(&url_str).await?;
            process_channel(channel, limit, tui).await?;
        }
        Commands::Ui { config } => {
            if !config.exists() {
                println!(
                    "Config file not found at {:?}. Creating default config.",
                    config
                );
                config::create_default_config(&config)?;
            }
            let cfg = config::Config::load(&config)?;
            tui::run_tui(tui::App::with_config(cfg)).await?;
        }
        Commands::Server {
            config,
            host,
            port,
            open,
        } => {
            if !config.exists() {
                println!(
                    "Config file not found at {:?}. Creating default config.",
                    config
                );
                config::create_default_config(&config)?;
            }
            let cfg = config::Config::load(&config)?;
            server::run_server(cfg, host, port, open).await?;
        }
    }

    Ok(())
}

async fn fetch_feed(url: &str) -> Result<Channel> {
    let client = reqwest::Client::new();

    let response = client
        .get(url)
        .send()
        .await
        .context("Failed to fetch RSS feed")?;

    if !response.status().is_success() {
        println!("Error: Received status code {}", response.status());
        let text = response.text().await.unwrap_or_default();
        println!("Response body: {}", text);
        return Err(anyhow::anyhow!("Failed to fetch RSS feed"));
    }

    let content = response
        .bytes()
        .await
        .context("Failed to read response body")?;

    let channel = Channel::read_from(Cursor::new(content)).context("Failed to parse RSS feed")?;

    Ok(channel)
}

async fn process_channel(channel: Channel, limit: usize, use_tui: bool) -> Result<()> {
    if use_tui {
        tui::run_tui(tui::App::with_channel(channel)).await?;
    } else {
        print_channel(&channel, limit);
    }
    Ok(())
}

fn print_channel(channel: &Channel, limit: usize) {
    println!("\nTitle: {}", channel.title());
    if !channel.description().is_empty() {
        println!("Description: {}", channel.description());
    }
    println!("----------------------------------------");

    for (i, item) in channel.items().iter().take(limit).enumerate() {
        println!("{}. {}", i + 1, item.title().unwrap_or("No Title"));
        if let Some(link) = item.link() {
            println!("   Link: {}", link);
        }
        if let Some(pub_date) = item.pub_date() {
            println!("   Date: {}", pub_date);
        }
        println!();
    }
}
