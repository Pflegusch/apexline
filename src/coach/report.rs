//! Console reports for `analyze`: corner-by-corner comparison of a session, driving style of a
//! single lap and trail braking per braking zone (replaces the former Python tools).

use super::compare::{Comparer, Comparison};
use super::feedback::issue_label;
use super::lap::{self, fmt_t, Row, G, HZ};
use super::trailbrake;
use crate::i18n::{self, corner, dist, speed, speed_unit, speed_value as sv, temp};
use crate::tr;
use std::path::Path;

/// Corner direction as stored in `corners.json` ("links"/"rechts") in the display language.
fn dir_label(dir: &str) -> String {
    match dir {
        "links" => tr!("links", "left"),
        "rechts" => tr!("rechts", "right"),
        other => other.to_string(),
    }
}

fn opt_gear(g: Option<i64>) -> String {
    g.map_or("None".into(), |g| g.to_string())
}

/// Corner-by-corner comparison of all complete laps of a session.
pub fn session(path: &Path) -> std::io::Result<()> {
    let res = Comparer::default().compare(path)?;
    print_comparison(&res);
    println!();
    for lap in &res.laps {
        let mut rows = lap::load(&path.join(&lap.file))?;
        let (ratio, _) = lap::steer_ratio(&rows);
        lap::add_dynamics(&mut rows, ratio);
        let tb = trailbrake::evaluate(&rows);
        let (n, score, verdict) = (lap.lap, tb.score.map_or("–".into(), |s| s.to_string()), trailbrake::verdict(tb.score));
        println!("{}", tr!("R{n} Trail-Braking: {score} / 100 ({verdict})", "L{n} trail braking: {score} / 100 ({verdict})"));
    }
    Ok(())
}

