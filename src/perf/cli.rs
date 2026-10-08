//! `apexline perf <session|lap.csv>… [--save]`: finds measurement runs in recorded laps,
//! e.g. drives from before the measurements existed, and prints them; `--save` stores them like
//! live runs so they show up on the dashboard.

use super::{store::is_brake_key, Detector, Kind, Mark, Run, Sample, Store};
use crate::i18n::{self, num};
use crate::tr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn is_distance(key: &str) -> bool {
    key.ends_with("mi") || key.ends_with("km")
}

/// Marks of the configured unit system (mph keys for imperial, km/h keys for metric).
fn shown(m: &Mark) -> bool {
    let imperial = i18n::imperial();
    match m.key.as_str() {
        "1km" => !imperial,
        "1/2mi" => imperial,
        k if is_distance(k) => true,
        k => k.ends_with("mph") == imperial,
    }
}

pub fn label(key: &str) -> String {
    let mile = tr!("Meile", "mile");
    match key {
        "1/8mi" => format!("1/8 {mile}"),
        "1/4mi" => format!("1/4 {mile}"),
        "1/2mi" => format!("1/2 {mile}"),
        "1km" => "1 km".into(),
        k => match k.strip_suffix("mph") {
            Some(k) => format!("{} mph", k.replace('-', "–")),
            None => format!("{} km/h", k.replace('-', "–")),
        },
    }
}

/// One-line summary of a run (console and server log).
pub fn headline(r: &Run) -> String {
    match r.kind {
        Kind::Accel => {
            let keys = if i18n::imperial() { ["0-60mph", "0-100mph", "1/4mi"] } else { ["0-100", "0-200", "0-300"] };
            let mut parts: Vec<String> =
                keys.iter().filter_map(|k| r.mark(k)).map(|m| format!("{} {} s", label(&m.key), num(m.t as f64, 2))).collect();
            parts.push(format!("max. {}", i18n::speed(r.v_max as f64)));
            let parts = parts.join(" · ");
            tr!("Beschleunigung – {parts}", "Acceleration – {parts}")
        }
        Kind::Brake => {
            let (v, d, t) = (i18n::speed(r.v_start as f64), i18n::dist_prec(r.dist_m as f64, 1), num(r.duration_s as f64, 2));
            tr!("Bremsung aus {v} – {d} in {t} s", "Braking from {v} – {d} in {t} s")
        }
    }
}

fn details(r: &Run) -> Vec<String> {
    let mut lines = Vec::new();
    let marks: Vec<&Mark> = r.marks.iter().filter(|m| shown(m)).collect();
    let speed: Vec<String> = marks
        .iter()
        .filter(|m| !is_distance(&m.key))
        .map(|m| {
            if is_brake_key(&m.key) {
                format!("{} {} ({} s)", label(&m.key), i18n::dist_prec(m.d as f64, 1), num(m.t as f64, 2))
            } else {
                format!("{} {} s", label(&m.key), num(m.t as f64, 2))
            }
        })
        .collect();
    let dist: Vec<String> = marks
        .iter()
        .filter(|m| is_distance(&m.key))
        .map(|m| format!("{} {} s ({})", label(&m.key), num(m.t as f64, 2), i18n::speed(m.v as f64)))
        .collect();
    for chunk in speed.chunks(6) {
        lines.push(chunk.join(" · "));
    }
    if !dist.is_empty() {
        lines.push(dist.join(" · "));
    }
    if let Some(d) = &r.dyno {
        let (rpm, unit) = (d.peak[0], i18n::rpm_unit());
        lines.push(tr!("Leistungsspitze bei {rpm:.0} {unit}", "Peak power at {rpm:.0} {unit}"));
        let shifts: Vec<String> = d
            .shifts
            .iter()
            .map(|s| {
                let (g, n, rpm) = (s.gear, s.gear + 1, s.rpm);
                let best = if s.late { tr!("spät (Begrenzer)", "late (limiter)") } else { format!("{rpm:.0}") };
                match s.actual {
                    Some(a) => tr!("{g}→{n} {best} (gefahren {a:.0})", "{g}→{n} {best} (driven {a:.0})"),
                    None => format!("{g}→{n} {best}"),
                }
            })
            .collect();
        if !shifts.is_empty() {
            let shifts = shifts.join(" · ");
            lines.push(tr!("Beste Schaltpunkte ({unit}): {shifts}", "Best shift points ({unit}): {shifts}"));
        }
    }
    let sloped: Vec<String> =
        marks.iter().filter(|m| m.g.abs() > 2.0).map(|m| format!("{} {} %", label(&m.key), num(m.g as f64, 1))).collect();
    if !sloped.is_empty() {
        let sloped = sloped.join(" · ");
        lines.push(tr!("Steigung/Gefälle (zählt nicht als Bestwert): {sloped}", "Slope (does not count as a best value): {sloped}"));
    }
    lines
}

