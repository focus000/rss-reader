# RSS Reader Project Guide

## Commands
- **Build**: `cargo build`
- **Run**: `cargo run`
- **Test**: `cargo test`
- **Lint**: `cargo clippy`
- **Format**: `cargo fmt`
- **Clean**: `cargo clean`

## Project Structure
- `src/main.rs`: Application entry point
- `src/config.rs`: Configuration loading and feed management
- `src/tui/`: TUI interface implementation (Ratatui based)
- `feeds.toml`: Feed configuration file (RSS & RSSHub support)

## Configuration (`feeds.toml`)

### Global RSSHub Settings
```toml
[rsshub]
host = "https://rsshub.app"  # Default RSSHub instance, set once
```

### RSS Feeds
```toml
[[rss]]
name = "Hacker News"
url = "https://news.ycombinator.com/rss"
```

### RSSHub Feeds
```toml
[[rsshub_feeds]]
name = "GitHub Trending"
url = "/github/trending/daily"  # Route path, host is inherited from [rsshub]
```

## Style Guidelines
- Follow standard Rust idioms
- Use `rustfmt` for formatting
- Handle errors with `anyhow`
- Use `tokio` for async operations