fn print_comparison(res: &Comparison) {
    let Some(reference) = &res.reference else {
        println!("{}", tr!("Noch keine vollständige Runde.", "No complete lap yet."));
        return;
    };
    let (n, t, ratio) = (reference.lap, fmt_t(reference.ms as f64), res.steer_ratio);
    println!("{}", tr!("Referenz: Runde {n} ({t}), Lenkübersetzung {ratio:.1}:1", "Reference: lap {n} ({t}), steering ratio {ratio:.1}:1"));
    let lap_tag = tr!("R", "L");
    let laps: Vec<String> =
        res.laps.iter().map(|l| format!("{lap_tag}{} {} ({:+.3})", l.lap, fmt_t(l.ms as f64), l.delta_ms as f64 / 1000.0)).collect();
    let laps = laps.join(", ");
    println!("{}\n", tr!("Runden: {laps}", "Laps: {laps}"));
    let m = |v: Option<f64>| v.map_or("–".into(), dist);
    let u = speed_unit();
    for c in &res.corners {
        let Some(r) = &c.reference else { continue };
        let (k, dir, at, brake, full, gear) =
            (corner(c.n), dir_label(&c.dir), dist(c.s as f64), m(r.brake_at), m(r.full_at), opt_gear(r.gear_min));
        let (v0, v1, v2, glat, under, over) = (sv(r.v_entry), sv(r.v_min), sv(r.v_exit), r.glat, r.under, r.over);
        println!(
            "{}",
            tr!(
                "{k} ({dir}, {at}): Referenz Bremse bei {brake}, {v0:.0}→{v1:.0}→{v2:.0} {u}, Gang {gear}, Vollgas bei {full}, {glat:.1} g, unter {under:.0}% / über {over:.0}%",
                "{k} ({dir}, {at}): reference brake at {brake}, {v0:.0}→{v1:.0}→{v2:.0} {u}, gear {gear}, full throttle at {full}, {glat:.1} g, under {under:.0}% / over {over:.0}%"
            )
        );
        for l in &res.laps {
            let lc = l.corner(c.n);
            let Some(s) = &lc.stats else { continue };
            let db = s.brake_at.zip(r.brake_at).map(|(a, b)| a - b);
            let dfull = s.full_at.zip(r.full_at).map(|(a, b)| a - b);
            let (n, dt, db, dfull, gear) = (l.lap, lc.dt, m(db), m(dfull), opt_gear(s.gear_min));
            let (dapex, dexit, under, over, rear, tcs) =
                (sv(s.v_min - r.v_min), sv(s.v_exit - r.v_exit), s.under, s.over, s.rear_slide, s.tcs);
            println!(
                "{}",
                tr!(
                    "    R{n}: {dt:+.3} s | Bremse {db} | Scheitel {dapex:+.0} {u} | Ausgang {dexit:+.0} {u} | Vollgas {dfull} | Gang {gear} | unter {under:.0}% über {over:.0}% Heck {rear:.1}° TCS {tcs:.0}%",
                    "    L{n}: {dt:+.3} s | brake {db} | apex {dapex:+.0} {u} | exit {dexit:+.0} {u} | full throttle {dfull} | gear {gear} | under {under:.0}% over {over:.0}% rear {rear:.1}° TCS {tcs:.0}%"
                )
            );
        }
    }
    println!();
    let mut pot: Vec<(f64, usize)> = res.corners.iter().map(|c| (lap::mean(res.laps.iter().map(|l| l.corner(c.n).dt)), c.n)).collect();
    pot.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    let pot = pot.iter().map(|(v, n)| format!("{} {v:+.2}s", corner(*n))).collect::<Vec<_>>().join(", ");
    println!("{}", tr!("Ø Zeitverlust zur Referenz je Kurve: {pot}", "Avg time lost to the reference per corner: {pot}"));
    let mut better = Vec::new();
    for c in &res.corners {
        if let Some(l) = res.laps.iter().min_by(|a, b| a.corner(c.n).dt.total_cmp(&b.corner(c.n).dt)) {
            if l.corner(c.n).dt < -0.02 {
                better.push(format!("{} {lap_tag}{}", corner(c.n), l.lap));
            }
        }
    }
    if let Some(th) = res.theoretical_ms {
        let better = better.join(", ");
        let suffix = if better.is_empty() {
            String::new()
        } else {
            tr!(" – besser als Referenz in: {better}", " – better than the reference in: {better}")
        };
        let t = fmt_t(th);
        println!(
            "{}",
            tr!("Theoretische Bestzeit (schnellste Runde je Kurve): {t}{suffix}", "Theoretical best (fastest lap per corner): {t}{suffix}")
        );
    }
}

