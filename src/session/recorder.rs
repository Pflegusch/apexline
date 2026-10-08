//! Records every packet (60/s) as CSV: `recordings/<date>_<car>/<no>_lap<lap>.csv`.
//! Writing happens in a thread of its own, so receiving never blocks.
//!
//! Every session has a `session.json` (car, start, end, reason). The coach detects the end of a
//! session from it. When the server restarts (update, crash), a session that is still open and was
//! written shortly before is continued with the same car instead of being split.

use crate::telemetry::dynamics::Dynamics;
use crate::telemetry::packet::Packet;
use crate::tr;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

/// Continue an open session after a server restart if it was written this recently
const RESTART_RESUME: Duration = Duration::from_secs(600);

pub enum Msg {
    /// New session, or the current one continued (`resume`)
    Session {
        car: String,
        car_code: i32,
        resume: bool,
    },
    End {
        reason: &'static str,
    },
    Lap(i16),
    Row(String),
}

pub struct Recorder {
    tx: mpsc::Sender<Msg>,
}

const HEADER: &str = "lap,lap_ms,delta_ms,packet_id,flags,speed_kmh,rpm,gear,suggested_gear,throttle,brake,clutch_pedal,clutch_engagement,rpm_clutch_gearbox,\
steering,sway,heave,surge,\
pos_x,pos_y,pos_z,vel_x,vel_y,vel_z,rot_x,rot_y,rot_z,rot_w,angvel_x,angvel_y,angvel_z,\
v_fwd,v_lat,yaw_rate,heading,pitch,roll,body_slip,slip_front,slip_rear,balance,\
body_height,road_plane_x,road_plane_y,road_plane_z,road_plane_distance,\
tire_temp_fl,tire_temp_fr,tire_temp_rl,tire_temp_rr,wheel_rps_fl,wheel_rps_fr,wheel_rps_rl,wheel_rps_rr,\
tire_radius_fl,tire_radius_fr,tire_radius_rl,tire_radius_rr,susp_fl,susp_fr,susp_rl,susp_rr,\
fuel,boost_bar,oil_pressure,water_temp,oil_temp,energy_recovery,time_of_day_ms,\
unk_b0,unk_b1,unk_b2,unk_b3,unk_f0,unk_f1,unk_f2,unk_f3,unk_f4,\
last_lap_ms,best_lap_ms,rpm_alert_min,rpm_alert_max,total_laps,\
gear_ratio_1,gear_ratio_2,gear_ratio_3,gear_ratio_4,gear_ratio_5,gear_ratio_6,gear_ratio_7,gear_ratio_8";

impl Recorder {
    /// `wake` is notified whenever a lap file is finished or a session ends (coach trigger).
    pub fn start(dir: PathBuf, wake: Option<mpsc::Sender<()>>) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || writer(dir, rx, wake));
        Recorder { tx }
    }

    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
    }

    pub fn row(p: &Packet, d: &Dynamics, lap_ms: Option<i64>, delta_ms: Option<i64>) -> String {
        let mut s = String::with_capacity(1024);
        let opt = |v: Option<f32>| v.map_or(String::new(), |v| format!("{v:.3}"));
        let _ = write!(
            s,
            "{},{},{},{},{},{:.2},{:.0},{},{},{},{},{:.3},{:.3},{:.0},{:.4},{:.4},{:.4},{:.4},",
            p.current_lap,
            lap_ms.map_or(String::new(), |v| v.to_string()),
            delta_ms.map_or(String::new(), |v| v.to_string()),
            p.packet_id,
            p.flags,
            p.speed_ms * 3.6,
            p.engine_rpm,
            p.gear,
            p.suggested_gear,
            p.throttle,
            p.brake,
            p.clutch_pedal,
            p.clutch_engagement,
            p.rpm_clutch_gearbox,
            p.steering,
            p.sway,
            p.heave,
            p.surge,
        );
        for v in p.position.iter().chain(&p.velocity).chain(&p.rotation).chain(&p.angular_velocity) {
            let _ = write!(s, "{v:.4},");
        }
        let _ = write!(
            s,
            "{:.3},{:.3},{:.4},{:.2},{:.2},{:.2},{},{},{},{},",
            d.v_fwd,
            d.v_lat,
            d.yaw_rate,
            d.heading,
            d.pitch,
            d.roll,
            opt(d.body_slip),
            opt(d.slip_front),
            opt(d.slip_rear),
            opt(d.balance),
        );
        let _ = write!(s, "{:.4},", p.body_height);
        for v in p
            .road_plane
            .iter()
            .chain([&p.road_plane_distance])
            .chain(&p.tire_temp)
            .chain(&p.wheel_rps)
            .chain(&p.tire_radius)
            .chain(&p.suspension_height)
        {
            let _ = write!(s, "{v:.4},");
        }
        let _ = write!(
            s,
            "{:.3},{:.3},{:.3},{:.2},{:.2},{:.4},{},",
            p.fuel_level, p.boost_bar, p.oil_pressure, p.water_temp, p.oil_temp, p.energy_recovery, p.time_of_day_ms
        );
        for b in p.unknown_bytes {
            let _ = write!(s, "{b},");
        }
        for v in p.unknown_floats.iter().chain([&p.unknown_float]) {
            let _ = write!(s, "{v:.4},");
        }
        let _ = write!(s, "{},{},{},{},{}", p.last_lap_ms, p.best_lap_ms, p.rpm_alert_min, p.rpm_alert_max, p.total_laps);
        for r in p.gear_ratios {
            let _ = write!(s, ",{r:.4}");
        }
        s
    }
}