/// Reads the channels a measurement needs from a recorded lap file.
fn samples(path: &Path) -> io::Result<Vec<Sample>> {
    let text = fs::read_to_string(path)?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
    let col = |name: &str| header.iter().position(|c| *c == name);
    let cols = ["packet_id", "flags", "speed_kmh", "rpm", "gear", "throttle", "brake", "pos_y"].map(col);
    // Recorded since 08.10.2026; older files have no transmission ratios
    let ratio_cols: Vec<Option<usize>> = (1..=8).map(|g| col(&format!("gear_ratio_{g}"))).collect();
    let [Some(tick), Some(flags), Some(speed), Some(rpm), Some(gear), Some(thr), Some(brk), Some(y)] = cols else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            tr!("alte Aufzeichnung ohne die nötigen Spalten", "old recording without the required columns"),
        ));
    };
    Ok(lines
        .filter_map(|line| {
            let c: Vec<&str> = line.split(',').collect();
            let f = |i: usize| c.get(i)?.parse::<f32>().ok();
            Some(Sample {
                tick: f(tick)? as i32,
                flags: f(flags)? as u16,
                speed_kmh: f(speed)?,
                rpm: f(rpm)?,
                gear: f(gear)? as u8,
                throttle: f(thr)? as u8,
                brake: f(brk)? as u8,
                y: f(y)?,
                gear_ratios: std::array::from_fn(|i| ratio_cols[i].and_then(f).unwrap_or(0.0)),
            })
        })
        .collect())
}

fn lap_files(dir: &Path) -> Vec<PathBuf> {
    crate::coach::compare::lap_files(dir)
}

/// Car and start time of a session, plus the packet counter of its first sample.
struct SessionInfo {
    name: String,
    car_code: i32,
    car: String,
    started: chrono::DateTime<chrono::FixedOffset>,
    first_tick: i32,
}

fn session_info(dir: &Path) -> Option<SessionInfo> {
    let meta: serde_json::Value = serde_json::from_str(&fs::read_to_string(dir.join("session.json")).ok()?).ok()?;
    let first_tick = lap_files(dir).iter().find_map(|f| samples(f).ok()?.first().map(|s| s.tick))?;
    Some(SessionInfo {
        name: dir.file_name()?.to_string_lossy().into_owned(),
        car_code: meta["car_code"].as_i64()? as i32,
        car: meta["car"].as_str()?.to_string(),
        started: chrono::DateTime::parse_from_rfc3339(meta["started"].as_str()?).ok()?,
        first_tick,
    })
}

pub fn run(paths: &[PathBuf], store: Option<&Store>) {
    for path in paths {
        let (files, dir) = if path.is_dir() {
            (lap_files(path), path.clone())
        } else {
            (vec![path.clone()], path.parent().map(Path::to_path_buf).unwrap_or_default())
        };
        let info = session_info(&dir);
        println!("{}", path.display());

        // Laps of a session are fed in order: a run may continue across the start/finish line
        let mut det = Detector::default();
        let mut runs = Vec::new();
        for f in &files {
            match samples(f) {
                Ok(samples) => runs.extend(samples.into_iter().filter_map(|s| det.update(s))),
                Err(e) => eprintln!("  {}: {e}", f.display()),
            }
        }
        runs.extend(det.flush());
        if runs.is_empty() {
            println!("  {}", tr!("keine Messläufe gefunden", "no measurement runs found"));
        }

        for mut run in runs {
            if let Some(i) = &info {
                let offset = (run.start_tick - i.first_tick).max(0) as i64 * 1000 / 60;
                let start = (i.started + chrono::TimeDelta::milliseconds(offset)).with_timezone(&chrono::Local);
                run.stamp(i.car_code, &i.car, start);
                run.source = Some(i.name.clone());
            }
            let time = chrono::DateTime::parse_from_rfc3339(&run.time).map_or("--:--:--".into(), |t| t.format("%H:%M:%S").to_string());
            println!("  {time}  {}", headline(&run));
            for line in details(&run) {
                println!("            {line}");
            }
            match (store, &info) {
                (Some(store), Some(_)) => match store.save(&store.dedupe(run)) {
                    Ok(p) => {
                        let p = p.display();
                        println!("            {}", tr!("gespeichert: {p}", "saved: {p}"));
                    }
                    Err(e) => eprintln!("            {}", tr!("nicht gespeichert: {e}", "not saved: {e}")),
                },
                (Some(_), None) => eprintln!(
                    "            {}",
                    tr!("nicht gespeichert: session.json fehlt (Auto unbekannt)", "not saved: session.json missing (car unknown)")
                ),
                _ => {}
            }
        }
    }
}
