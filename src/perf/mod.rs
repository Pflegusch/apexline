//! Performance measurements: acceleration from standstill (0–100 km/h, 0–200 km/h, 1/4 mile …),
//! braking to standstill (100–0 km/h …) and a dyno-style power/torque curve derived from the
//! acceleration. Runs are detected automatically from the packet stream, no button needed:
//!
//! - stop, then accelerate: an acceleration run starts with the first movement and ends when
//!   braking, when the speed drops (lifted, crashed) or after 3 minutes; it counts once it
//!   reached 100 km/h;
//! - brake from at least 100 km/h down to standstill: a braking run.
//!
//! Impacts (walls, other cars) end acceleration runs and void braking runs. Each mark carries the
//! slope it was measured on; marks on slopes are listed but don't count as best values.
//!
//! Finished runs are stored per car (see [`store`]) and served to the dashboard (page 4).

pub mod cli;
mod dyno;
pub mod store;

pub use dyno::Dyno;
pub use store::Store;

use crate::telemetry::packet::{flags, Packet};
use serde::{Deserialize, Serialize};

const TICKS_PER_SEC: f32 = 60.0;
/// Below this speed (km/h) the car counts as standing.
const STAND_KMH: f32 = 0.5;
/// Larger gaps in the packet stream (ticks) interrupt a run.
const MAX_GAP: i32 = 30;
/// Pedal thresholds (raw 0–255): pressed (~10 %) and released (~5 %).
const PEDAL_ON: u8 = 26;
const PEDAL_OFF: u8 = 13;
/// A launch needs at least this much throttle within the first second (rolling away after a spin
/// does not count).
const LAUNCH_THROTTLE: u8 = 128;
const LAUNCH_WINDOW_S: f32 = 1.0;
/// No launch: after this long (s) still below this speed (km/h) – creeping, not accelerating.
const CREEP_S: f32 = 3.0;
const CREEP_KMH: f32 = 20.0;
/// An acceleration run ends when the speed stays this far below its maximum for `DROP_S`,
/// or immediately when it drops by `DROP_HARD_KMH` (gear shifts cost a few km/h at most).
const DROP_KMH: f32 = 3.0;
const DROP_S: f32 = 1.5;
const DROP_HARD_KMH: f32 = 15.0;
const MAX_ACCEL_S: f32 = 180.0;
/// Acceleration runs are kept from this top speed on (pit exits, restarts after a spin don't count).
const ACCEL_MIN_KMH: f32 = 100.0;
/// Braking runs start at this speed or above (so pit stops don't count).
const BRAKE_MIN_KMH: f32 = 100.0;
const MAX_BRAKE_S: f32 = 60.0;
/// Brake released for this long: run ends (below `RELEASE_FINISH_KMH`) or is dropped.
const RELEASE_S: f32 = 0.3;
const RELEASE_FINISH_KMH: f32 = 10.0;
const MPH: f32 = 1.609_344;
/// Speed marks: metric every 50 km/h ("0-100", "100-0"), imperial in mph ("0-60mph", "60-0mph").
/// Both are measured; the dashboard shows the ones of its unit system.
const METRIC_STEP: u32 = 50;
const METRIC_MAX: u32 = 600;
const MPH_ACCEL: [u32; 9] = [30, 60, 100, 130, 150, 200, 250, 300, 350];
const MPH_BRAKE: [u32; 8] = [60, 70, 100, 130, 150, 200, 250, 300];
/// Rolling intervals inside an acceleration run: (from, to, unit).
const INTERVALS: [(u32, u32, &str); 5] = [(100, 200, ""), (200, 300, ""), (300, 400, ""), (60, 130, "mph"), (100, 150, "mph")];
/// Distance marks (m) with their keys.
const DISTANCES: [(f32, &str); 4] = [(201.168, "1/8mi"), (402.336, "1/4mi"), (804.672, "1/2mi"), (1000.0, "1km")];
/// Trace points stored per run: every n-th sample (60 Hz → 20 Hz).
const TRACE_EVERY: usize = 3;
/// A speed change above this (m/s² between two samples, ~8 g) is an impact, not driving.
const IMPACT: f32 = 80.0;
/// Marks shorter than this (m) get no slope (suspension movement dominates).
const GRADE_MIN_M: f32 = 20.0;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Accel,
    Brake,
}

