use anyhow::Result;
use clap::{ArgAction, Parser, Subcommand};
use rss::Channel;
use std::path::PathBuf;

mod config;
mod feed;
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
            let channel = feed::fetch_channel(&url).await?;
            process_channel(channel, limit, tui).await?;
        }
        Commands::Rsshub {
            route,
            host,
            limit,
            tui,
        } => {
            let url_str = feed::build_rsshub_url(&host, &route)?;
            println!("Fetching RSSHub route: {} (full URL: {})", route, url_str);
            let channel = feed::fetch_channel(&url_str).await?;
            process_channel(channel, limit, tui).await?;
        }
        Commands::Ui { config } => {
            let cfg = config::load_or_create_config(&config)?;
            tui::run_tui(tui::App::with_config(cfg)).await?;
        }
        Commands::Server {
            config,
            host,
            port,
            open,
        } => {
            let cfg = config::load_or_create_config(&config)?;
            server::run_server(cfg, host, port, open).await?;
        }
    }

    Ok(())
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