fn now_iso() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

fn read_meta(dir: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&fs::read_to_string(dir.join("session.json")).ok()?).ok()
}

fn write_meta(dir: &Path, meta: &serde_json::Value) {
    let tmp = dir.join("session.json.tmp");
    if fs::write(&tmp, serde_json::to_string_pretty(meta).unwrap_or_default()).is_ok() {
        let _ = fs::rename(tmp, dir.join("session.json"));
    }
}

/// Highest sequence number of the lap files in `dir` (for continued sessions).
fn last_seq(dir: &Path) -> u32 {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.split('_').next()?.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
}

/// Newest session that can be continued after a server restart.
fn resumable(root: &Path, car_code: i32) -> Option<PathBuf> {
    let latest = fs::read_dir(root).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).max()?;
    let meta = read_meta(&latest)?;
    let open = meta.get("ended").is_some_and(|e| e.is_null());
    let same_car = meta.get("car_code").and_then(|c| c.as_i64()) == Some(car_code as i64);
    let recent = fs::read_dir(&latest)
        .ok()?
        .flatten()
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
        .is_some_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < RESTART_RESUME);
    (open && same_car && recent).then_some(latest)
}

fn writer(root: PathBuf, rx: mpsc::Receiver<Msg>, wake: Option<mpsc::Sender<()>>) {
    let notify = || {
        if let Some(w) = &wake {
            let _ = w.send(());
        }
    };
    let mut session: Option<PathBuf> = None;
    let mut meta = serde_json::Value::Null;
    let mut file: Option<BufWriter<File>> = None;
    let mut rows = 0u32;
    let mut seq = 0u32;
    // Files are created with their first row only, so menus (lap −1/0 without driving) don't
    // leave empty files.
    let mut pending_lap: Option<i16> = None;
    for msg in rx {
        match msg {
            Msg::Session { car, car_code, resume } => {
                file = None;
                pending_lap = None;
                let resumed_dir = if resume {
                    session.clone()
                } else if session.is_none() {
                    resumable(&root, car_code)
                } else {
                    None
                };
                if let Some(dir) = resumed_dir {
                    meta = read_meta(&dir).unwrap_or_else(|| serde_json::json!({ "car": car, "car_code": car_code, "started": now_iso() }));
                    meta["ended"] = serde_json::Value::Null;
                    meta["end_reason"] = serde_json::Value::Null;
                    meta["resumed"] = (meta["resumed"].as_i64().unwrap_or(0) + 1).into();
                    seq = last_seq(&dir);
                    write_meta(&dir, &meta);
                    let d = dir.display();
                    println!("{}", tr!("Aufzeichnung fortgesetzt: {d}", "Recording continued: {d}"));
                    session = Some(dir);
                    continue;
                }
                let name: String = car.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
                let dir = root.join(format!("{}_{name}", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")));
                match fs::create_dir_all(&dir) {
                    Ok(()) => {
                        let d = dir.display();
                        println!("{}", tr!("Aufzeichnung: {d}", "Recording: {d}"));
                        meta = serde_json::json!({ "car": car, "car_code": car_code, "started": now_iso(),
                                                   "ended": null, "end_reason": null, "resumed": 0 });
                        write_meta(&dir, &meta);
                        seq = 0;
                        session = Some(dir);
                    }
                    Err(e) => {
                        let d = dir.display();
                        eprintln!("{}", tr!("Aufzeichnung nicht möglich ({d}): {e}", "Recording not possible ({d}): {e}"));
                        session = None;
                    }
                }
            }
            Msg::End { reason } => {
                file = None;
                notify();
                pending_lap = None;
                if let Some(dir) = &session {
                    meta["ended"] = now_iso().into();
                    meta["end_reason"] = reason.into();
                    write_meta(dir, &meta);
                }
            }
            Msg::Lap(lap) => {
                file = None;
                notify();
                pending_lap = Some(lap);
            }
            Msg::Row(row) => {
                if let (None, Some(lap), Some(dir)) = (&file, pending_lap.take(), &session) {
                    seq += 1;
                    let path = dir.join(format!("{seq:03}_lap{lap:02}.csv"));
                    match File::create(&path) {
                        Ok(f) => {
                            let mut w = BufWriter::new(f);
                            let _ = writeln!(w, "{HEADER}");
                            file = Some(w);
                        }
                        Err(e) => {
                            let p = path.display();
                            eprintln!("{}", tr!("Kann {p} nicht anlegen: {e}", "Cannot create {p}: {e}"));
                        }
                    }
                }
                if let Some(w) = file.as_mut() {
                    let _ = writeln!(w, "{row}");
                    rows += 1;
                    if rows.is_multiple_of(60) {
                        let _ = w.flush();
                    }
                }
            }
        }
    }
}