/// What the detector is doing right now (for the live view).
#[derive(Serialize, Clone, Copy, Default, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Idle,
    /// Standing on track: the next launch is measured
    Ready,
    Accel,
    Brake,
}

/// One measured value, e.g. key "0-100": time (s), distance (m) and speed (km/h) at the end.
/// For braking marks ("100-0") the time and distance from crossing the speed to standstill.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Mark {
    pub key: String,
    pub t: f32,
    pub d: f32,
    pub v: f32,
    /// Slope over the measured distance in percent (+ uphill)
    #[serde(default)]
    pub g: f32,
}

impl Mark {
    fn new(key: String, t: f32, d: f32, v: f32, rise: f32) -> Self {
        let g = if d >= GRADE_MIN_M { rise / d * 100.0 } else { 0.0 };
        Mark { key, t, d, v, g }
    }
}

/// A finished run as stored on disk and served by the API.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Run {
    /// File stem: start time + kind, e.g. "2026-10-08_20-15-03_accel"
    pub id: String,
    pub kind: Kind,
    pub car_code: i32,
    pub car: String,
    /// Start time (RFC 3339)
    pub time: String,
    pub v_start: f32,
    pub v_end: f32,
    pub v_max: f32,
    pub duration_s: f32,
    pub dist_m: f32,
    /// Mean slope over the run in percent (+ uphill)
    pub grade_pct: f32,
    pub marks: Vec<Mark>,
    /// 20 Hz: [t s, km/h, distance m, rpm, gear, throttle %]
    pub trace: Vec<[f32; 6]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dyno: Option<Dyno>,
    /// Session folder for runs imported from recordings
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Packet counter at the start (only needed while stamping)
    #[serde(skip)]
    pub start_tick: i32,
}

impl Run {
    /// Sets car and start time; the id is derived from both.
    pub fn stamp(&mut self, car_code: i32, car: &str, start: chrono::DateTime<chrono::Local>) {
        self.car_code = car_code;
        self.car = car.to_string();
        self.time = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
        let kind = match self.kind {
            Kind::Accel => "accel",
            Kind::Brake => "brake",
        };
        self.id = format!("{}_{kind}", start.format("%Y-%m-%d_%H-%M-%S"));
    }

    pub fn mark(&self, key: &str) -> Option<&Mark> {
        self.marks.iter().find(|m| m.key == key)
    }
}

/// Short version of the last finished run for the live view.
#[derive(Serialize, Clone, Debug)]
pub struct LastRun {
    pub kind: Kind,
    pub t: f32,
    pub d: f32,
    pub v_start: f32,
    pub v_max: f32,
    pub marks: Vec<Mark>,
}

/// Live state, part of the dashboard snapshot.
#[derive(Serialize, Clone, Default, Debug)]
pub struct Live {
    pub phase: Phase,
    /// Elapsed time (s) and distance (m) of the running attempt
    pub t: f32,
    pub d: f32,
    /// Marks reached so far in the running acceleration attempt
    pub marks: Vec<Mark>,
    pub last: Option<LastRun>,
    /// Number of runs saved since the server started; a change tells clients to reload
    pub saved: u32,
}

/// The channels a measurement needs, from a live packet or a recorded CSV row.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    /// Transmission ratios of gears 1–8 (live packets only; zero in recordings)
    pub gear_ratios: [f32; 8],
    pub tick: i32,
    pub speed_kmh: f32,
    pub rpm: f32,
    /// 0 = reverse, 15 = neutral
    pub gear: u8,
    pub throttle: u8,
    pub brake: u8,
    /// Height (m)
    pub y: f32,
    pub flags: u16,
}

impl Sample {
    pub fn from_packet(p: &Packet) -> Self {
        Sample {
            tick: p.packet_id,
            speed_kmh: p.speed_ms * 3.6,
            rpm: p.engine_rpm,
            gear: p.gear,
            throttle: p.throttle,
            brake: p.brake,
            y: p.position[1],
            flags: p.flags,
            gear_ratios: p.gear_ratios,
        }
    }

