//! Lap data loaded from the recorded CSV files plus the derived vehicle dynamics.
//!
//! All math is done in f64 and mirrors the original Python tools (tools/analyze.py) so that both
//! produce the same numbers.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub const HZ: f64 = 60.0;
pub const G: f64 = 9.81;
const WHEELBASE: f64 = 2.7;
const CG_TO_FRONT: f64 = WHEELBASE / 2.0;
const CG_TO_REAR: f64 = WHEELBASE / 2.0;
/// Rear slip angle (deg) above which the rear axle counts as sliding.
const REAR_SLIDE: f64 = 5.0;
pub const DEFAULT_STEER_RATIO: f64 = 20.0;

/// One telemetry sample (60 per second). Only the channels the coach needs.
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub lap: f64,
    pub lap_ms: Option<f64>,
    pub flags: u32,
    pub speed_kmh: f64,
    pub rpm: f64,
    pub gear: f64,
    /// Raw pedal values 0–255
    pub throttle: f64,
    pub brake: f64,
    /// Steering wheel angle (rad, + left)
    pub steering: f64,
    /// Lateral and longitudinal acceleration (m/s²)
    pub sway: f64,
    pub surge: f64,
    pub pos_x: f64,
    pub pos_z: f64,
    /// FL, FR, RL, RR
    pub tire_temp: [f64; 4],
    pub wheel_rps: [f64; 4],
    pub tire_radius: [f64; 4],
    pub fuel: f64,
    pub total_laps: Option<f64>,

    // Derived
    /// Velocity in the car frame (m/s): forward, lateral (+ left); yaw rate (rad/s, + left)
    pub vf: f64,
    pub vl: f64,
    pub yaw: f64,
    /// Distance driven since the start of the file (m), set by `with_distance`
    pub s: Option<f64>,
    /// Distance along the reference lap (m), set by `compare::project`
    pub s_ref: f64,
    /// Front/rear slip angle, balance (+ understeer / − oversteer), body slip angle (deg)
    pub af: Option<f64>,
    pub ar: Option<f64>,
    pub bal: Option<f64>,
    pub beta: Option<f64>,
}

impl Row {
    pub fn throttle_pct(&self) -> f64 {
        self.throttle / 2.55
    }
    pub fn brake_pct(&self) -> f64 {
        self.brake / 2.55
    }
    pub fn has_flag(&self, bit: u32) -> bool {
        self.flags & (1 << bit) != 0
    }
}

/// Inverse rotation of `v` by the quaternion `q` (x, y, z, w): world frame → car frame.
fn rotate_inv(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let (x, y, z, w) = (-q[0], -q[1], -q[2], q[3]);
    let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
    [v[0] + w * t[0] + (y * t[2] - z * t[1]), v[1] + w * t[1] + (z * t[0] - x * t[2]), v[2] + w * t[2] + (x * t[1] - y * t[0])]
}

/// Column lookup for one CSV file (older recordings lack some columns).
struct Columns(HashMap<String, usize>);

impl Columns {
    fn idx(&self, name: &str) -> Option<usize> {
        self.0.get(name).copied()
    }
}

fn field(cells: &[&str], idx: Option<usize>) -> Option<f64> {
    let cell = cells.get(idx?)?;
    if cell.is_empty() {
        None
    } else {
        cell.parse().ok()
    }
}

