//! Coach worker: watches the recorded sessions, analyses every completed lap, writes the dashboard
//! feed (`coach/feed.json`, page 3) and a summary when a session ends.
//!
//! Runs as a thread inside the server. It works purely on the files (session.json, lap CSVs,
//! analysis/state.json), so it also catches up after a restart. The recorder wakes it up when a
//! lap file is started or a session ends; otherwise it polls every few seconds.

use super::compare::{lap_files, Comparer, Comparison};
use super::feedback::{self, LapMetrics};
use crate::tr;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

const POLL: Duration = Duration::from_secs(5);
/// Sessions without session.json (recorded before 06.10.2026) end after this idle time.
const LEGACY_IDLE: Duration = Duration::from_secs(180);
/// Open sessions without new data for this long count as ended (server crashed or stopped).
const STALE_OPEN: Duration = Duration::from_secs(1800);
const MAX_ITEMS: usize = 30;
/// Number of most recent sessions that are looked at.
const RECENT_SESSIONS: usize = 3;

/// Per-session progress, stored in `analysis/state.json`.
#[derive(Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    processed: Vec<String>,
    #[serde(default)]
    summarized_laps: usize,
    #[serde(default)]
    metrics: BTreeMap<String, LapMetrics>,
    #[serde(default)]
    laps: Vec<String>,
    #[serde(default)]
    closed: bool,
}

pub struct Coach {
    record_dir: PathBuf,
    feed_path: PathBuf,
    comparer: Comparer,
}

fn log(msg: impl AsRef<str>) {
    println!("{} Coach: {}", chrono::Local::now().format("%H:%M:%S"), msg.as_ref());
}

fn now_iso() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn line_count(path: &Path) -> usize {
    fs::read(path).map(|b| bytecount(&b)).unwrap_or(0)
}

fn bytecount(b: &[u8]) -> usize {
    b.iter().filter(|c| **c == b'\n').count()
}

fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH)
}