    fn usable(&self) -> bool {
        self.flags & flags::ON_TRACK != 0 && self.flags & (flags::PAUSED | flags::LOADING) == 0
    }

    fn reverse(&self) -> bool {
        self.gear == 0
    }
}

/// One sample inside a run, relative to its start.
#[derive(Clone, Copy, Debug, Default)]
struct Pt {
    t: f32,
    v: f32,
    s: f32,
    rpm: f32,
    gear: u8,
    thr: u8,
    brk: u8,
    y: f32,
}

struct Attempt {
    kind: Kind,
    start_tick: i32,
    pts: Vec<Pt>,
    v_max: f32,
    v_max_t: f32,
    v_min: f32,
    /// Braking: index of the sample where the brake was released
    released: Option<usize>,
    marks: Vec<Mark>,
    /// Speed marks reached so far: (key, time, distance, height), for the intervals
    reached: Vec<(String, f32, f32, f32)>,
    gear_ratios: [f32; 8],
}

impl Attempt {
    fn new(kind: Kind, s: &Sample) -> Self {
        let p = Pt { v: s.speed_kmh, rpm: s.rpm, gear: s.gear, thr: s.throttle, brk: s.brake, y: s.y, ..Default::default() };
        Attempt {
            kind,
            start_tick: s.tick,
            pts: vec![p],
            v_max: p.v,
            v_max_t: 0.0,
            v_min: p.v,
            released: None,
            marks: Vec::new(),
            reached: Vec::new(),
            gear_ratios: s.gear_ratios,
        }
    }

    fn last(&self) -> Pt {
        *self.pts.last().expect("attempt has a first point")
    }

    /// True if the speed change to `s` is too abrupt for driving (hit a wall or a car).
    fn impact(&self, s: &Sample, dt: f32) -> bool {
        (s.speed_kmh - self.last().v).abs() / 3.6 / dt > IMPACT
    }

    fn push(&mut self, s: &Sample, dt: f32) {
        let a = self.last();
        let v = s.speed_kmh;
        let b = Pt { t: a.t + dt, v, s: a.s + (a.v + v) / 2.0 / 3.6 * dt, rpm: s.rpm, gear: s.gear, thr: s.throttle, brk: s.brake, y: s.y };
        self.pts.push(b);
        if self.gear_ratios[0] <= 0.0 {
            self.gear_ratios = s.gear_ratios;
        }
        if self.kind == Kind::Accel {
            self.accel_marks(a, b);
        }
        if v > self.v_max {
            self.v_max = v;
            self.v_max_t = b.t;
        }
        self.v_min = self.v_min.min(v);
    }

    /// Marks reached between two consecutive samples of an acceleration run (interpolated).
    /// Speeds count the first time only (shifting can dip below a mark again).
    fn accel_marks(&mut self, a: Pt, b: Pt) {
        let y0 = self.pts[0].y;
        for &(kmh, num, unit) in accel_targets() {
            if self.v_max < kmh && b.v >= kmh {
                let f = (kmh - a.v) / (b.v - a.v);
                let (t, s, y) = (lerp(a.t, b.t, f), lerp(a.s, b.s, f), lerp(a.y, b.y, f));
                let key = format!("0-{num}{unit}");
                self.marks.push(Mark::new(key.clone(), t, s, kmh, y - y0));
                for (lo, hi, u) in INTERVALS {
                    if hi == num && u == unit {
                        let lo_key = format!("0-{lo}{u}");
                        if let Some(&(_, tl, sl, yl)) = self.reached.iter().find(|r| r.0 == lo_key) {
                            self.marks.push(Mark::new(format!("{lo}-{hi}{u}"), t - tl, s - sl, kmh, y - yl));
                        }
                    }
                }
                self.reached.push((key, t, s, y));
            }
        }
        for (dist, key) in DISTANCES {
            if a.s < dist && b.s >= dist {
                let f = (dist - a.s) / (b.s - a.s);
                let y = lerp(a.y, b.y, f);
                self.marks.push(Mark::new(key.to_string(), lerp(a.t, b.t, f), dist, lerp(a.v, b.v, f), y - y0));
            }
        }
    }

    fn live(&self) -> (f32, f32) {
        let p = self.last();
        (p.t, p.s)
    }
}