/// Loads a recorded lap file.
pub fn load(path: &Path) -> std::io::Result<Vec<Row>> {
    let text = fs::read_to_string(path)?;
    let mut lines = text.lines();
    let header = lines.next().unwrap_or_default();
    let cols = Columns(header.split(',').enumerate().map(|(i, c)| (c.to_string(), i)).collect());
    let c = |n: &str| cols.idx(n);
    let (lap, lap_ms, flags, speed, rpm, gear) = (c("lap"), c("lap_ms"), c("flags"), c("speed_kmh"), c("rpm"), c("gear"));
    let (thr, brk, steer, sway, surge) = (c("throttle"), c("brake"), c("steering"), c("sway"), c("surge"));
    let (px, pz) = (c("pos_x"), c("pos_z"));
    let vel = [c("vel_x"), c("vel_y"), c("vel_z")];
    let rot = [c("rot_x"), c("rot_y"), c("rot_z"), c("rot_w")];
    let angvel_y = c("angvel_y");
    let wheels = ["fl", "fr", "rl", "rr"];
    let temp = wheels.map(|w| c(&format!("tire_temp_{w}")));
    let rps = wheels.map(|w| c(&format!("wheel_rps_{w}")));
    let radius = wheels.map(|w| c(&format!("tire_radius_{w}")));
    let (fuel, total_laps) = (c("fuel"), c("total_laps"));

    let mut rows = Vec::with_capacity(text.len() / 700);
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let cells: Vec<&str> = line.split(',').collect();
        let f = |i: Option<usize>| field(&cells, i).unwrap_or(0.0);
        let q = rot.map(f);
        let v = vel.map(f);
        let loc = rotate_inv(q, v);
        rows.push(Row {
            lap: f(lap),
            lap_ms: field(&cells, lap_ms),
            flags: f(flags) as u32,
            speed_kmh: f(speed),
            rpm: f(rpm),
            gear: f(gear),
            throttle: f(thr),
            brake: f(brk),
            steering: f(steer),
            sway: f(sway),
            surge: f(surge),
            pos_x: f(px),
            pos_z: f(pz),
            tire_temp: temp.map(f),
            wheel_rps: rps.map(f),
            tire_radius: radius.map(f),
            fuel: f(fuel),
            total_laps: field(&cells, total_laps),
            vf: -loc[2],
            vl: -loc[0],
            yaw: f(angvel_y),
            ..Default::default()
        });
    }
    Ok(rows)
}

/// Median as in Python's `statistics.median` (mean of the two middle values for even counts).
pub fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

