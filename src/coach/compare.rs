//! Lap comparison: every complete lap of a session is mapped onto the fastest lap and compared
//! corner by corner (time lost, braking point, apex speed, full-throttle point, balance).

use super::lap::{self, Row, G, HZ};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Per-corner figures of one lap.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CornerStats {
    pub brake_at: Option<f64>,
    pub brake_max: f64,
    pub v_entry: f64,
    pub v_min: f64,
    pub v_exit: f64,
    pub full_at: Option<f64>,
    pub gear_min: Option<i64>,
    pub under: f64,
    pub over: f64,
    pub rear_slide: f64,
    pub glat: f64,
    pub tcs: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LapCorner {
    pub n: usize,
    /// Time lost against the reference in this corner section (s)
    pub dt: f64,
    #[serde(flatten)]
    pub stats: Option<CornerStats>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LapResult {
    pub file: String,
    pub lap: i64,
    pub ms: i64,
    pub delta_ms: i64,
    pub corners: Vec<LapCorner>,
}

impl LapResult {
    pub fn corner(&self, n: usize) -> &LapCorner {
        self.corners.iter().find(|c| c.n == n).expect("corner exists in every lap")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CornerInfo {
    pub n: usize,
    pub dir: String,
    pub x: f64,
    pub z: f64,
    pub s: i64,
    #[serde(rename = "ref")]
    pub reference: Option<CornerStats>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reference {
    pub file: String,
    pub ms: i64,
    pub lap: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Comparison {
    pub reference: Option<Reference>,
    pub steer_ratio: f64,
    pub corners: Vec<CornerInfo>,
    pub laps: Vec<LapResult>,
    /// Sum of the best corner sections over all laps
    pub theoretical_ms: Option<f64>,
}

impl Comparison {
    pub fn reference_lap(&self) -> Option<&LapResult> {
        let file = &self.reference.as_ref()?.file;
        self.laps.iter().find(|l| &l.file == file)
    }
}

/// Lightweight facts about a lap file, read without keeping the data in memory.
#[derive(Clone, Debug)]
struct Meta {
    n: usize,
    lap: Option<i64>,
    first_ms: Option<f64>,
    last_ms: Option<f64>,
    /// Official time of the previous lap (sent in the packets of this lap)
    prev_official: Option<f64>,
}

/// Corner definition stored per session in `corners.json` so numbering stays stable.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct CornerDef {
    n: usize,
    dir: String,
    from: [f64; 2],
    apex: [f64; 2],
    to: [f64; 2],
}

#[derive(Serialize, Deserialize)]
struct CornerFile {
    source: i64,
    corners: Vec<CornerDef>,
}

/// A corner mapped onto a reference lap (indices into the reference rows).
#[derive(Clone, Debug)]
struct Corner {
    n: usize,
    dir: String,
    from: usize,
    to: usize,
    apex: usize,
    x: f64,
    z: f64,
    s: f64,
}

/// Cache key of a lap file: path, modification time, size.
type FileKey = (PathBuf, SystemTime, u64);
/// Cache key of a comparison: lap file, reference file, corner windows.
type ResultKey = (PathBuf, PathBuf, Vec<(usize, usize, usize)>);

/// Keeps caches between runs so only new laps are loaded and projected.
#[derive(Default)]
pub struct Comparer {
    meta: HashMap<FileKey, Meta>,
    results: HashMap<ResultKey, LapResult>,
}

pub fn lap_files(session: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(session)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "csv"))
        .collect();
    files.sort();
    files
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

impl Comparer {
    fn meta(&mut self, path: &Path) -> Meta {
        let md = fs::metadata(path).ok();
        let key = (
            path.to_path_buf(),
            md.as_ref().and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH),
            md.map(|m| m.len()).unwrap_or(0),
        );
        if let Some(m) = self.meta.get(&key) {
            return m.clone();
        }
        let text = fs::read_to_string(path).unwrap_or_default();
        let mut lines = text.lines();
        let header: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
        let col = |n: &str| header.iter().position(|c| *c == n);
        let (c_lap, c_ms, c_last) = (col("lap"), col("lap_ms"), col("last_lap_ms"));
        let num = |line: &str, c: Option<usize>| -> Option<f64> {
            let cell = line.split(',').nth(c?)?;
            if cell.is_empty() {
                None
            } else {
                cell.parse().ok()
            }
        };
        let rows: Vec<&str> = lines.filter(|l| !l.is_empty()).collect();
        let first = rows.first().copied();
        let last = rows.last().copied();
        let early = rows.get(rows.len().min(6).saturating_sub(1)).copied().filter(|_| !rows.is_empty());
        let m = Meta {
            n: rows.len(),
            lap: first.and_then(|l| num(l, c_lap)).map(|v| v as i64),
            first_ms: first.and_then(|l| num(l, c_ms)),
            last_ms: last.and_then(|l| num(l, c_ms)),
            prev_official: early.and_then(|l| num(l, c_last)),
        };
        self.meta.retain(|k, _| k.0 != path);
        self.meta.insert(key, m.clone());
        m
    }

    /// All complete laps `(file, official ms)`: started at the line and the recorded duration
    /// matches the official time (sent as `last_lap_ms` in the following lap).
    pub fn complete_laps(&mut self, session: &Path) -> Vec<(PathBuf, i64)> {
        let files = lap_files(session);
        let mut out = Vec::new();
        for pair in files.windows(2) {
            let (a, b) = (self.meta(&pair[0]), self.meta(&pair[1]));
            if a.n < 600 || b.n == 0 || b.lap != Some(a.lap.unwrap_or(0) + 1) {
                continue;
            }
            let recorded = a.last_ms.unwrap_or(0.0);
            if let Some(official) = b.prev_official.filter(|o| *o > 0.0) {
                if (recorded + 1000.0 / HZ - official).abs() < 400.0 && a.first_ms.unwrap_or(0.0) < 100.0 {
                    out.push((pair[0].clone(), official as i64));
                }
            }
        }
        out
    }

    /// Compares all complete laps of a session against the fastest one.
    pub fn compare(&mut self, session: &Path) -> std::io::Result<Comparison> {
        let laps = self.complete_laps(session);
        let Some((ref_path, ref_ms)) = laps.iter().min_by_key(|(_, ms)| *ms).cloned() else {
            return Ok(Comparison::default());
        };
        let mut reference = lap::load(&ref_path)?;
        let (ratio, _) = lap::steer_ratio(&reference);
        prepare(&mut reference, ratio);
        project(&mut reference, None);

        let corners_path = session.join("corners.json");
        if !corners_path.exists() {
            // Corners are defined once per session from the first complete lap
            if laps[0].0 == ref_path {
                write_corner_defs(&corners_path, &reference)?;
            } else {
                let mut first = lap::load(&laps[0].0)?;
                prepare(&mut first, ratio);
                write_corner_defs(&corners_path, &first)?;
            }
        }
        let corners = map_corner_defs(&corners_path, &reference)?;
        let ckey: Vec<(usize, usize, usize)> = corners.iter().map(|c| (c.n, c.from, c.to)).collect();

        let mut result = Comparison {
            reference: Some(Reference { file: file_name(&ref_path), ms: ref_ms, lap: reference[0].lap as i64 }),
            steer_ratio: lap::round_to(ratio, 1),
            ..Default::default()
        };
        for c in &corners {
            result.corners.push(CornerInfo {
                n: c.n,
                dir: c.dir.clone(),
                x: lap::round_to(c.x, 1),
                z: lap::round_to(c.z, 1),
                s: lap::round_i(c.s),
                reference: corner_stats(&reference, s_of(&reference, c.from), s_of(&reference, c.to), c.s),
            });
        }
        for (path, ms) in &laps {
            let key = (path.clone(), ref_path.clone(), ckey.clone());
            if !self.results.contains_key(&key) {
                let lap_result = if *path == ref_path {
                    evaluate_lap(path, *ms, ref_ms, &reference, &reference, &corners)
                } else {
                    let mut rows = lap::load(path)?;
                    prepare(&mut rows, ratio);
                    project(&mut rows, Some(&reference));
                    evaluate_lap(path, *ms, ref_ms, &rows, &reference, &corners)
                };
                self.results.insert(key.clone(), lap_result);
            }
            result.laps.push(self.results[&key].clone());
        }
        // Drop cached results that belong to an older reference
        self.results.retain(|k, _| k.1 == ref_path);
        result.theoretical_ms = Some(
            ref_ms as f64
                + 1000.0
                    * result
                        .corners
                        .iter()
                        .map(|c| result.laps.iter().map(|l| l.corner(c.n).dt).fold(f64::INFINITY, f64::min))
                        .sum::<f64>(),
        );
        Ok(result)
    }
}

fn prepare(rows: &mut [Row], ratio: f64) {
    lap::add_dynamics(rows, ratio);
    lap::with_distance(rows);
}

fn s_of(rows: &[Row], i: usize) -> f64 {
    rows[i].s.unwrap_or(0.0)
}

fn evaluate_lap(path: &Path, ms: i64, ref_ms: i64, rows: &[Row], reference: &[Row], corners: &[Corner]) -> LapResult {
    let corners = corners
        .iter()
        .map(|c| {
            let (s0, s1) = (s_of(reference, c.from), s_of(reference, c.to));
            let dt = (time_at(rows, s1) - time_at(rows, s0)) - (time_at(reference, s1) - time_at(reference, s0));
            LapCorner { n: c.n, dt: lap::round_to(dt, 3), stats: corner_stats(rows, s0, s1, c.s) }
        })
        .collect();
    LapResult { file: file_name(path), lap: rows[0].lap as i64, ms, delta_ms: ms - ref_ms, corners }
}

fn dist2(r: &Row, x: f64, z: f64) -> f64 {
    (r.pos_x - x).powi(2) + (r.pos_z - z).powi(2)
}

/// Index of the minimum (first one on ties, like Python's `min`).
fn argmin(range: impl Iterator<Item = usize>, key: impl Fn(usize) -> f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for i in range {
        let k = key(i);
        if best.is_none_or(|(_, b)| k < b) {
            best = Some((i, k));
        }
    }
    best.map(|(i, _)| i)
}

/// Assigns every sample the distance along the reference lap (`None` = the lap is its own reference).
fn project(rows: &mut [Row], reference: Option<&[Row]>) {
    let ref_pts: Vec<(f64, f64, f64)> = match reference {
        Some(r) => r.iter().map(|p| (p.pos_x, p.pos_z, p.s.unwrap_or(0.0))).collect(),
        None => rows.iter().map(|p| (p.pos_x, p.pos_z, p.s.unwrap_or(0.0))).collect(),
    };
    let n = ref_pts.len();
    let mut idx = 0usize;
    for r in rows.iter_mut() {
        let (x, z) = (r.pos_x, r.pos_z);
        let d2 = |i: usize| (ref_pts[i].0 - x).powi(2) + (ref_pts[i].1 - z).powi(2);
        let (lo, hi) = (idx.saturating_sub(30), (n - 1).min(idx + 240));
        let mut best = argmin(lo..=hi, d2).unwrap_or(0);
        if d2(best).sqrt() > 25.0 {
            // Lost the reference: search globally
            best = argmin(0..n, d2).unwrap_or(0);
        }
        idx = best;
        r.s_ref = ref_pts[best].2;
    }
}

/// Lap time (s) at which `rows` reaches reference distance `s` (linear interpolation).
fn time_at(rows: &[Row], s: f64) -> f64 {
    let (mut lo, mut hi) = (0usize, rows.len() - 1);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if rows[mid].s_ref < s {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let i = lo.max(1);
    let (a, b) = (&rows[i - 1], &rows[i]);
    let ds = b.s_ref - a.s_ref;
    let f = if ds > 0.0 { (s - a.s_ref) / ds } else { 0.0 };
    ((i - 1) as f64 + f.clamp(0.0, 1.0)) / HZ
}

/// Corners of a lap including the braking zone before them; the lap is split into gapless sections.
fn corners_of(rows: &[Row]) -> Vec<Corner> {
    let raw = lap::segments(rows, |r| r.vf > 8.0 && r.sway.abs() > 7.0, 15, 20);
    let mut found: Vec<(usize, usize, usize, usize)> = Vec::new(); // start, turn, end, apex
    for (s, e) in raw {
        let mut b = s;
        while b > 0 && s_of(rows, s) - s_of(rows, b) < 250.0 && (rows[b - 1].brake > 10.0 || rows[b - 1].throttle < 200.0) {
            b -= 1;
        }
        let apex = argmin(s..=e, |i| rows[i].speed_kmh).unwrap_or(s);
        found.push((b, s, e, apex));
    }
    let mut bounds = vec![0];
    for w in found.windows(2) {
        bounds.push((w[0].2 + w[1].0) / 2);
    }
    bounds.push(rows.len() - 1);
    found
        .iter()
        .enumerate()
        .map(|(k, &(_, turn, end, apex))| {
            let a = &rows[apex];
            let yaw = lap::mean((turn..=end).map(|i| rows[i].yaw));
            Corner {
                n: k + 1,
                dir: if yaw > 0.0 { "links" } else { "rechts" }.into(),
                from: bounds[k],
                to: bounds[k + 1],
                apex,
                x: a.pos_x,
                z: a.pos_z,
                s: a.s.unwrap_or(0.0),
            }
        })
        .collect()
}

fn write_corner_defs(path: &Path, rows: &[Row]) -> std::io::Result<()> {
    let pos = |i: usize| [lap::round_to(rows[i].pos_x, 2), lap::round_to(rows[i].pos_z, 2)];
    let defs = corners_of(rows)
        .iter()
        .map(|c| CornerDef { n: c.n, dir: c.dir.clone(), from: pos(c.from), apex: pos(c.apex), to: pos(c.to) })
        .collect();
    let file = CornerFile { source: rows[0].lap as i64, corners: defs };
    fs::write(path, serde_json::to_string_pretty(&file).unwrap_or_default())
}

/// Index of the closest reference point at or after `start`. Only a third of the lap ahead is
/// searched so start/finish (beginning ≈ end) and nearby track parts are not confused.
fn nearest_from(rows: &[Row], x: f64, z: f64, start: usize) -> usize {
    let start = start.min(rows.len() - 1);
    let end = rows.len().min(start + (rows.len() / 3).max(60));
    argmin(start..end, |i| dist2(&rows[i], x, z)).unwrap_or(start)
}

/// Maps the session's corner definitions onto `reference`.
fn map_corner_defs(path: &Path, reference: &[Row]) -> std::io::Result<Vec<Corner>> {
    let file: CornerFile = serde_json::from_str(&fs::read_to_string(path)?)?;
    let mut out: Vec<Corner> = Vec::new();
    let mut prev = 0usize;
    for (k, d) in file.corners.iter().enumerate() {
        let i_from = if k == 0 { 0 } else { nearest_from(reference, d.from[0], d.from[1], prev.saturating_sub(50)) };
        let i_apex = nearest_from(reference, d.apex[0], d.apex[1], i_from);
        let i_to = nearest_from(reference, d.to[0], d.to[1], i_apex);
        prev = i_to;
        let a = &reference[i_apex];
        out.push(Corner {
            n: d.n,
            dir: d.dir.clone(),
            from: i_from,
            to: i_to,
            apex: i_apex,
            x: a.pos_x,
            z: a.pos_z,
            s: a.s.unwrap_or(0.0),
        });
    }
    if let Some(first) = out.first_mut() {
        first.from = 0; // gapless up to start/finish
    }
    if let Some(last) = out.last_mut() {
        last.to = reference.len() - 1;
    }
    Ok(out)
}

/// Figures of one lap in the section `[s_from, s_to]` (reference distances).
fn corner_stats(rows: &[Row], s_from: f64, s_to: f64, s_apex: f64) -> Option<CornerStats> {
    let seg: Vec<&Row> = rows.iter().filter(|r| s_from <= r.s_ref && r.s_ref <= s_to).collect();
    if seg.is_empty() {
        return None;
    }
    let brake_at = seg.iter().find(|r| r.brake > 25.0 && r.s_ref <= s_apex + 30.0).map(|r| r.s_ref);
    let mut near_apex: Vec<&Row> = seg.iter().copied().filter(|r| (r.s_ref - s_apex).abs() < 60.0).collect();
    if near_apex.is_empty() {
        near_apex = seg.clone();
    }
    let v_min = near_apex.iter().map(|r| r.speed_kmh).fold(f64::INFINITY, f64::min);
    let full_at = seg.iter().filter(|r| r.s_ref > s_apex - 20.0).find(|r| r.throttle > 245.0).map(|r| r.s_ref);
    let bals: Vec<f64> = seg.iter().filter(|r| r.sway.abs() > 6.0).filter_map(|r| r.bal).collect();
    let pct = |count: usize, total: usize| if total == 0 { 0.0 } else { count as f64 / total as f64 * 100.0 };
    Some(CornerStats {
        brake_at,
        brake_max: seg.iter().map(|r| r.brake).fold(f64::MIN, f64::max) / 2.55,
        v_entry: seg[0].speed_kmh,
        v_min,
        v_exit: seg[seg.len() - 1].speed_kmh,
        full_at,
        gear_min: near_apex.iter().filter(|r| r.gear > 0.0 && r.gear < 15.0).map(|r| r.gear as i64).min(),
        under: pct(bals.iter().filter(|b| **b > 2.0).count(), bals.len()),
        over: pct(bals.iter().filter(|b| **b < -1.5).count(), bals.len()),
        rear_slide: seg.iter().filter_map(|r| r.ar).map(f64::abs).fold(0.0, f64::max),
        glat: seg.iter().map(|r| r.sway.abs()).fold(f64::MIN, f64::max) / G,
        tcs: pct(seg.iter().filter(|r| r.has_flag(11)).count(), seg.len()),
    })
}
