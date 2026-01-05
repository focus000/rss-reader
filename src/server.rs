use anyhow::{Context, Result};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use rss::Channel;
use serde::Serialize;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::Mutex;
use tower_http::services::ServeDir;

use crate::{
    config::{Config, Feed},
    db, feed,
};

#[derive(Clone)]
struct AppState {
    feeds: Vec<Feed>,
    cache: Arc<Mutex<Vec<Option<Channel>>>>,
    db: db::Database,
}

#[derive(Serialize, Clone)]
struct FeedInfo {
    name: String,
    url: String,
    is_rsshub: bool,
}

#[derive(Serialize, Clone)]
struct FeedResponse {
    title: String,
    description: Option<String>,
    items: Vec<ItemMeta>,
}

#[derive(Serialize, Clone)]
struct ItemMeta {
    id: usize,
    title: String,
    link: Option<String>,
    pub_date: Option<String>,
}

#[derive(Serialize, Clone)]
struct ItemContent {
    title: String,
    link: Option<String>,
    pub_date: Option<String>,
    content_html: String,
}

pub async fn run_server(
    config: Config,
    host: String,
    port: u16,
    open_browser: bool,
    database: db::Database,
) -> Result<()> {
    let feeds = config.get_all_feeds();
    let cache = vec![None; feeds.len()];
    let state = AppState {
        feeds,
        cache: Arc::new(Mutex::new(cache)),
        db: database,
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/api/feeds", get(list_feeds))
        .route("/api/feeds/:index", get(get_feed))
        .route("/api/feeds/:index/items/:item_index", get(get_item))
        .nest_service(
            "/images",
            ServeDir::new(db::default_store_dir().join("images")),
        )
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .context("Invalid host/port")?;
    let url = format!("http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Server running at {}", url);
    if open_browser {
        let _ = open::that(&url);
    }
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn list_feeds(State(state): State<AppState>) -> Json<Vec<FeedInfo>> {
    let feeds = state
        .feeds
        .iter()
        .map(|feed| FeedInfo {
            name: feed.name.clone(),
            url: feed.url.clone(),
            is_rsshub: feed.is_rsshub,
        })
        .collect();
    Json(feeds)
}

async fn get_feed(Path(index): Path<usize>, State(state): State<AppState>) -> impl IntoResponse {
    let feed = match state.feeds.get(index) {
        Some(feed) => feed.clone(),
        None => return (StatusCode::NOT_FOUND, "Feed not found").into_response(),
    };

    let channel = match get_or_fetch_channel(index, &feed, &state).await {
        Ok(channel) => channel,
        Err(response) => return response,
    };

    let db = state.db.clone();
    let feed_name = feed.name.clone();
    let feed_url = feed.url.clone();
    let channel_clone = channel.clone();
    tokio::spawn(async move {
        let _ = db
            .store_channel(&feed_name, &feed_url, &channel_clone)
            .await;
    });

    Json(channel_to_response(&channel)).into_response()
}

async fn get_item(
    Path((index, item_index)): Path<(usize, usize)>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let feed = match state.feeds.get(index) {
        Some(feed) => feed.clone(),
        None => return (StatusCode::NOT_FOUND, "Feed not found").into_response(),
    };

    let channel = match get_or_fetch_channel(index, &feed, &state).await {
        Ok(channel) => channel,
        Err(response) => return response,
    };

    let item = match channel.items().get(item_index) {
        Some(item) => item,
        None => return (StatusCode::NOT_FOUND, "Item not found").into_response(),
    };

    let markdown = match state.db.read_item_markdown(&feed.name, &feed.url, item) {
        Some(markdown) => markdown,
        None => {
            return Json(ItemContent {
                title: item.title().unwrap_or("No Title").to_string(),
                link: item.link().map(|s| s.to_string()),
                pub_date: item.pub_date().map(|s| s.to_string()),
                content_html: "<em>Content is still processing.</em>".to_string(),
            })
            .into_response();
        }
    };

    let content_html = if markdown.trim().is_empty() {
        "<em>No content.</em>".to_string()
    } else {
        db::render_markdown_html(&markdown)
    };

    Json(ItemContent {
        title: item.title().unwrap_or("No Title").to_string(),
        link: item.link().map(|s| s.to_string()),
        pub_date: item.pub_date().map(|s| s.to_string()),
        content_html,
    })
    .into_response()
}

async fn get_or_fetch_channel(
    index: usize,
    feed: &Feed,
    state: &AppState,
) -> Result<Channel, axum::response::Response> {
    if let Some(cached) = state.cache.lock().await.get(index).cloned().flatten() {
        return Ok(cached);
    }

    let channel = match feed::fetch_configured_feed(feed).await {
        Ok(channel) => channel,
        Err(err) => return Err((StatusCode::BAD_GATEWAY, err.to_string()).into_response()),
    };

    if let Some(slot) = state.cache.lock().await.get_mut(index) {
        *slot = Some(channel.clone());
    }

    Ok(channel)
}

fn channel_to_response(channel: &Channel) -> FeedResponse {
    let items = channel
        .items()
        .iter()
        .enumerate()
        .map(|(idx, item)| ItemMeta {
            id: idx,
            title: item.title().unwrap_or("No Title").to_string(),
            link: item.link().map(|s| s.to_string()),
            pub_date: item.pub_date().map(|s| s.to_string()),
        })
        .collect();

    FeedResponse {
        title: channel.title().to_string(),
        description: if channel.description().is_empty() {
            None
        } else {
            Some(channel.description().to_string())
        },
        items,
    }
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>RSS Reader</title>
    <style>
      :root {
        color-scheme: light;
        --bg: #f6f1e5;
        --panel: #fff8ef;
        --accent: #c05621;
        --accent-soft: #f7d9b5;
        --ink: #1f1b16;
        --muted: #7a6756;
        --border: #e4c9a6;
        --shadow: 0 10px 25px rgba(60, 30, 0, 0.12);
      }
      * {
        box-sizing: border-box;
      }
      body {
        margin: 0;
        font-family: "Georgia", "Times New Roman", serif;
        background: radial-gradient(circle at top, #fff7e9 0%, #f4e3cc 45%, #e9d2b5 100%);
        color: var(--ink);
        min-height: 100vh;
      }
      header {
        padding: 24px 32px;
        border-bottom: 2px solid var(--border);
        background: rgba(255, 248, 239, 0.8);
        backdrop-filter: blur(10px);
      }
      header h1 {
        margin: 0;
        font-size: 28px;
        letter-spacing: 1px;
      }
      header p {
        margin: 6px 0 0;
        color: var(--muted);
      }
      main {
        display: grid;
        grid-template-columns: minmax(260px, 320px) 1fr;
        gap: 20px;
        padding: 24px 32px 40px;
        align-items: stretch;
      }
      .sidebar {
        display: flex;
        flex-direction: column;
        gap: 16px;
        min-height: 70vh;
      }
      .panel,
      section.content {
        background: var(--panel);
        border: 1px solid var(--border);
        border-radius: 16px;
        box-shadow: var(--shadow);
        display: flex;
        flex-direction: column;
      }
      .panel {
        min-height: 70vh;
      }
      section h2 {
        margin: 0;
        padding: 16px 18px 12px;
        font-size: 18px;
        border-bottom: 1px solid var(--border);
        text-transform: uppercase;
        letter-spacing: 2px;
        color: var(--accent);
      }
      .list {
        list-style: none;
        margin: 0;
        padding: 0 10px 14px;
        overflow-y: auto;
      }
      .list li {
        padding: 12px 10px;
        margin: 8px 0;
        border-radius: 12px;
        cursor: pointer;
        transition: all 0.2s ease;
        border: 1px solid transparent;
      }
      .list li:hover {
        border-color: var(--accent);
        background: var(--accent-soft);
      }
      .list li.active {
        background: var(--accent);
        color: #fffaf3;
        border-color: var(--accent);
      }
      .list li small {
        display: block;
        font-size: 12px;
        color: var(--muted);
        margin-top: 4px;
      }
      .list li.active small {
        color: #ffe9cf;
      }
      .detail {
        padding: 18px 22px 28px;
        overflow-y: auto;
      }
      .detail h3 {
        margin: 0 0 6px;
        font-size: 22px;
      }
      .detail .meta {
        font-size: 13px;
        color: var(--muted);
        margin-bottom: 16px;
      }
      .detail a {
        color: var(--accent);
        text-decoration: none;
      }
      .detail a:hover {
        text-decoration: underline;
      }
      .detail .content {
        line-height: 1.6;
      }
      .detail .content p {
        margin: 0 0 12px;
      }
      .detail .content code {
        background: var(--accent-soft);
        padding: 2px 4px;
        border-radius: 4px;
        font-size: 0.9em;
      }
      .panel-header {
        display: flex;
        align-items: center;
        gap: 10px;
        padding-right: 18px;
      }
      .panel-header h2 {
        border-bottom: 0;
        padding-left: 0;
        flex: 1;
      }
      .back-button {
        margin-left: 16px;
        border: 1px solid var(--border);
        background: var(--accent-soft);
        color: var(--ink);
        border-radius: 999px;
        padding: 6px 12px;
        font-size: 12px;
        cursor: pointer;
        text-transform: uppercase;
        letter-spacing: 1px;
      }
      .back-button:hover {
        background: var(--accent);
        color: #fffaf3;
      }
      .hidden {
        display: none;
      }
      .placeholder {
        padding: 18px 22px;
        color: var(--muted);
      }
      @media (max-width: 1000px) {
        main {
          grid-template-columns: 1fr;
        }
        section.content {
          min-height: auto;
        }
      }
    </style>
  </head>
  <body>
    <header>
      <h1>RSS Reader</h1>
      <p>Sidebar navigation for feeds and items with a focused article view.</p>
    </header>
    <main>
      <aside class="sidebar">
        <div id="feedsView" class="panel">
          <h2>Feeds</h2>
          <ul id="feedList" class="list"></ul>
        </div>
        <div id="itemsView" class="panel hidden">
          <div class="panel-header">
            <button id="backToFeeds" class="back-button">Back</button>
            <h2>Items</h2>
          </div>
          <ul id="itemList" class="list"></ul>
        </div>
      </aside>
      <section class="content">
        <h2>Article</h2>
        <div id="article" class="detail placeholder">Select a feed and item to read.</div>
      </section>
    </main>
    <script>
      const feedList = document.getElementById("feedList");
      const itemList = document.getElementById("itemList");
      const article = document.getElementById("article");
      const feedsView = document.getElementById("feedsView");
      const itemsView = document.getElementById("itemsView");
      const backToFeeds = document.getElementById("backToFeeds");
      let feeds = [];
      let currentFeedIndex = null;

      function clearActive(list) {
        list.querySelectorAll("li").forEach((li) => li.classList.remove("active"));
      }

      function renderFeeds() {
        feedList.innerHTML = "";
        feeds.forEach((feed, index) => {
          const li = document.createElement("li");
          li.innerHTML = `${feed.name}<small>${feed.url}</small>`;
          li.addEventListener("click", () => loadFeed(index, li));
          feedList.appendChild(li);
        });
      }

      function renderItems(items) {
        itemList.innerHTML = "";
        if (!items || items.length === 0) {
          itemList.innerHTML = "<li class='placeholder'>No items.</li>";
          article.innerHTML = "No items.";
          return;
        }
        items.forEach((item, index) => {
          const li = document.createElement("li");
          li.textContent = item.title || "Untitled";
          li.addEventListener("click", () => loadItem(item, li));
          itemList.appendChild(li);
        });
      }

      async function loadItem(item, li) {
        clearActive(itemList);
        li.classList.add("active");
        article.innerHTML = "Loading article...";
        try {
          const res = await fetch(`/api/feeds/${currentFeedIndex}/items/${item.id}`);
          if (!res.ok) {
            throw new Error(await res.text());
          }
          const content = await res.json();
          const link = content.link
            ? `<a href="${content.link}" target="_blank">Open link</a>`
            : "";
          const date = content.pub_date ? content.pub_date : "";
          article.innerHTML = `
            <h3>${content.title || "Untitled"}</h3>
            <div class="meta">${date} ${link}</div>
            <div class="content">${content.content_html}</div>
          `;
        } catch (err) {
          article.innerHTML = `<span style="color: var(--accent);">Failed to load article.</span>`;
        }
      }

      async function loadFeed(index, li) {
        clearActive(feedList);
        li.classList.add("active");
        currentFeedIndex = index;
        article.innerHTML = "Loading...";
        itemList.innerHTML = "";
        feedsView.classList.add("hidden");
        itemsView.classList.remove("hidden");
        try {
          const res = await fetch(`/api/feeds/${index}`);
          if (!res.ok) {
            throw new Error(await res.text());
          }
          const feed = await res.json();
          renderItems(feed.items);
          if (feed.items && feed.items.length) {
            const firstItem = feed.items[0];
            const firstLi = itemList.querySelector("li");
            if (firstLi) {
              loadItem(firstItem, firstLi);
            }
          }
        } catch (err) {
          article.innerHTML = `<span style="color: var(--accent);">Failed to load feed.</span>`;
        }
      }

      async function init() {
        const res = await fetch("/api/feeds");
        feeds = await res.json();
        renderFeeds();
      }

      backToFeeds.addEventListener("click", () => {
        itemsView.classList.add("hidden");
        feedsView.classList.remove("hidden");
        itemList.innerHTML = "";
        article.innerHTML = "Select a feed and item to read.";
      });

      init();
    </script>
  </body>
</html>
"#;
