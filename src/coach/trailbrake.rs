//! Trail-braking rating per braking zone.
//!
//! Trail braking = not releasing the brake abruptly at turn-in but bleeding the pressure off towards
//! the apex. That keeps load on the front axle and uses the combined grip (braking + cornering) in
//! the transition. Rated per braking zone before a corner:
//!
//! - trail duration: how long braking continues after turn-in, and with which pressure
//! - coasting: time with neither brake nor throttle before the apex (wasted grip)
//! - g-dip: drop of the combined acceleration between braking and cornering
//! - release: smooth bleed-off instead of an abrupt release or re-applying
//! - consequences: front lock-up, understeer or the rear stepping out while trail braking

use super::lap::{self, Row, G, HZ};
use crate::{i18n, tr};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Zone {
    pub lap_ms: Option<f64>,
    pub s: Option<f64>,
    pub x: f64,
    pub z: f64,
    pub v_start: f64,
    pub v_apex: f64,
    pub peak_brake: i64,
    pub brake_at_turn: i64,
    pub trail_s: f64,
    pub gap_s: f64,
    pub coast_s: f64,
    pub g_dip: f64,
    pub brake_g: f64,
    pub lat_g: f64,
    pub lock: i64,
    pub under: i64,
    pub rear: i64,
    pub abrupt: bool,
    pub reapply: i64,
    pub score: i64,
    /// (issue, detail) – the issue name is used for grouping and tips
    pub issues: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TrailBraking {
    pub score: Option<i64>,
    pub zones: Vec<Zone>,
}

fn front_lock(r: &Row) -> bool {
    if r.vf < 8.0 {
        return false;
    }
    let v = (r.wheel_rps[0].abs() * r.tire_radius[0]).min(r.wheel_rps[1].abs() * r.tire_radius[1]);
    v / r.vf < 0.85
}

/// Index of the slowest sample in `range` (first one on ties).
fn slowest(rows: &[Row], range: std::ops::RangeInclusive<usize>) -> usize {
    let start = *range.start();
    range.fold(start, |best, i| if rows[i].speed_kmh < rows[best].speed_kmh { i } else { best })
}

/// Braking zones followed by a corner: (brake start, turn-in, brake end, apex).
pub fn zones(rows: &[Row]) -> Vec<(usize, usize, usize, usize)> {
    let mut out = Vec::new();
    for (s, e) in lap::segments(rows, |r| r.brake_pct() > 10.0, 12, 8) {
        let win_end = (rows.len() - 1).min(e + (3.0 * HZ) as usize);
        let mut apex = slowest(rows, s..=win_end);
        // Corner direction at the apex; only lateral force in that direction counts as turn-in
        // (otherwise the previous corner of an S-bend is taken for the turn-in)
        let around = s.max(apex.saturating_sub(12))..=win_end.min(apex + 12);
        let direction = if around.map(|i| rows[i].sway).sum::<f64>() > 0.0 { 1.0 } else { -1.0 };
        let lat_peak = (s..=win_end).map(|i| rows[i].sway * direction).fold(f64::MIN, f64::max);
        if lat_peak < 8.0 {
            continue; // braking on a straight without a real corner
        }
        let Some(turn) = (s..=win_end).find(|&i| rows[i].sway * direction > 0.4 * lat_peak) else { continue };
        if turn > apex {
            apex = slowest(rows, turn..=win_end);
        }
        out.push((s, turn, e, apex));
    }
    out
}

fn evaluate_zone(rows: &[Row], s: usize, turn: usize, e: usize, apex: usize) -> Zone {
    let peak_brake = (s..=e).map(|i| rows[i].brake).fold(f64::MIN, f64::max) / 2.55;
    let brake_at_turn = rows[turn].brake_pct();
    let trail_end = if e >= turn { e } else { turn };
    let trail = e.saturating_sub(turn) as f64 / HZ;
    let gap_before = turn.saturating_sub(e) as f64 / HZ; // brake released before turn-in

    // Coasting between brake release and throttle (until apex + 0.5 s)
    let mut coast = 0usize;
    for r in rows.iter().take(rows.len().min(apex + (0.5 * HZ) as usize)).skip(e + 1) {
        if r.throttle_pct() > 10.0 {
            break;
        }
        if r.brake_pct() < 5.0 {
            coast += 1;
        }
    }
    let coast = coast as f64 / HZ;

    // G-dip: combined acceleration right around turn-in relative to the weaker of the two peaks.
    // Only this window, so direction changes in chicanes (lateral force crossing zero) don't count.
    let comb = |r: &Row| r.sway.hypot(r.surge) / G;
    let brake_g = (s..=e).map(|i| -rows[i].surge).fold(f64::MIN, f64::max) / G;
    let lat_g = (turn..=turn.max(apex)).map(|i| rows[i].sway.abs()).fold(f64::MIN, f64::max) / G;
    let lo = s.max(turn.saturating_sub((0.2 * HZ) as usize));
    let hi = apex.min(turn + (0.4 * HZ) as usize);
    let mut window: Vec<f64> = if hi >= lo { (lo..=hi).map(|i| comb(&rows[i])).collect() } else { Vec::new() };
    window.sort_by(f64::total_cmp);
    let dip = if window.is_empty() { 1.0 } else { window[window.len() / 10] / 0.1f64.max(brake_g.min(lat_g)) };

    // Release: re-applying (pressure rises again after turn-in) or abrupt (>60 % in 0.1 s)
    let seg: Vec<f64> = if e >= turn { (turn..=e).map(|i| rows[i].brake_pct()).collect() } else { Vec::new() };
    // Re-applying: within 0.15 s after turn-in the pressure rises by >20 points to >30 %
    let (mut reapply, mut i) = (0i64, 0usize);
    while i + 9 < seg.len() {
        if seg[i + 9] - seg[i] > 20.0 && seg[i + 9] > 30.0 {
            reapply += 1;
            i += 30;
        } else {
            i += 1;
        }
    }
    let mut abrupt = seg.len() > 6 && (0..seg.len() - 6).any(|i| seg[i] - seg[i + 6] > 60.0);
    if e < turn && peak_brake > 50.0 {
        // released before turn-in: how quickly?
        let tail: Vec<f64> = (s.max(e.saturating_sub(6))..=e).map(|i| rows[i].brake_pct()).collect();
        abrupt = tail[0] - tail[tail.len() - 1] > 60.0;
    }

    let trail_rows: Vec<&Row> = (turn..=trail_end).map(|i| &rows[i]).collect();
    let share = |pred: &dyn Fn(&Row) -> bool| trail_rows.iter().filter(|r| pred(r)).count() as f64 / trail_rows.len() as f64 * 100.0;
    let lock = share(&front_lock);
    let under = share(&|r: &Row| r.bal.is_some_and(|b| b > 2.5));
    let rear = share(&|r: &Row| r.ar.is_some_and(|a| a.abs() > 5.0));

    let mut score = 100.0;
    let mut issues: Vec<(String, String)> = Vec::new();
    let mut issue = |name: &str, detail: String| issues.push((name.to_string(), detail));
    if trail < 0.15 && gap_before > 0.15 {
        score -= 30.0;
        let g = i18n::num(gap_before, 1);
        issue("kein Trail-Braking", tr!("Bremse {g} s vor dem Einlenken gelöst", "brake released {g} s before turn-in"));
    } else if brake_at_turn > 85.0 && trail > 0.3 {
        score -= 10.0;
        issue("zu viel Druck beim Einlenken", tr!("{brake_at_turn:.0} % Bremse beim Einlenken", "{brake_at_turn:.0} % brake on turn-in"));
    }
    if coast > 0.3 {
        score -= (coast * 35.0).min(25.0);
        let c = i18n::num(coast, 1);
        issue("Rollphase", tr!("{c} s weder Bremse noch Gas", "{c} s neither brake nor throttle"));
    }
    if dip < 0.75 {
        score -= ((0.75 - dip) * 100.0).min(25.0);
        let d = dip * 100.0;
        issue("G-Loch", tr!("Haftung im Übergang nur zu {d:.0} % genutzt", "only {d:.0} % of the grip used in the transition"));
    }
    if lock > 10.0 {
        score -= (lock * 0.5).min(25.0);
        issue("Blockieren", tr!("Vorderräder {lock:.0} % der Trail-Phase blockiert", "front wheels locked {lock:.0} % of the trail phase"));
    }
    if under > 30.0 {
        score -= 15.0;
        issue("Untersteuern", tr!("{under:.0} % Untersteuern beim Einlenken", "{under:.0} % understeer on turn-in"));
    }
    if rear > 20.0 {
        score -= 15.0;
        issue("Heck kommt", tr!("Hinterachse rutscht {rear:.0} % der Trail-Phase", "rear axle sliding {rear:.0} % of the trail phase"));
    }
    if abrupt {
        score -= 10.0;
        issue("abrupt gelöst", tr!("Bremse schlagartig losgelassen", "brake let go abruptly"));
    }
    if reapply > 0 {
        score -= 5.0 * reapply.min(3) as f64;
        issue("nachgetreten", tr!("Bremsdruck beim Einlenken wieder erhöht", "brake pressure increased again on turn-in"));
    }

    Zone {
        lap_ms: rows[s].lap_ms,
        s: rows[s].s,
        x: rows[apex].pos_x,
        z: rows[apex].pos_z,
        v_start: rows[s].speed_kmh,
        v_apex: rows[apex].speed_kmh,
        peak_brake: lap::round_i(peak_brake),
        brake_at_turn: lap::round_i(brake_at_turn),
        trail_s: lap::round_to(trail, 2),
        gap_s: lap::round_to(gap_before, 2),
        coast_s: lap::round_to(coast, 2),
        g_dip: lap::round_to(dip, 2),
        brake_g: lap::round_to(brake_g, 2),
        lat_g: lap::round_to(lat_g, 2),
        lock: lap::round_i(lock),
        under: lap::round_i(under),
        rear: lap::round_i(rear),
        abrupt,
        reapply,
        score: lap::round_i(score).max(0),
        issues,
    }
}

/// All braking zones of a lap; `rows` need `lap::add_dynamics`.
pub fn evaluate(rows: &[Row]) -> TrailBraking {
    let zones: Vec<Zone> = zones(rows).into_iter().map(|(s, t, e, a)| evaluate_zone(rows, s, t, e, a)).collect();
    if zones.is_empty() {
        return TrailBraking::default();
    }
    // Harder braking zones weigh more
    let weights: Vec<f64> = zones.iter().map(|z| (z.v_start - z.v_apex).max(10.0)).collect();
    let score = zones.iter().zip(&weights).map(|(z, w)| z.score as f64 * w).sum::<f64>() / weights.iter().sum::<f64>();
    TrailBraking { score: Some(lap::round_i(score)), zones }
}

pub fn verdict(score: Option<i64>) -> String {
    match score {
        None => "–".into(),
        Some(s) if s >= 85 => tr!("sehr gut", "very good"),
        Some(s) if s >= 70 => tr!("gut", "good"),
        Some(s) if s >= 50 => tr!("ausbaufähig", "room to improve"),
        Some(_) => tr!("schwach", "weak"),
    }
}