pub fn mean(values: impl IntoIterator<Item = f64>) -> f64 {
    let (sum, n) = values.into_iter().fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

/// Population standard deviation (Python `statistics.pstdev`).
pub fn pstdev(values: &[f64]) -> f64 {
    let m = mean(values.iter().copied());
    (values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / values.len().max(1) as f64).sqrt()
}

/// Python's `round()` (ties to even) to an integer.
pub fn round_i(v: f64) -> i64 {
    v.round_ties_even() as i64
}

/// Python's `round(v, digits)`.
pub fn round_to(v: f64, digits: i32) -> f64 {
    let f = 10f64.powi(digits);
    (v * f).round_ties_even() / f
}

/// Learns the steering ratio (steering wheel / road wheel angle) from gentle driving, where the tyres
/// roll with almost no slip: road wheel angle ≈ wheelbase · yaw rate / speed.
pub fn steer_ratio(rows: &[Row]) -> (f64, usize) {
    let mut ks: Vec<f64> = rows
        .iter()
        .filter(|r| {
            let (v, yaw, s) = (r.vf, r.yaw, r.steering);
            v > 8.0
                && v < 30.0
                && r.sway.abs() < 3.0
                && s.abs() > 0.05
                && s.abs() < 2.5
                && yaw.abs() > 0.03
                && yaw * s > 0.0
                && (r.vl / v).atan().to_degrees().abs() < 2.0
        })
        .map(|r| r.steering / (WHEELBASE * r.yaw / r.vf))
        .filter(|k| *k > 5.0 && *k < 60.0)
        .collect();
    let n = ks.len();
    if n >= 30 {
        (median(&mut ks), n)
    } else {
        (DEFAULT_STEER_RATIO, n)
    }
}

/// Slip angles and balance from the single-track model (see src/dynamics.rs).
pub fn add_dynamics(rows: &mut [Row], ratio: f64) {
    for r in rows {
        let v = r.vf;
        if v > 8.0 {
            let front = (r.steering / ratio - ((r.vl + CG_TO_FRONT * r.yaw) / v).atan()).to_degrees();
            let rear = (-((r.vl - CG_TO_REAR * r.yaw) / v).atan()).to_degrees();
            let mut bal = front.abs() - rear.abs();
            if rear.abs() > REAR_SLIDE {
                bal = bal.min(-(rear.abs() - REAR_SLIDE + 1.5));
            }
            r.af = Some(front);
            r.ar = Some(rear);
            r.bal = Some(bal);
            r.beta = Some((r.vl / v).atan().to_degrees());
        } else {
            r.af = None;
            r.ar = None;
            r.bal = None;
            r.beta = None;
        }
    }
}

/// Cumulative driven distance along the file.
pub fn with_distance(rows: &mut [Row]) {
    let mut d = 0.0;
    for i in 0..rows.len() {
        if i > 0 {
            d += (rows[i].pos_x - rows[i - 1].pos_x).hypot(rows[i].pos_z - rows[i - 1].pos_z);
        }
        rows[i].s = Some(d);
    }
}

/// Contiguous ranges (inclusive) where `pred` holds; gaps up to `gap` samples are bridged.
pub fn segments(rows: &[Row], pred: impl Fn(&Row) -> bool, min_len: usize, gap: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let (mut start, mut last): (Option<usize>, usize) = (None, 0);
    for (i, r) in rows.iter().enumerate() {
        if pred(r) {
            match start {
                None => start = Some(i),
                Some(s) if i - last > gap => {
                    if last - s + 1 >= min_len {
                        out.push((s, last));
                    }
                    start = Some(i);
                }
                _ => {}
            }
            last = i;
        }
    }
    if let Some(s) = start {
        if last - s + 1 >= min_len {
            out.push((s, last));
        }
    }
    out
}

/// Steering direction changes larger than `threshold` rad (hysteresis against noise).
pub fn reversals(vals: &[f64], threshold: f64) -> usize {
    let Some(&first) = vals.first() else { return 0 };
    let (mut count, mut direction, mut extreme) = (0, 0i8, first);
    for &v in &vals[1..] {
        if direction == 0 {
            if (v - extreme).abs() > threshold {
                direction = if v > extreme { 1 } else { -1 };
                extreme = v;
            }
        } else if (v - extreme) * direction as f64 > 0.0 {
            extreme = v;
        } else if (v - extreme).abs() > threshold {
            count += 1;
            direction = -direction;
            extreme = v;
        }
    }
    count
}

/// Lap time as `m:ss.mmm`.
pub fn fmt_t(ms: f64) -> String {
    let min = (ms / 60000.0).floor();
    format!("{}:{:06.3}", min as i64, (ms - min * 60000.0) / 1000.0)
}

/// Lap time with the decimal separator of the language (German: comma).
pub fn t_loc(ms: f64) -> String {
    if crate::i18n::german() {
        fmt_t(ms).replace('.', ",")
    } else {
        fmt_t(ms)
    }
}

/// Number with the decimal separator of the language.
pub fn secs(v: f64, digits: usize) -> String {
    crate::i18n::num(v, digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_lap_times() {
        assert_eq!(fmt_t(105_728.0), "1:45.728");
        assert_eq!(fmt_t(5_800.0), "0:05.800");
        assert_eq!(t_loc(131_683.0), "2:11,683");
        assert_eq!(secs(0.229, 2), "0,23");
    }

    #[test]
    fn rounds_like_python() {
        assert_eq!(round_i(2.5), 2);
        assert_eq!(round_i(3.5), 4);
        assert_eq!(round_to(0.125, 2), 0.12);
        assert_eq!(median(&mut [3.0, 1.0, 2.0, 10.0]), 2.5);
    }

    #[test]
    fn finds_segments_with_gaps() {
        let rows: Vec<Row> =
            [0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1].iter().map(|&v| Row { brake: v as f64, ..Default::default() }).collect();
        // gap of one sample is bridged, the long gap splits
        assert_eq!(segments(&rows, |r| r.brake > 0.0, 3, 2), vec![(1, 6), (15, 17)]);
    }

    #[test]
    fn counts_steering_reversals() {
        assert_eq!(reversals(&[0.0, 0.1, 0.2, 0.1, 0.0, 0.1, 0.2, 0.21, 0.2], 0.05), 2);
        assert_eq!(reversals(&[0.0, 0.01, 0.02, 0.01], 0.05), 0);
    }
}
