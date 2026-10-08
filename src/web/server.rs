//! HTTP server: static dashboard files, the live WebSocket and the API routes.

use super::api::{self, AppState};
use super::Feed;
use crate::tr;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    http::header,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use std::borrow::Cow;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::time;

pub async fn serve(port: u16, state: AppState) -> std::io::Result<()> {
    let app = Router::new()
        .route("/", get(index))
        .route("/i18n.js", get(i18n_js))
        .route(
            "/manifest.webmanifest",
            get(|| async { ([(header::CONTENT_TYPE, "application/manifest+json")], include_str!("../../web/manifest.webmanifest")) }),
        )
        .route("/icon.png", get(|| async { ([(header::CONTENT_TYPE, "image/png")], include_bytes!("../../web/icon.png").as_slice()) }))
        .route(
            "/keepawake.mp4",
            get(|| async { ([(header::CONTENT_TYPE, "video/mp4")], include_bytes!("../../web/keepawake.mp4").as_slice()) }),
        )
        .route("/ws", get(ws))
        .merge(api::routes())
        .with_state(state);

    let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        std::io::Error::new(
            e.kind(),
            tr!(
                "Port {port} nicht verfügbar ({e}) – anderen Port mit --port oder in der Konfiguration wählen",
                "Port {port} not available ({e}) – choose another port with --port or in the configuration"
            ),
        )
    })?;
    let ip = local_ip().map_or_else(|| tr!("<diese-IP>", "<this-IP>"), |ip| ip.to_string());
    println!("Dashboard: http://{ip}:{port}");
    axum::serve(listener, app).await
}

/// Placeholder in index.html that is replaced by the language and units from the settings.
const SETTINGS_TAG: &str = r#"<script id="server-settings" type="application/json">{"language":"auto","units":"auto"}</script>"#;

/// A dashboard file: with `--web-dir` read from disk on every request (for development).
fn asset(s: &AppState, name: &str, builtin: &'static str) -> Cow<'static, str> {
    match s.web_dir.as_ref().map(|d| std::fs::read_to_string(d.join(name))) {
        Some(Ok(text)) => Cow::Owned(text),
        _ => Cow::Borrowed(builtin),
    }
}

async fn index(State(s): State<AppState>) -> impl IntoResponse {
    let html = asset(&s, "index.html", include_str!("../../web/index.html"));
    let (language, units) = {
        let c = s.settings.config.lock().unwrap_or_else(|e| e.into_inner());
        (c.language.as_str(), c.units.as_str())
    };
    let tag = format!(r#"<script id="server-settings" type="application/json">{{"language":"{language}","units":"{units}"}}</script>"#);
    ([(header::CACHE_CONTROL, "no-cache")], Html(html.replacen(SETTINGS_TAG, &tag, 1)))
}

async fn i18n_js(State(s): State<AppState>) -> impl IntoResponse {
    let js = asset(&s, "i18n.js", include_str!("../../web/i18n.js"));
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], js)
}

async fn ws(ws: WebSocketUpgrade, State(s): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| stream_to_client(socket, s.feeds.snapshot.clone(), s.hz))
}

/// Sends at most `hz` updates per second, always only the latest state.
async fn stream_to_client(mut socket: WebSocket, mut feed: Feed, hz: u32) {
    let mut tick = time::interval(Duration::from_secs_f64(1.0 / hz as f64));
    tick.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    feed.mark_changed();
    loop {
        tick.tick().await;
        if feed.changed().await.is_err() {
            return;
        }
        let msg = feed.borrow_and_update().to_string();
        if socket.send(Message::Text(msg.into())).await.is_err() {
            return;
        }
    }
}

/// LAN address of this computer (only for the start message, nothing is sent).
fn local_ip() -> Option<IpAddr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    s.local_addr().ok().map(|a| a.ip())
}
