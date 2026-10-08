//! Track map and live delta.
//!
//! GT7 reports no track, only the world position. The map is built from the driven line of every
//! complete lap; the fastest lap is the reference for the live delta.

use serde::Serialize;

/// Distance (m) between stored points of a lap
const SPACING: f32 = 2.0;
/// Farther away (m) the reference counts as lost (shortcut, pit, reset)
const MAX_OFF_LINE: f32 = 40.0;
const SEARCH_WINDOW: usize = 60;

#[derive(Clone, Copy, Serialize)]
pub struct Point {
    pub x: f32,
    pub z: f32,
    #[serde(skip)]
    pub t_ms: i64,
}

#[derive(Default)]
pub struct Track {
    current: Vec<Point>,
    /// true if the running lap is recorded from the start/finish line on
    current_complete: bool,
    best: Option<(i32, Vec<Point>)>,
    map: Vec<Point>,
    pub map_version: u32,
    ref_idx: usize,
}

#[derive(Serialize)]
pub struct MapData<'a> {
    pub version: u32,
    pub points: &'a [Point],
}

impl Track {
    pub fn reset(&mut self) {
        *self = Track { map_version: self.map_version + 1, ..Default::default() };
    }

    /// A new lap starts. `finished_ms` is the official time of the lap just finished.
    pub fn lap_started(&mut self, finished_ms: Option<i32>, from_line: bool) {
        let lap = std::mem::take(&mut self.current);
        if let (true, Some(ms)) = (self.current_complete, finished_ms) {
            if lap.len() > 20 {
                if self.best.as_ref().is_none_or(|(b, _)| ms <= *b) {
                    self.best = Some((ms, lap.clone()));
                }
                self.map = lap;
                self.map_version += 1;
            }
        }
        self.current_complete = from_line;
        self.ref_idx = 0;
    }

    pub fn add(&mut self, x: f32, z: f32, t_ms: i64) {
        let far = self.current.last().is_none_or(|p| dist(p, x, z) >= SPACING);
        if far {
            self.current.push(Point { x, z, t_ms });
            // Until there is a complete lap, the map grows live
            if self.map.is_empty() && self.current.len().is_multiple_of(25) {
                self.map_version += 1;
            }
        }
    }

    /// Is (x, z) at most `max` metres away from the known track?
    pub fn is_near(&self, x: f32, z: f32, max: f32) -> bool {
        self.map.iter().chain(&self.current).any(|p| dist(p, x, z) <= max)
    }

    pub fn map(&self) -> MapData<'_> {
        let points = if self.map.is_empty() { &self.current } else { &self.map };
        MapData { version: self.map_version, points }
    }

    /// Time difference (ms) to the best lap at the current position; negative = faster.
    pub fn delta(&mut self, x: f32, z: f32, t_ms: i64) -> Option<i64> {
        let (_, best) = self.best.as_ref()?;
        if best.len() < 2 {
            return None;
        }
        let lo = self.ref_idx.saturating_sub(5);
        let hi = (self.ref_idx + SEARCH_WINDOW).min(best.len() - 1);
        let nearest = |range: std::ops::RangeInclusive<usize>| range.map(|i| (i, dist(&best[i], x, z))).min_by(|a, b| a.1.total_cmp(&b.1));
        let (mut idx, mut d) = nearest(lo..=hi)?;
        if d > MAX_OFF_LINE / 2.0 {
            (idx, d) = nearest(0..=best.len() - 1)?;
        }
        if d > MAX_OFF_LINE {
            return None;
        }
        self.ref_idx = idx;

        // Project onto the segment to the neighbouring point
        let (a, b) = if idx + 1 < best.len() { (&best[idx], &best[idx + 1]) } else { (&best[idx - 1], &best[idx]) };
        let (sx, sz) = (b.x - a.x, b.z - a.z);
        let len2 = sx * sx + sz * sz;
        let f = if len2 > 0.0 { (((x - a.x) * sx + (z - a.z) * sz) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let ref_t = a.t_ms as f32 + f * (b.t_ms - a.t_ms) as f32;
        Some(t_ms - ref_t as i64)
    }
}

fn dist(p: &Point, x: f32, z: f32) -> f32 {
    ((p.x - x).powi(2) + (p.z - z).powi(2)).sqrt()
}