fn session_ended(session: &Path, idle: Duration) -> bool {
    match fs::read_to_string(session.join("session.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        Some(meta) => !meta["ended"].is_null() || idle > STALE_OPEN,
        None => idle > LEGACY_IDLE,
    }
}

impl Coach {
    pub fn new(record_dir: PathBuf, feed_path: PathBuf) -> Self {
        Coach { record_dir, feed_path, comparer: Comparer::default() }
    }

    /// Runs forever: one pass now, then on every wake-up or poll interval.
    pub fn run(mut self, wake: mpsc::Receiver<()>) {
        let dir = self.record_dir.display();
        log(tr!("gestartet ({dir})", "started ({dir})"));
        loop {
            self.tick();
            match wake.recv_timeout(POLL) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(POLL),
            }
        }
    }

    /// One pass over the most recent sessions.
    pub fn tick(&mut self) {
        let mut sessions: Vec<PathBuf> =
            fs::read_dir(&self.record_dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        sessions.sort();
        let Some(newest) = sessions.last().cloned() else { return };
        for session in sessions.iter().skip(sessions.len().saturating_sub(RECENT_SESSIONS)) {
            let is_newest = *session == newest;
            let files = lap_files(session);
            if files.is_empty() {
                continue;
            }
            let last_write = files.iter().map(|f| mtime(f)).max().unwrap_or(SystemTime::UNIX_EPOCH);
            let idle = SystemTime::now().duration_since(last_write).unwrap_or_default();
            let fin = !is_newest || session_ended(session, idle);
            let mut state = load_state(session);
            if !fin && state.closed {
                state.closed = false; // session was resumed
            }
            if fin && !is_newest && state.closed {
                continue;
            }
            if let Err(e) = self.analyze_session(session, &mut state, fin) {
                let s = session.display();
                log(tr!("Fehler in {s}: {e}", "error in {s}: {e}"));
            }
            if fin && !is_newest {
                state.closed = true;
                save_state(session, &state);
            }
        }
    }

    fn analyze_session(&mut self, session: &Path, state: &mut State, fin: bool) -> std::io::Result<()> {
        let files = lap_files(session);
        let names: Vec<String> = files.iter().map(|f| name_of(f)).collect();
        // The last file is still being written; a lap is complete once the next file has data.
        let ready: Vec<String> = if fin {
            names.clone()
        } else {
            (0..names.len().saturating_sub(1)).filter(|&i| line_count(&files[i + 1]) > 20).map(|i| names[i].clone()).collect()
        };
        let new: Vec<String> = ready.into_iter().filter(|n| !state.processed.contains(n)).collect();
        if new.is_empty() && !(fin && state.summarized_laps < state.laps.len()) {
            return Ok(());
        }
        let res: Comparison = self.comparer.compare(session)?;
        let corners: Option<Value> =
            (!res.corners.is_empty()).then(|| json!(res.corners.iter().map(|c| json!({"n": c.n, "x": c.x, "z": c.z})).collect::<Vec<_>>()));
        let dir = session.join("analysis");
        fs::create_dir_all(&dir)?;
        for name in new {
            if let Some(lap) = res.laps.iter().find(|l| l.file == name) {
                let m = feedback::lap_metrics(&session.join(&name))?;
                let prev = res.laps.iter().filter(|l| state.laps.contains(&l.file)).map(|l| l.ms).min();
                let item = feedback::lap_feedback(&res, lap, &m, prev);
                feedback::save_lap(&dir, lap, &m, &item)?;
                state.metrics.insert(name.clone(), m);
                state.laps.push(name.clone());
                self.write_feed(session, Some(&item), feedback::focus_for(&res, lap), corners.clone())?;
                let (n, title) = (lap.lap, item["title"].as_str().unwrap_or_default());
                log(tr!("Runde {n} ausgewertet: {title}", "lap {n} analysed: {title}"));
            }
            state.processed.push(name);
        }
        if fin && !res.laps.is_empty() && state.summarized_laps < state.laps.len() {
            let item = feedback::summary(session, &dir, &res, &state.metrics)?;
            let first = item["points"][0].as_str().unwrap_or_default().to_string();
            self.write_feed(session, Some(&item), Some(json!({"title": tr!("Session beendet", "Session ended"), "text": first})), corners)?;
            state.summarized_laps = state.laps.len();
            let s = session.display();
            log(tr!("Zusammenfassung geschrieben: {s}", "summary written: {s}"));
        }
        save_state(session, state);
        Ok(())
    }

    /// Updates the dashboard feed. `focus`/`corners` = `None` keeps the current value.
    fn write_feed(&self, session: &Path, item: Option<&Value>, focus: Option<Value>, corners: Option<Value>) -> std::io::Result<()> {
        let mut feed: Value =
            fs::read_to_string(&self.feed_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}));
        let name = name_of(session);
        if feed["session"].as_str() != Some(name.as_str()) {
            // New session: keep only the latest summary of the previous one
            let keep: Vec<Value> =
                feed["items"].as_array().into_iter().flatten().filter(|i| i["kind"] == "summary").take(1).cloned().collect();
            feed = json!({"session": name, "items": keep, "focus": null});
        }
        if let Some(item) = item {
            let mut items = vec![item.clone()];
            items.extend(feed["items"].as_array().into_iter().flatten().filter(|i| i["id"] != item["id"]).cloned());
            items.truncate(MAX_ITEMS);
            feed["items"] = Value::Array(items);
            let log_path = session.join("analysis").join("coach.jsonl");
            let mut line = serde_json::to_string(item).unwrap_or_default();
            line.push('\n');
            use std::io::Write;
            fs::OpenOptions::new().create(true).append(true).open(log_path)?.write_all(line.as_bytes())?;
        }
        if let Some(focus) = focus {
            feed["focus"] = focus;
        }
        if let Some(corners) = corners {
            feed["corners"] = corners;
        }
        feed["updated"] = now_iso().into();
        if let Some(parent) = self.feed_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = self.feed_path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(&feed).unwrap_or_default())?;
        fs::rename(tmp, &self.feed_path)
    }
}

fn load_state(session: &Path) -> State {
    fs::read_to_string(session.join("analysis").join("state.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_state(session: &Path, state: &State) {
    let dir = session.join("analysis");
    if fs::create_dir_all(&dir).is_ok() {
        let _ = fs::write(dir.join("state.json"), serde_json::to_string_pretty(state).unwrap_or_default());
    }
}
