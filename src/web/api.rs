//! HTTP API of the dashboard.
//!
//! | Method | Path | Purpose |
//! |---|---|---|
//! | GET | `/api/coach` | coach feed (page 3) |
//! | GET | `/api/map` | track map |
//! | GET | `/api/perf?car=<code>` | measurements of a car (page 4) |
//! | GET/POST | `/api/settings` | server settings (`config.toml`) |

use super::Feeds;
use crate::config::{Config, DataPaths, Language, Units};
use crate::perf;
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

#[derive(Clone)]
pub struct AppState {
    pub feeds: Feeds,
    /// Updates per second on the WebSocket
    pub hz: u32,
    /// Serve the dashboard from this folder instead of the built-in copy
    pub web_dir: Option<PathBuf>,
    pub perf: perf::Store,
    pub settings: Arc<Settings>,
}

/// Server settings that can be changed on the dashboard.
pub struct Settings {
    pub config: Mutex<Config>,
    pub config_file: PathBuf,
    pub data: DataPaths,
    /// PS5 address for the telemetry loop (`None` = search)
    pub ps5: watch::Sender<Option<Ipv4Addr>>,
    /// Port this server runs on (a changed port applies after a restart)
    pub port: u16,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/coach", get(coach))
        .route("/api/map", get(map))
        .route("/api/perf", get(perf_overview))
        .route("/api/settings", get(settings).post(update_settings))
}

fn json_body(body: String) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], body)
}

/// The coach feed is read fresh from its file on every request.
async fn coach(State(s): State<AppState>) -> impl IntoResponse {
    let path = s.settings.data.coach_feed.clone();
    json_body(std::fs::read_to_string(path).unwrap_or_else(|_| "{\"items\":[]}".into()))
}

async fn map(State(s): State<AppState>) -> impl IntoResponse {
    json_body(s.feeds.map.borrow().to_string())
}

#[derive(Deserialize)]
struct PerfQuery {
    car: Option<i32>,
}

async fn perf_overview(State(s): State<AppState>, Query(q): Query<PerfQuery>) -> impl IntoResponse {
    let body = tokio::task::spawn_blocking(move || s.perf.overview(q.car)).await.unwrap_or_default();
    json_body(body)
}

fn settings_json(s: &Settings) -> String {
    let c = s.config.lock().unwrap_or_else(|e| e.into_inner()).clone();
    json!({
        "ps5_ip": c.ps5_ip,
        "http_port": c.http_port,
        "language": c.language,
        "units": c.units,
        "port_in_use": s.port,
        "config_file": s.config_file.display().to_string(),
        "data_dir": s.data.root.display().to_string(),
    })
    .to_string()
}

async fn settings(State(s): State<AppState>) -> impl IntoResponse {
    json_body(settings_json(&s.settings))
}

/// Fields that may be changed from the dashboard; missing fields stay as they are. The data
/// folder is only set in the file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsUpdate {
    ps5_ip: Option<String>,
    http_port: Option<u16>,
    language: Option<Language>,
    units: Option<Units>,
}

async fn update_settings(State(s): State<AppState>, Json(u): Json<SettingsUpdate>) -> impl IntoResponse {
    let st = s.settings.clone();
    let res = tokio::task::spawn_blocking(move || {
        let mut cfg = st.config.lock().unwrap_or_else(|e| e.into_inner());
        let mut new = cfg.clone();
        if let Some(ip) = u.ps5_ip {
            new.ps5_ip = ip.trim().to_string();
        }
        new.http_port = u.http_port.unwrap_or(new.http_port);
        new.language = u.language.unwrap_or(new.language);
        new.units = u.units.unwrap_or(new.units);
        new.validate()?;
        if new != *cfg {
            new.save(&st.config_file).map_err(|e| format!("{}: {e}", st.config_file.display()))?;
            if new.ps5() != cfg.ps5() {
                st.ps5.send_replace(new.ps5());
            }
            // Coach texts and the log follow the new language right away
            crate::i18n::set(new.language, new.units);
            *cfg = new;
        }
        drop(cfg);
        Ok::<_, String>(settings_json(&st))
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));
    match res {
        Ok(body) => (StatusCode::OK, json_body(body)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, json_body(json!({ "error": e }).to_string())).into_response(),
    }
}
