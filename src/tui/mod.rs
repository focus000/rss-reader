use crate::config::{Config, Feed};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use html2text::from_read;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use rss::Channel;
use rss::Item;
use std::io::{self, Stdout};
use url::Url;

#[derive(PartialEq)]
pub enum Screen {
    Feeds,
    Items,
    Article,
}

pub struct App {
    pub config: Option<Config>,
    pub feeds: Vec<Feed>,
    pub current_feed: Option<Channel>,
    pub current_items: Vec<Item>,
    pub current_screen: Screen,
    pub feed_state: ListState,
    pub item_state: ListState,
    pub should_quit: bool,
    pub status_message: String,
    pub scroll_offset: u16,
    pub is_loading: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            config: None,
            feeds: Vec::new(),
            current_feed: None,
            current_items: Vec::new(),
            current_screen: Screen::Feeds,
            feed_state: ListState::default(),
            item_state: ListState::default(),
            should_quit: false,
            status_message: String::from("Press 'q' to quit, 'Enter' to select, 'Esc' to go back"),
            scroll_offset: 0,
            is_loading: false,
        }
    }

    pub fn with_config(config: Config) -> Self {
        let mut app = Self::new();
        app.feeds = config.get_all_feeds();
        app.config = Some(config);
        if !app.feeds.is_empty() {
            app.feed_state.select(Some(0));
        }
        app
    }

    // Helper for direct URL/RSSHub usage (backward compatibility)
    pub fn with_channel(channel: Channel) -> Self {
        let items = channel.items().to_vec();
        let mut app = Self::new();
        app.current_feed = Some(channel);
        app.current_items = items;
        app.current_screen = Screen::Items;
        if !app.current_items.is_empty() {
            app.item_state.select(Some(0));
        }
        app
    }

    pub async fn fetch_feed(
        &mut self,
        url_or_route: String,
        is_rsshub: bool,
        rsshub_host: Option<String>,
    ) -> Result<()> {
        self.is_loading = true;
        self.status_message = format!("Fetching {}...", url_or_route);

        // Note: In a real async TUI, we'd spawn a task and send a message back.
        // For simplicity here, we're doing a blocking wait which freezes the UI.

        let url_res: Result<String> = if is_rsshub {
            if let Some(host) = rsshub_host {
                let base = Url::parse(&host)?;
                let route_clean = if !url_or_route.starts_with('/') {
                    format!("/{}", url_or_route)
                } else {
                    url_or_route
                };
                let url = base.join(&route_clean)?;
                Ok(url.to_string())
            } else {
                Ok(url_or_route)
            }
        } else {
            Ok(url_or_route)
        };

        let url = match url_res {
            Ok(url) => url,
            Err(e) => {
                self.is_loading = false;
                self.status_message = format!("Error: {}", e);
                return Err(e);
            }
        };

        let client = reqwest::Client::new();
        let response_res = client.get(&url).send().await;

        let channel_res = match response_res {
            Ok(response) => {
                if !response.status().is_success() {
                    Err(anyhow::anyhow!(
                        "Failed to fetch feed: {}",
                        response.status()
                    ))
                } else {
                    match response.bytes().await {
                        Ok(content) => match Channel::read_from(std::io::Cursor::new(content)) {
                            Ok(channel) => Ok(channel),
                            Err(e) => Err(anyhow::anyhow!(e)),
                        },
                        Err(e) => Err(anyhow::anyhow!(e)),
                    }
                }
            }
            Err(e) => Err(anyhow::anyhow!(e)),
        };

        match channel_res {
            Ok(channel) => {
                self.current_items = channel.items().to_vec();
                self.current_feed = Some(channel);
                self.is_loading = false;
                self.status_message =
                    String::from("Loaded feed. Press 'Enter' to view article, 'Esc' to back.");
                self.current_screen = Screen::Items;
                self.item_state.select(Some(0));
                Ok(())
            }
            Err(e) => {
                self.is_loading = false;
                self.status_message = format!("Error: {}", e);
                Err(e)
            }
        }
    }

    pub fn next(&mut self) {
        match self.current_screen {
            Screen::Feeds => {
                if self.feeds.is_empty() {
                    return;
                }
                let i = match self.feed_state.selected() {
                    Some(i) => {
                        if i >= self.feeds.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                self.feed_state.select(Some(i));
            }
            Screen::Items => {
                if self.current_items.is_empty() {
                    return;
                }
                let i = match self.item_state.selected() {
                    Some(i) => {
                        if i >= self.current_items.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                self.item_state.select(Some(i));
            }
            Screen::Article => {
                self.scroll_down();
            }
        }
    }

    pub fn previous(&mut self) {
        match self.current_screen {
            Screen::Feeds => {
                if self.feeds.is_empty() {
                    return;
                }
                let i = match self.feed_state.selected() {
                    Some(i) => {
                        if i == 0 {
                            self.feeds.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                self.feed_state.select(Some(i));
            }
            Screen::Items => {
                if self.current_items.is_empty() {
                    return;
                }
                let i = match self.item_state.selected() {
                    Some(i) => {
                        if i == 0 {
                            self.current_items.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                self.item_state.select(Some(i));
            }
            Screen::Article => {
                self.scroll_up();
            }
        }
    }

    pub async fn select(&mut self) {
        match self.current_screen {
            Screen::Feeds => {
                if let Some(i) = self.feed_state.selected() {
                    if let Some(feed) = self.feeds.get(i) {
                        let is_rsshub = feed.is_rsshub;
                        let host = feed.rsshub_host.clone();

                        if let Err(e) = self.fetch_feed(feed.url.clone(), is_rsshub, host).await {
                            // Status message is set in fetch_feed on error for more specific details
                            if self.status_message.starts_with("Fetching") {
                                self.status_message = format!("Error: {}", e);
                            }
                            self.is_loading = false;
                        }
                    }
                }
            }
            Screen::Items => {
                if self.item_state.selected().is_some() {
                    self.current_screen = Screen::Article;
                    self.scroll_offset = 0;
                    self.status_message =
                        String::from("Reading article. Press 'Esc' or 'q' to back.");
                }
            }
            Screen::Article => {}
        }
    }

    pub fn back(&mut self) {
        match self.current_screen {
            Screen::Article => {
                self.current_screen = Screen::Items;
                self.status_message =
                    String::from("Feed items. Press 'Enter' to read, 'Esc' to feeds.");
            }
            Screen::Items => {
                // Only go back to feeds if we have a config (navigating via config)
                // If loaded directly from CLI args, we probably just want to quit or stay?
                // For now assuming if config exists, we go back.
                if self.config.is_some() {
                    self.current_screen = Screen::Feeds;
                    self.current_feed = None;
                    self.current_items.clear();
                    self.status_message = String::from("Select a feed. Press 'Enter' to open.");
                } else {
                    // Direct mode, just quit? or do nothing?
                    // Let's do nothing or maybe just quit
                    // self.should_quit = true;
                }
            }
            Screen::Feeds => {
                self.should_quit = true;
            }
        }
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }
}

pub async fn run_tui(mut app: App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(err) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(err.into());
    }
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(err) => {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            return Err(err.into());
        }
    };

    let res = run_app(&mut terminal, &mut app).await;
    let cleanup_res = restore_terminal(&mut terminal);

    if let Err(err) = res {
        let _ = cleanup_res;
        return Err(err);
    }

    cleanup_res?;
    Ok(())
}

fn restore_terminal(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        // Very basic polling. For true async, we need a better event loop.
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => {
                            if app.current_screen == Screen::Article {
                                app.back();
                            } else {
                                app.should_quit = true;
                            }
                        }
                        KeyCode::Esc => {
                            app.back();
                        }
                        KeyCode::Enter => {
                            app.select().await;
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            app.next();
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            app.previous();
                        }
                        KeyCode::Char('d') | KeyCode::PageDown => {
                            app.scroll_down();
                        }
                        KeyCode::Char('u') | KeyCode::PageUp => {
                            app.scroll_up();
                        }
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
        .split(f.area());

    let main_area = chunks[0];
    let status_area = chunks[1];

    match app.current_screen {
        Screen::Feeds => {
            let items: Vec<ListItem> = app
                .feeds
                .iter()
                .map(|feed| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{} ", feed.name),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(format!("({})", feed.url), Style::default().fg(Color::Gray)),
                    ]))
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Feeds Configuration"),
                )
                .highlight_style(
                    Style::default()
                        .add_modifier(Modifier::BOLD)
                        .fg(Color::Yellow),
                )
                .highlight_symbol(">> ");

            f.render_stateful_widget(list, main_area, &mut app.feed_state);
        }
        Screen::Items => {
            let title = if let Some(channel) = &app.current_feed {
                channel.title().to_string()
            } else {
                "Feed Items".to_string()
            };

            let items: Vec<ListItem> = app
                .current_items
                .iter()
                .map(|i| {
                    let title = i.title().unwrap_or("No Title");
                    ListItem::new(Line::from(Span::raw(title)))
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(title))
                .highlight_style(
                    Style::default()
                        .add_modifier(Modifier::BOLD)
                        .fg(Color::Yellow),
                )
                .highlight_symbol(">> ");

            f.render_stateful_widget(list, main_area, &mut app.item_state);
        }
        Screen::Article => {
            let selected_item = app
                .item_state
                .selected()
                .and_then(|i| app.current_items.get(i));

            let details_text = if let Some(item) = selected_item {
                let mut lines = Vec::new();
                lines.push(Line::from(vec![
                    Span::styled("Title: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(item.title().unwrap_or("No Title")),
                ]));

                if let Some(link) = item.link() {
                    lines.push(Line::from(vec![
                        Span::styled("Link: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(link),
                    ]));
                }

                if let Some(pub_date) = item.pub_date() {
                    lines.push(Line::from(vec![
                        Span::styled("Date: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(pub_date),
                    ]));
                }

                lines.push(Line::from(""));

                // Show content if available (often fuller than description)
                if let Some(content) = item.content() {
                    lines.push(Line::from(""));
                    lines.extend(html_to_lines(content, main_area.width));
                } else if let Some(desc) = item.description() {
                    lines.push(Line::from(""));
                    lines.extend(html_to_lines(desc, main_area.width));
                }

                lines
            } else {
                vec![Line::from("No item selected")]
            };

            let paragraph = Paragraph::new(details_text)
                .block(Block::default().borders(Borders::ALL).title("Article View"))
                .wrap(Wrap { trim: true })
                .scroll((app.scroll_offset, 0));

            f.render_widget(paragraph, main_area);
        }
    }

    // Status Bar
    let status_paragraph = Paragraph::new(app.status_message.clone())
        .block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(status_paragraph, status_area);
}

fn html_to_lines(html: &str, width: u16) -> Vec<Line<'_>> {
    let safe_width = usize::from(width.max(1));
    let text = match from_read(html.as_bytes(), safe_width) {
        Ok(text) => text,
        Err(_) => html.to_string(),
    };
    text.lines()
        .map(|line| Line::from(line.to_string()))
        .collect()
}