/// Driving style of a single lap file plus trail braking per zone.
pub fn lap_file(path: &Path) -> std::io::Result<()> {
    let mut rows = lap::load(path)?;
    if rows.is_empty() {
        let p = path.display();
        println!("{}", tr!("{p}: keine Daten", "{p}: no data"));
        return Ok(());
    }
    let (ratio, n_cal) = lap::steer_ratio(&rows);
    lap::add_dynamics(&mut rows, ratio);
    let n = rows.len();
    let nf = n as f64;
    let share = |pred: &dyn Fn(&Row) -> bool| rows.iter().filter(|r| pred(r)).count() as f64 / nf * 100.0;
    let u = speed_unit();
    println!("\n=== {} ===", path.display());
    let (lap_no, dur) = (rows[0].lap as i64, fmt_t(nf / HZ * 1000.0));
    println!(
        "{}",
        tr!(
            "Runde {lap_no}: {n} Pakete, {dur} aufgezeichnet, Lenkübersetzung ≈ {ratio:.1}:1 ({n_cal} Kalibrierpunkte)",
            "Lap {lap_no}: {n} packets, {dur} recorded, steering ratio ≈ {ratio:.1}:1 ({n_cal} calibration points)"
        )
    );
    let v_max = sv(rows.iter().map(|r| r.speed_kmh).fold(f64::MIN, f64::max));
    let moving = sv(lap::mean(rows.iter().filter(|r| r.speed_kmh > 5.0).map(|r| r.speed_kmh)));
    let avg = sv(lap::mean(rows.iter().map(|r| r.speed_kmh)));
    println!(
        "{}",
        tr!(
            "Tempo: max {v_max:.0} {u}, Ø {avg:.0} {u} (Ø in Fahrt {moving:.0})",
            "Speed: max {v_max:.0} {u}, avg {avg:.0} {u} (avg while moving {moving:.0})"
        )
    );
    let (full, part, brake) =
        (share(&|r| r.throttle_pct() > 98.0), share(&|r| (5.0..=98.0).contains(&r.throttle_pct())), share(&|r| r.brake_pct() > 5.0));
    let coast = share(&|r| r.throttle_pct() < 5.0 && r.brake_pct() < 5.0 && r.speed_kmh > 30.0);
    let both = share(&|r| r.throttle_pct() > 10.0 && r.brake_pct() > 10.0);
    let v30 = speed(30.0);
    println!(
        "{}",
        tr!(
            "Vollgas {full:.0} %, Teilgas {part:.0} %, Bremse {brake:.0} %, Rollen (>{v30}, kein Pedal) {coast:.0} %, Gas+Bremse gleichzeitig {both:.1} %",
            "Full throttle {full:.0} %, part throttle {part:.0} %, brake {brake:.0} %, coasting (>{v30}, no pedal) {coast:.0} %, throttle+brake together {both:.1} %"
        )
    );

    let stops = lap::segments(&rows, |r| r.speed_kmh < 15.0, 30, 6);
    let spins = lap::segments(&rows, |r| r.beta.is_some_and(|b| b.abs() > 20.0), 3, 6);
    let (n_spins, n_stops, v15) = (spins.len(), stops.len(), speed(15.0));
    println!(
        "{}",
        tr!(
            "Zwischenfälle: {n_spins} Dreher/große Rutscher (Schwimmwinkel > 20°), {n_stops}× unter {v15} (> 0,5 s)",
            "Incidents: {n_spins} spins/big slides (body slip > 20°), {n_stops}× below {v15} (> 0.5 s)"
        )
    );
    for (s, _) in &spins {
        let r = &rows[*s];
        let (at, v, gear, thr, brk, steer) =
            (fmt_t(r.lap_ms.unwrap_or(0.0)), speed(r.speed_kmh), r.gear as i64, r.throttle_pct(), r.brake_pct(), r.steering.to_degrees());
        println!(
            "{}",
            tr!(
                "   Dreher bei {at}: {v}, Gang {gear}, Gas {thr:.0} %, Bremse {brk:.0} %, Lenkung {steer:.0}°",
                "   Spin at {at}: {v}, gear {gear}, throttle {thr:.0} %, brake {brk:.0} %, steering {steer:.0}°"
            )
        );
    }
    if !stops.is_empty() {
        let t = i18n::num(stops.iter().map(|(s, e)| e - s + 1).sum::<usize>() as f64 / HZ, 1);
        println!("{}", tr!("   Zeit unter {v15}: {t} s", "   Time below {v15}: {t} s"));
    }

    let zones = lap::segments(&rows, |r| r.brake_pct() > 20.0, 10, 10);
    let nz = zones.len();
    println!("\n{}", tr!("Bremszonen: {nz}", "Braking zones: {nz}"));
    for &(s, e) in &zones {
        let seg = &rows[s..=e];
        let peak = (s..=e).fold(s, |b, i| if rows[i].brake > rows[b].brake { i } else { b });
        let decel = -seg.iter().map(|r| r.surge).fold(f64::INFINITY, f64::min) / G;
        let len = seg.len() as f64;
        let trail = seg.iter().filter(|r| r.steering.to_degrees().abs() > 15.0).count() as f64 / len * 100.0;
        let lock = seg
            .iter()
            .filter(|r| {
                r.speed_kmh > 30.0
                    && r.vf > 1.0
                    && (r.wheel_rps[0].abs() * r.tire_radius[0]).min(r.wheel_rps[1].abs() * r.tire_radius[1]) / r.vf < 0.85
            })
            .count() as f64
            / len
            * 100.0;
        let (at, v0, v1, dur) = (fmt_t(seg[0].lap_ms.unwrap_or(0.0)), sv(seg[0].speed_kmh), sv(seg[seg.len() - 1].speed_kmh), len / HZ);
        let (pk, pk_t) = (rows[peak].brake_pct(), (peak - s) as f64 / HZ);
        println!(
            "{}",
            tr!(
                "   {at}: {v0:.0} → {v1:.0} {u} in {dur:.1} s, Spitze {pk:.0} % nach {pk_t:.2} s, max {decel:.1} g, Trail-Braking {trail:.0} %, Blockieren {lock:.0} %",
                "   {at}: {v0:.0} → {v1:.0} {u} in {dur:.1} s, peak {pk:.0} % after {pk_t:.2} s, max {decel:.1} g, trail braking {trail:.0} %, locking {lock:.0} %"
            )
        );
    }

    let corners = lap::segments(&rows, |r| r.vf > 8.0 && r.sway.abs() > 6.0, 20, 15);
    let nc = corners.len();
    println!("\n{}", tr!("Kurven: {nc}", "Corners: {nc}"));
    for &(s, e) in &corners {
        let seg = &rows[s..=e];
        let imin = (s..=e).fold(s, |b, i| if rows[i].speed_kmh < rows[b].speed_kmh { i } else { b });
        let bals: Vec<f64> = seg.iter().filter_map(|r| r.bal).collect();
        let pct = |c: usize| if bals.is_empty() { 0.0 } else { c as f64 / bals.len() as f64 * 100.0 };
        let glat = seg.iter().map(|r| r.sway.abs()).fold(f64::MIN, f64::max) / G;
        let full_after = (imin..n.min(e + 120)).find(|&i| rows[i].throttle > 250.0);
        let len = seg.len() as f64;
        let spin_rear = seg
            .iter()
            .filter(|r| {
                r.throttle > 100.0
                    && r.speed_kmh > 30.0
                    && r.vf > 1.0
                    && (r.wheel_rps[2].abs() * r.tire_radius[2]).max(r.wheel_rps[3].abs() * r.tire_radius[3]) / r.vf > 1.15
            })
            .count() as f64
            / len
            * 100.0;
        let steering: Vec<f64> = seg.iter().map(|r| r.steering).collect();
        let corrections = lap::reversals(&steering, 0.05) as f64 / (len / HZ);
        let dir = dir_label(if lap::mean(seg.iter().map(|r| r.yaw)) > 0.0 { "links" } else { "rechts" });
        let (at, v0, v1, v2) =
            (fmt_t(seg[0].lap_ms.unwrap_or(0.0)), sv(seg[0].speed_kmh), sv(rows[imin].speed_kmh), sv(seg[seg.len() - 1].speed_kmh));
        let (under, over) = (pct(bals.iter().filter(|b| **b > 2.0).count()), pct(bals.iter().filter(|b| **b < -1.5).count()));
        let full = full_after.map_or("–".into(), |i| format!("{:.1} s", (i - imin) as f64 / HZ));
        println!(
            "{}",
            tr!(
                "   {at} ({dir}): Einlenken {v0:.0} → Scheitel {v1:.0} → Ausgang {v2:.0} {u}, {glat:.1} g quer, Untersteuern {under:.0} % / Übersteuern {over:.0} %, \
                 Vollgas {full} nach Scheitel, Radschlupf hinten {spin_rear:.0} %, Lenkkorrekturen {corrections:.1}/s",
                "   {at} ({dir}): turn-in {v0:.0} → apex {v1:.0} → exit {v2:.0} {u}, {glat:.1} g lateral, understeer {under:.0} % / oversteer {over:.0} %, \
                 full throttle {full} after the apex, rear wheel slip {spin_rear:.0} %, steering corrections {corrections:.1}/s"
            )
        );
    }

    let ups: Vec<f64> = rows
        .windows(2)
        .filter(|w| w[0].gear > 0.0 && w[0].gear < 15.0 && w[1].gear > 0.0 && w[1].gear < 15.0 && w[1].gear == w[0].gear + 1.0)
        .map(|w| w[0].rpm)
        .collect();
    if !ups.is_empty() {
        let (k, avg, min, max) = (
            ups.len(),
            lap::mean(ups.iter().copied()),
            ups.iter().copied().fold(f64::INFINITY, f64::min),
            ups.iter().copied().fold(f64::MIN, f64::max),
        );
        println!(
            "\n{}",
            tr!(
                "Hochschalten: {k}×, Drehzahl Ø {avg:.0}, min {min:.0}, max {max:.0}",
                "Upshifts: {k}×, rpm avg {avg:.0}, min {min:.0}, max {max:.0}"
            )
        );
    }
    let wheels = if i18n::german() { ["VL", "VR", "HL", "HR"] } else { ["FL", "FR", "RL", "RR"] };
    for (w, name) in wheels.iter().enumerate() {
        let temps: Vec<f64> = rows.iter().filter(|r| r.speed_kmh > 30.0).map(|r| r.tire_temp[w]).collect();
        if !temps.is_empty() {
            let (avg, max, t100) =
                (temp(lap::mean(temps.iter().copied())), temp(temps.iter().copied().fold(f64::MIN, f64::max)), temp(100.0));
            let hot = temps.iter().filter(|t| **t > 100.0).count() as f64 / temps.len() as f64 * 100.0;
            print!(
                "{}",
                tr!(
                    "Reifen {name}: Ø {avg}, max {max}, über {t100}: {hot:.0} %; ",
                    "Tyre {name}: avg {avg}, max {max}, above {t100}: {hot:.0} %; "
                )
            );
        }
    }
    println!();
    let (tcs, asm) = (share(&|r| r.has_flag(11)), share(&|r| r.has_flag(10)));
    println!("{}", tr!("TCS aktiv {tcs:.1} %, ASM aktiv {asm:.1} %", "TCS active {tcs:.1} %, ASM active {asm:.1} %"));
    let mut bals: Vec<f64> = rows.iter().filter(|r| r.sway.abs() > 6.0).filter_map(|r| r.bal).collect();
    if !bals.is_empty() {
        let k = bals.len() as f64;
        let (under, over) =
            (bals.iter().filter(|b| **b > 2.0).count() as f64 / k * 100.0, bals.iter().filter(|b| **b < -1.5).count() as f64 / k * 100.0);
        let med = lap::median(&mut bals);
        println!(
            "{}",
            tr!(
                "Balance in Kurven: Median {med:+.1}°, Untersteuern {under:.0} %, Übersteuern {over:.0} %",
                "Balance in corners: median {med:+.1}°, understeer {under:.0} %, oversteer {over:.0} %"
            )
        );
    }

    let tb = trailbrake::evaluate(&rows);
    let (score, verdict) = (tb.score.map_or("–".into(), |s| s.to_string()), trailbrake::verdict(tb.score));
    println!("\n{}", tr!("Trail-Braking {score} / 100 ({verdict})", "Trail braking {score} / 100 ({verdict})"));
    for z in &tb.zones {
        let iss = z.issues.iter().map(|(a, b)| format!("{} ({b})", issue_label(a))).collect::<Vec<_>>().join("; ");
        let iss = if iss.is_empty() { tr!("sauber", "clean") } else { iss };
        let (at, v0, v1, score, bt, trail, coast, gd) =
            (fmt_t(z.lap_ms.unwrap_or(0.0)), sv(z.v_start), sv(z.v_apex), z.score, z.brake_at_turn, z.trail_s, z.coast_s, z.g_dip * 100.0);
        println!(
            "{}",
            tr!(
                "  {at} {v0:.0}→{v1:.0} {u}: {score:3} | Einlenken mit {bt}% Bremse, Trail {trail} s, Rollen {coast} s, G-Nutzung {gd:.0}% | {iss}",
                "  {at} {v0:.0}→{v1:.0} {u}: {score:3} | turn-in with {bt}% brake, trail {trail} s, coasting {coast} s, grip use {gd:.0}% | {iss}"
            )
        );
    }
    Ok(())
}