/// (km/h, number, unit suffix) of every speed mark.
type Target = (f32, u32, &'static str);

fn targets(mph: &'static [u32]) -> Vec<Target> {
    let metric = (1..=METRIC_MAX / METRIC_STEP).map(|i| ((i * METRIC_STEP) as f32, i * METRIC_STEP, ""));
    metric.chain(mph.iter().map(|&m| (m as f32 * MPH, m, "mph"))).collect()
}

fn accel_targets() -> &'static [Target] {
    static T: std::sync::OnceLock<Vec<Target>> = std::sync::OnceLock::new();
    T.get_or_init(|| targets(&MPH_ACCEL))
}

fn brake_targets() -> &'static [Target] {
    static T: std::sync::OnceLock<Vec<Target>> = std::sync::OnceLock::new();
    T.get_or_init(|| targets(&MPH_BRAKE))
}

fn lerp(a: f32, b: f32, f: f32) -> f32 {
    a + (b - a) * f
}

fn round(v: f32, digits: i32) -> f32 {
    let m = 10f32.powi(digits);
    (v * m).round() / m
}

/// Turns the packet stream into measurement runs.
#[derive(Default)]
pub struct Detector {
    attempt: Option<Attempt>,
    standing: bool,
    last: Option<Sample>,
    pub live: Live,
}

impl Detector {
    /// Feeds one sample; returns a run when one was completed with it.
    pub fn update(&mut self, s: Sample) -> Option<Run> {
        let prev = self.last.replace(s);
        let gap = prev.map(|p| s.tick - p.tick);
        if gap == Some(0) {
            self.last = prev; // duplicate packet
            return None;
        }
        let mut done = None;
        if !s.usable() || !gap.is_some_and(|g| (1..=MAX_GAP).contains(&g)) {
            // Pause, menu, off track or lost packets: an acceleration run keeps what it measured so
            // far, a braking run without its end is no braking distance
            if self.attempt.as_ref().is_some_and(|a| a.kind == Kind::Brake) {
                self.attempt = None;
            }
            done = self.finish();
            self.standing = s.usable() && s.speed_kmh < STAND_KMH;
            self.update_live();
            return done;
        }
        let dt = gap.unwrap_or(1) as f32 / TICKS_PER_SEC;
        let prev = prev.expect("checked above");

        match self.attempt.as_ref().map(|a| a.kind) {
            Some(Kind::Accel) => {
                let a = self.attempt.as_mut().expect("checked above");
                if a.impact(&s, dt) {
                    done = self.finish();
                    self.standing = false;
                    self.update_live();
                    return done;
                }
                a.push(&s, dt);
                let (t, v) = (a.last().t, s.speed_kmh);
                if t > CREEP_S && a.v_max < CREEP_KMH {
                    self.attempt = None;
                    self.standing = false;
                    self.update_live();
                    return None;
                }
                let braking = s.brake >= PEDAL_ON;
                let dropped = a.v_max - v > DROP_HARD_KMH || (a.v_max - v > DROP_KMH && t - a.v_max_t > DROP_S);
                if braking || s.reverse() || dropped || t > MAX_ACCEL_S {
                    done = self.finish();
                    // 0–200–0: the braking run starts right where the acceleration ended
                    if braking && v >= BRAKE_MIN_KMH && s.throttle < PEDAL_ON {
                        self.attempt = Some(Attempt::new(Kind::Brake, &s));
                    }
                }
            }
            Some(Kind::Brake) => {
                let a = self.attempt.as_mut().expect("checked above");
                let invalid = s.throttle >= PEDAL_ON || s.reverse() || a.impact(&s, dt);
                if invalid || s.speed_kmh > a.v_min + DROP_KMH || a.last().t > MAX_BRAKE_S {
                    self.attempt = None;
                } else {
                    a.push(&s, dt);
                    if s.brake < PEDAL_OFF {
                        a.released.get_or_insert(a.pts.len() - 1);
                    } else {
                        a.released = None;
                    }
                    if s.speed_kmh < STAND_KMH {
                        a.released = None;
                        done = self.finish();
                    } else if let Some(r) = a.released {
                        if a.last().t - a.pts[r].t > RELEASE_S {
                            if a.pts[r].v < RELEASE_FINISH_KMH {
                                done = self.finish();
                            } else {
                                self.attempt = None;
                            }
                        }
                    }
                }
            }
            None => {
                if self.standing && s.speed_kmh >= STAND_KMH && !s.reverse() {
                    // Launch: the run starts at the last standing sample
                    let mut a = Attempt::new(Kind::Accel, &prev);
                    a.push(&s, dt);
                    self.attempt = Some(a);
                } else if s.brake >= PEDAL_ON && s.throttle < PEDAL_ON && s.speed_kmh >= BRAKE_MIN_KMH && !s.reverse() {
                    self.attempt = Some(Attempt::new(Kind::Brake, &s));
                }
            }
        }
        self.standing = s.speed_kmh < STAND_KMH;
        self.update_live();
        done
    }

