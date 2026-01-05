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

### Browser Server Mode

```bash
cargo run -- server
```

Options:

```bash
cargo run -- server --host 127.0.0.1 --port 7878
cargo run -- server --open=false
```

Opens a sidebar browser UI (feeds -> items) with a focused article view. Articles are stored
as Markdown and rendered on demand.

### Storage

- Article markdown files: `data/articles/*.md`
- Image assets: `data/articles/images/`
- Index CSV: `data/articles/index.csv` with columns `time,article_name,rss_subscription_name,path`

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
