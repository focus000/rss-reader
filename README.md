# RSS Reader

A terminal-based RSS reader with TUI interface, built in Rust.

## Features

- Read RSS feeds directly from URLs
- RSSHub integration for extended feed sources
- Terminal UI with keyboard navigation
- Configuration file for managing subscriptions

## Installation

```bash
cargo build --release
```

## Usage

### TUI Mode (Recommended)

```bash
cargo run -- ui
```

Uses `feeds.toml` for feed configuration.

### Direct Feed Reading

```bash
# Read from URL
cargo run -- read https://news.ycombinator.com/rss

# With TUI mode
cargo run -- read https://news.ycombinator.com/rss --tui
```

### RSSHub Routes

```bash
cargo run -- rsshub /github/trending/daily --host https://rsshub.app
```

## Configuration

Edit `feeds.toml` to manage your subscriptions:

```toml
[rsshub]
host = "https://rsshub.app"

[[rss]]
name = "Hacker News"
url = "https://news.ycombinator.com/rss"

[[rsshub_feeds]]
name = "GitHub Trending"
url = "/github/trending/daily"
```

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `Enter` | Select / Open |
| `Esc` | Go back |
| `q` | Quit |
| `d` / `PageDown` | Scroll down (article view) |
| `u` / `PageUp` | Scroll up (article view) |

## License

Apache License 2.0. See `LICENSE`.