    /// End of input (offline analysis): completes a running acceleration run.
    pub fn flush(&mut self) -> Option<Run> {
        let done = self.finish();
        self.update_live();
        done
    }

    fn update_live(&mut self) {
        let l = &mut self.live;
        match &self.attempt {
            Some(a) => {
                l.phase = if a.kind == Kind::Accel { Phase::Accel } else { Phase::Brake };
                (l.t, l.d) = a.live();
                if a.kind == Kind::Accel && l.marks.len() != a.marks.len() {
                    l.marks = a.marks.clone();
                }
            }
            None => {
                l.phase = if self.standing { Phase::Ready } else { Phase::Idle };
                l.t = 0.0;
                l.d = 0.0;
                l.marks.clear();
            }
        }
    }

    /// Ends the current attempt; returns it as a run if it is a valid measurement.
    fn finish(&mut self) -> Option<Run> {
        let a = self.attempt.take()?;
        let run = match a.kind {
            Kind::Accel => finish_accel(a),
            Kind::Brake => finish_brake(a),
        }?;
        self.live.last = Some(LastRun {
            kind: run.kind,
            t: run.duration_s,
            d: run.dist_m,
            v_start: run.v_start,
            v_max: run.v_max,
            marks: run.marks.clone(),
        });
        Some(run)
    }
}

fn finish_accel(a: Attempt) -> Option<Run> {
    let launched = a.pts.iter().take_while(|p| p.t <= LAUNCH_WINDOW_S).any(|p| p.thr >= LAUNCH_THROTTLE);
    if a.v_max < ACCEL_MIN_KMH || !launched {
        return None;
    }
    let dyno = dyno::compute(&a.pts, a.gear_ratios);
    Some(build(a, dyno))
}

fn finish_brake(mut a: Attempt) -> Option<Run> {
    // Brake released shortly before standstill: cut there and extrapolate with the deceleration
    // just before (the remaining distance is a few centimetres). Nothing else is extrapolated.
    if let Some(r) = a.released {
        a.pts.truncate(r + 1);
    }
    let end = a.last();
    if end.v >= RELEASE_FINISH_KMH {
        return None;
    }
    if end.v >= STAND_KMH {
        let before = a.pts.iter().rev().find(|p| end.t - p.t >= 0.25)?;
        let decel = (before.v - end.v) / 3.6 / (end.t - before.t);
        if decel < 1.0 {
            return None;
        }
        let v = end.v / 3.6;
        a.pts.push(Pt { t: end.t + v / decel, v: 0.0, s: end.s + v * v / (2.0 * decel), brk: end.brk, ..end });
    }
    let end = a.last();
    for &(kmh, num, unit) in brake_targets() {
        let Some(i) = a.pts.windows(2).position(|w| w[0].v >= kmh && w[1].v < kmh) else { continue };
        let (p, q) = (a.pts[i], a.pts[i + 1]);
        let f = (p.v - kmh) / (p.v - q.v);
        let (t, s, y) = (lerp(p.t, q.t, f), lerp(p.s, q.s, f), lerp(p.y, q.y, f));
        a.marks.push(Mark::new(format!("{num}-0{unit}"), end.t - t, end.s - s, kmh, end.y - y));
    }
    Some(build(a, None))
}

