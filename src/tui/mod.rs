use crate::{
    config::{Config, Feed},
    db, feed,
};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use minimad::{parse_text, Composite, CompositeStyle, Line as MdLine, Options};
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
    pub current_feed_name: Option<String>,
    pub current_feed_url: Option<String>,
    pub item_markdown: Vec<Option<String>>,
    pub db: Option<db::Database>,
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
            current_feed_name: None,
            current_feed_url: None,
            item_markdown: Vec::new(),
            db: None,
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

    pub fn with_config_and_db(config: Config, db: Option<db::Database>) -> Self {
        let mut app = Self::new();
        app.feeds = config.get_all_feeds();
        app.config = Some(config);
        app.db = db;
        if !app.feeds.is_empty() {
            app.feed_state.select(Some(0));
        }
        app
    }

    pub fn with_channel_and_db(
        channel: Channel,
        db: Option<db::Database>,
        feed_name: Option<String>,
        feed_url: Option<String>,
    ) -> Self {
        let items = channel.items().to_vec();
        let mut app = Self::new();
        app.current_feed = Some(channel);
        app.current_items = items;
        app.item_markdown = vec![None; app.current_items.len()];
        app.db = db;
        app.current_feed_name = feed_name;
        app.current_feed_url = feed_url;
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
        feed_name: Option<String>,
    ) -> Result<()> {
        self.is_loading = true;
        self.status_message = format!("Fetching {}...", url_or_route);

        let url_source = url_or_route.clone();
        let url_result = if is_rsshub {
            let host = rsshub_host
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("RSSHub host missing for feed"))?;
            feed::build_rsshub_url(host, &url_or_route)
        } else {
            Ok(url_or_route)
        };

        let channel_result = match url_result {
            Ok(url) => feed::fetch_channel(&url).await,
            Err(err) => Err(err),
        };

        match channel_result {
            Ok(channel) => {
                self.current_items = channel.items().to_vec();
                self.current_feed = Some(channel);
                self.current_feed_name = feed_name;
                self.current_feed_url = Some(url_source);
                self.item_markdown = vec![None; self.current_items.len()];
                self.is_loading = false;
                self.status_message =
                    String::from("Loaded feed. Press 'Enter' to view article, 'Esc' to back.");
                self.current_screen = Screen::Items;
                self.item_state.select(Some(0));

                if let (Some(db), Some(feed_name), Some(feed_url), Some(channel)) = (
                    self.db.clone(),
                    self.current_feed_name.clone(),
                    self.current_feed_url.clone(),
                    self.current_feed.clone(),
                ) {
                    tokio::spawn(async move {
                        let _ = db.store_channel(&feed_name, &feed_url, &channel).await;
                    });
                }
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
                        let feed_name = Some(feed.name.clone());

                        if let Err(e) = self
                            .fetch_feed(feed.url.clone(), is_rsshub, host, feed_name)
                            .await
                        {
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
                    self.status_message = String::from("Loading article...");
                    if let Err(e) = self.load_markdown_for_selected().await {
                        self.status_message = format!("Error: {}", e);
                        return;
                    }
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
                    self.current_feed_name = None;
                    self.current_feed_url = None;
                    self.current_items.clear();
                    self.item_markdown.clear();
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

    async fn load_markdown_for_selected(&mut self) -> Result<()> {
        let Some(index) = self.item_state.selected() else {
            return Ok(());
        };
        if self
            .item_markdown
            .get(index)
            .map(|value| value.is_some())
            .unwrap_or(false)
        {
            return Ok(());
        }

        let item = match self.current_items.get(index) {
            Some(item) => item,
            None => return Ok(()),
        };
        let feed_name = self.current_feed_name.as_deref().unwrap_or("Unknown Feed");
        let feed_url = self.current_feed_url.as_deref().unwrap_or("unknown");

        let markdown = if let Some(db) = &self.db {
            db.read_item_markdown(feed_name, feed_url, item)
        } else {
            Some(db::extract_markdown(item))
        };

        if let Some(slot) = self.item_markdown.get_mut(index) {
            *slot = markdown;
        }

        Ok(())
    }
}

pub async fn run_tui(mut app: App) -> Result<()> {
    if let (Some(db), Some(feed_name), Some(feed_url), Some(channel)) = (
        app.db.clone(),
        app.current_feed_name.clone(),
        app.current_feed_url.clone(),
        app.current_feed.clone(),
    ) {
        tokio::spawn(async move {
            let _ = db.store_channel(&feed_name, &feed_url, &channel).await;
        });
    }

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

                let markdown = app
                    .item_markdown
                    .get(app.item_state.selected().unwrap_or(0))
                    .and_then(|value| value.as_ref());
                match markdown {
                    Some(markdown) => {
                        if !markdown.trim().is_empty() {
                            lines.push(Line::from(""));
                            lines.extend(markdown_to_lines(markdown, main_area.width));
                        } else {
                            lines.push(Line::from("No content."));
                        }
                    }
                    None => {
                        lines.push(Line::from("Content is still processing..."));
                    }
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

fn markdown_to_lines(markdown: &str, width: u16) -> Vec<Line<'static>> {
    let text = parse_text(markdown, Options::default());
    let max_width = usize::from(width.max(1));
    let mut lines = Vec::new();

    for line in text.lines {
        match line {
            MdLine::Normal(composite) => lines.push(composite_to_line(composite)),
            MdLine::CodeFence(composite) => lines.push(composite_to_line(composite)),
            MdLine::TableRow(row) => {
                let row_text = row
                    .cells
                    .iter()
                    .map(|cell| composite_plain(cell))
                    .collect::<Vec<_>>()
                    .join(" | ");
                lines.push(Line::from(row_text));
            }
            MdLine::TableRule(_) | MdLine::HorizontalRule => {
                lines.push(Line::from("─".repeat(max_width)));
            }
        }
    }

    if lines.is_empty() {
        lines.push(Line::from("No content."));
    }

    lines
}

fn composite_to_line(composite: Composite<'_>) -> Line<'static> {
    let mut spans = Vec::new();
    if let Some(prefix) = composite_prefix(&composite.style) {
        spans.push(Span::styled(
            prefix.to_string(),
            Style::default().fg(Color::Gray),
        ));
    }

    for compound in composite.compounds {
        let mut style = Style::default();
        if compound.bold || matches!(composite.style, CompositeStyle::Header(_)) {
            style = style.add_modifier(Modifier::BOLD);
        }
        if compound.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if compound.strikeout {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if compound.code {
            style = style.fg(Color::Yellow);
        }
        if matches!(composite.style, CompositeStyle::Quote) {
            style = style.fg(Color::Gray);
        }
        spans.push(Span::styled(compound.src.to_string(), style));
    }

    Line::from(spans)
}

fn composite_prefix(style: &CompositeStyle) -> Option<String> {
    match style {
        CompositeStyle::ListItem(depth) => {
            let indent = "  ".repeat(*depth as usize);
            Some(format!("{indent}• "))
        }
        CompositeStyle::Quote => Some("│ ".to_string()),
        CompositeStyle::Code => Some("    ".to_string()),
        CompositeStyle::Header(level) => {
            let prefix = "#".repeat(*level as usize);
            Some(format!("{prefix} "))
        }
        CompositeStyle::Paragraph => None,
    }
}

fn composite_plain(composite: &Composite<'_>) -> String {
    composite
        .compounds
        .iter()
        .map(|compound| compound.src)
        .collect::<Vec<_>>()
        .join("")
}
