//! Driving sessions: the tracker turns the packet stream into the dashboard state and decides
//! when a session starts and ends ([`tracker`]), the recorder writes the laps to disk
//! ([`recorder`]), the track map is built from the driven line ([`track_map`]).

pub mod recorder;
pub mod track_map;
pub mod tracker;