fn build(a: Attempt, dyno: Option<Dyno>) -> Run {
    let (first, end) = (a.pts[0], a.last());
    let mut trace: Vec<[f32; 6]> = a
        .pts
        .iter()
        .step_by(TRACE_EVERY)
        .chain(a.pts.last())
        .map(|p| [round(p.t, 2), round(p.v, 1), round(p.s, 1), p.rpm.round(), p.gear as f32, (p.thr as f32 / 2.55).round()])
        .collect();
    if a.pts.len() % TRACE_EVERY == 1 {
        trace.pop(); // the last point was already taken by step_by
    }
    let marks = a.marks.into_iter().map(|m| Mark { t: round(m.t, 3), d: round(m.d, 1), v: round(m.v, 1), g: round(m.g, 1), ..m }).collect();
    Run {
        id: String::new(),
        kind: a.kind,
        car_code: 0,
        car: String::new(),
        time: String::new(),
        v_start: round(first.v, 1),
        v_end: round(end.v, 1),
        v_max: round(a.v_max, 1),
        duration_s: round(end.t, 3),
        dist_m: round(end.s, 1),
        grade_pct: if end.s > 50.0 { round((end.y - first.y) / end.s * 100.0, 1) } else { 0.0 },
        marks,
        trace,
        dyno,
        source: None,
        start_tick: a.start_tick,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: u16 = flags::ON_TRACK;

    /// Feeds `secs` of samples at 60 Hz; `f(t)` gives (speed km/h, throttle, brake, gear, rpm).
    fn drive(d: &mut Detector, tick: &mut i32, secs: f32, f: impl Fn(f32) -> (f32, u8, u8, u8, f32)) -> Vec<Run> {
        let mut runs = Vec::new();
        for i in 0..(secs * TICKS_PER_SEC) as i32 {
            *tick += 1;
            let (speed_kmh, throttle, brake, gear, rpm) = f(i as f32 / TICKS_PER_SEC);
            let s = Sample { tick: *tick, speed_kmh, throttle, brake, gear, rpm, flags: ON, ..Default::default() };
            runs.extend(d.update(s));
        }
        runs
    }

    fn mark<'a>(r: &'a Run, key: &str) -> &'a Mark {
        r.mark(key).unwrap_or_else(|| panic!("mark {key} missing: {:?}", r.marks))
    }

    #[test]
    fn measures_acceleration_and_braking() {
        let (mut d, mut tick) = (Detector::default(), 0);
        assert!(drive(&mut d, &mut tick, 1.0, |_| (0.0, 0, 0, 1, 900.0)).is_empty());
        assert_eq!(d.live.phase, Phase::Ready);

        // Constant 5 m/s² up to 250 km/h, then hold the speed
        let a = 5.0 * 3.6;
        let t_top = 250.0 / a;
        let runs = drive(&mut d, &mut tick, 20.0, |t| ((t * a).min(250.0), 255, 0, 3, 3000.0 + t * 100.0));
        assert!(runs.is_empty());
        assert_eq!(d.live.phase, Phase::Accel);

        // Full braking at 10 m/s² to standstill: ends the acceleration run, starts a braking run
        let decel = 10.0 * 3.6;
        let runs = drive(&mut d, &mut tick, 10.0, |t| ((250.0 - t * decel).max(0.0), 0, 255, 3, 2000.0));
        assert_eq!(runs.len(), 2, "acceleration + braking");
        let (acc, brk) = (&runs[0], &runs[1]);

        assert_eq!(acc.kind, Kind::Accel);
        // Analytic values (the run starts one tick early: last standing sample)
        let t100 = 100.0 / a;
        assert!((mark(acc, "0-100").t - t100).abs() < 0.03, "{:?}", acc.marks);
        assert!((mark(acc, "0-200").t - 200.0 / a).abs() < 0.03);
        assert!((mark(acc, "100-200").t - 100.0 / a).abs() < 0.01);
        let quarter = (2.0 * 402.336f32 / 5.0).sqrt();
        assert!((mark(acc, "1/4mi").t - quarter).abs() < 0.03);
        assert!((mark(acc, "1/4mi").v - quarter * a).abs() < 0.5);
        assert!(acc.mark("0-300").is_none());
        assert!((acc.v_max - 250.0).abs() < 0.01);
        assert!(acc.duration_s > t_top);

        assert_eq!(brk.kind, Kind::Brake);
        let d100 = (100.0f32 / 3.6).powi(2) / 20.0;
        assert!((mark(brk, "100-0").d - d100).abs() < 0.2, "{:?}", brk.marks);
        assert!((mark(brk, "100-0").t - 100.0 / decel).abs() < 0.03);
        assert!((mark(brk, "250-0").d - (250.0f32 / 3.6).powi(2) / 20.0).abs() < 0.5);
        // Imperial marks from the same runs
        assert!((mark(acc, "0-60mph").t - 60.0 * MPH / a).abs() < 0.03);
        assert!((mark(brk, "60-0mph").d - (60.0 * MPH / 3.6).powi(2) / 20.0).abs() < 0.2);
        assert!(acc.mark("60-130mph").is_some() && acc.mark("1/2mi").is_some());
        assert_eq!(d.live.phase, Phase::Ready);
    }

    #[test]
    fn ignores_ordinary_driving() {
        let (mut d, mut tick) = (Detector::default(), 0);
        // Creeping at walking pace (pit lane, grid) is no measurement
        drive(&mut d, &mut tick, 1.0, |_| (0.0, 0, 0, 1, 900.0));
        drive(&mut d, &mut tick, 5.0, |_| (0.6, 200, 0, 1, 1200.0));
        assert_eq!(d.live.phase, Phase::Idle);

        // Rolling away after a spin without throttle: no launch
        drive(&mut d, &mut tick, 1.0, |_| (0.0, 0, 0, 2, 900.0));
        let runs = drive(&mut d, &mut tick, 10.0, |t| (t * 8.0, 0, 0, 2, 2000.0));
        assert!(runs.is_empty());
        // Braking zone that ends at corner speed, then back on the throttle
        let runs = drive(&mut d, &mut tick, 3.0, |t| ((200.0 - t * 60.0).max(90.0), 0, 200, 4, 6000.0));
        assert!(runs.is_empty());
        let runs = drive(&mut d, &mut tick, 1.0, |t| (90.0 + t * 10.0, 255, 0, 4, 6000.0));
        assert!(runs.is_empty());
        assert_eq!(d.live.phase, Phase::Idle);
    }

    #[test]
    fn counts_marks_once_and_drops_impacts() {
        let (mut d, mut tick) = (Detector::default(), 0);
        drive(&mut d, &mut tick, 0.5, |_| (0.0, 0, 0, 1, 900.0));
        // Shift at 102 km/h dips below 100 again
        let speed = |t: f32| {
            if t < 6.0 {
                t * 17.0
            } else if t < 6.3 {
                102.0 - (t - 6.0) * 15.0
            } else {
                97.5 + (t - 6.3) * 17.0
            }
        };
        drive(&mut d, &mut tick, 10.0, |t| (speed(t), 255, 0, 2, 5000.0));
        // Hitting a wall at 150 km/h: the run keeps what it measured before
        let runs = drive(&mut d, &mut tick, 1.0, |t| (if t < 0.1 { 150.0 } else { 30.0 }, 255, 0, 2, 5000.0));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].marks.iter().filter(|m| m.key == "0-100").count(), 1, "{:?}", runs[0].marks);
        assert!((mark(&runs[0], "0-100").t - 100.0 / 17.0).abs() < 0.03);

        // A braking run interrupted (pause, off track) before standstill is dropped, not extrapolated
        let runs = drive(&mut d, &mut tick, 1.0, |t| (150.0 - t * 30.0, 0, 255, 3, 3000.0));
        assert!(runs.is_empty());
        tick += 120; // gap in the packet stream
        let runs = drive(&mut d, &mut tick, 0.5, |_| (0.0, 0, 255, 3, 900.0));
        assert!(runs.is_empty(), "{:?}", runs.iter().map(|r| (&r.kind, r.v_start)).collect::<Vec<_>>());

        // Braking into a wall is no braking distance
        let runs = drive(&mut d, &mut tick, 3.0, |t| ((150.0 - t * 30.0 - if t > 1.0 { 100.0 } else { 0.0 }).max(0.0), 0, 255, 3, 3000.0));
        assert!(runs.is_empty(), "{:?}", runs.iter().map(|r| &r.marks).collect::<Vec<_>>());
    }
}
