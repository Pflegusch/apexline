//! Web server: the dashboard (built in, or from a folder while developing), the live WebSocket
//! feed ([`server`]) and the HTTP API ([`api`]).

pub mod api;
pub mod server;

use crate::session::tracker::{Snapshot, Tracker};
use std::sync::Arc;
use tokio::sync::watch;

pub type Feed = watch::Receiver<Arc<str>>;

/// Latest JSON of the live snapshot and of the track map, for any number of clients.
#[derive(Clone)]
pub struct Feeds {
    pub snapshot: Feed,
    pub map: Feed,
}

/// Write side of [`Feeds`], owned by the telemetry (or demo) loop.
pub struct Publisher {
    snapshot: watch::Sender<Arc<str>>,
    map: watch::Sender<Arc<str>>,
    map_version: u32,
}

impl Publisher {
    pub fn new() -> (Publisher, Feeds) {
        let (snapshot, snap_rx) = watch::channel::<Arc<str>>(Arc::from("{\"live\":false}"));
        let (map, map_rx) = watch::channel::<Arc<str>>(Arc::from("{\"version\":0,\"points\":[]}"));
        (Publisher { snapshot, map, map_version: 0 }, Feeds { snapshot: snap_rx, map: map_rx })
    }

    pub fn publish(&mut self, tracker: &Tracker, snap: &Snapshot) {
        if let Ok(json) = serde_json::to_string(snap) {
            self.snapshot.send_replace(Arc::from(json));
        }
        if snap.map_version != self.map_version {
            self.map_version = snap.map_version;
            self.map.send_replace(Arc::from(tracker.map_json()));
        }
    }
}
