//! Derived state: lap times, fuel per lap, tyre slip, vehicle dynamics, track map, measurements.
//! Built from the packet stream and sent to the clients as a JSON snapshot.
//!
//! The tracker also decides the driving sessions (one session = one car on one track): a session
//! starts as soon as the car is driven and ends after a long pause / menu, when the track is left,
//! when no data arrives any more or when the car changes. Driving on with the same car on the same
//! track afterwards continues the session.

use super::recorder::{Msg, Recorder};
use super::track_map::Track;
use crate::perf;
use crate::telemetry::packet::{flags, Packet};
use crate::telemetry::{cars, dynamics, Link};
use crate::tr;
use serde::Serialize;
use std::time::{Duration, Instant};

const TICKS_PER_SEC: f64 = 60.0;
/// Paused this long (pause menu, back in the main menu) = session over
const SESSION_PAUSE_END: Duration = Duration::from_secs(180);
/// Off the track or without any data this long = session over
const SESSION_OFF_END: Duration = Duration::from_secs(60);
/// Driving on this close to the known track (m) continues the session
const RESUME_DISTANCE: f32 = 50.0;
const FUEL_HISTORY: usize = 5;
const LAP_HISTORY: usize = 8;
/// Smoothing of the balance display (packets): ~0.25 s
const BALANCE_SMOOTHING: f32 = 15.0;

#[derive(Serialize, Default, Clone)]
pub struct Snapshot {
    pub live: bool,
    pub on_track: bool,
    pub paused: bool,
    pub loading: bool,

    pub speed_kmh: f32,
    pub rpm: f32,
    pub rpm_alert_min: i16,
    pub rpm_alert_max: i16,
    /// 0 = reverse, 15 = neutral (as GT7 sends it)
    pub gear: u8,
    pub suggested_gear: Option<u8>,
    pub throttle: f32,
    pub brake: f32,

    /// FL, FR, RL, RR
    pub tire_temp: [f32; 4],
    /// Wheel surface speed / car speed (1.0 = no slip), `null` when standing
    pub tire_slip: [Option<f32>; 4],
    pub wheel_speed_kmh: [f32; 4],
    pub tire_radius: [f32; 4],
    pub suspension: [f32; 4],

    pub fuel: f32,
    pub fuel_capacity: f32,
    pub fuel_per_lap: Option<f32>,
    pub fuel_laps_left: Option<f32>,

    pub lap: i16,
    pub total_laps: i16,
    pub current_lap_ms: Option<i64>,
    pub last_lap_ms: Option<i32>,
    pub best_lap_ms: Option<i32>,
    pub live_delta_ms: Option<i64>,
    pub laps: Vec<i32>,

    pub position: Option<i16>,
    pub cars: Option<i16>,

    pub water_temp: f32,
    pub oil_temp: f32,
    pub oil_pressure: f32,
    pub boost_bar: Option<f32>,
    pub tcs: bool,
    pub asm: bool,
    pub rev_limiter: bool,
    pub handbrake: bool,
    pub in_gear: bool,
    pub lights: bool,
    pub high_beam: bool,
    pub flags: u16,

    pub car_code: i32,
    pub car_name: Option<&'static str>,
    pub calc_max_speed: i16,
    pub transmission_top_speed_kmh: f32,
    pub gear_ratios: Vec<f32>,
    pub clutch_pedal: f32,
    pub clutch_engagement: f32,
    pub rpm_clutch_gearbox: f32,
    pub energy_recovery: f32,
    pub time_of_day_ms: i32,

    /// World position (m) and attitude (degrees)
    pub pos: [f32; 3],
    pub heading: f32,
    pub pitch: f32,
    pub roll: f32,
    pub body_height: f32,
    pub road_plane_distance: f32,
    pub map_version: u32,

    pub steering_deg: f32,
    pub yaw_rate_deg: f32,
    pub v_lat_kmh: f32,
    pub body_slip: Option<f32>,
    pub slip_front: Option<f32>,
    pub slip_rear: Option<f32>,
    /// Smoothed, degrees: > 0 understeer, < 0 oversteer
    pub balance: Option<f32>,
    /// Accelerations as GT7 sends them (lateral, vertical, longitudinal)
    pub steer_ratio: f32,
    pub sway: f32,
    pub heave: f32,
    pub surge: f32,
    pub extended: bool,
    pub recording: bool,

    /// Measurements (0–100, braking distances), page 4
    pub perf: perf::Live,
    /// Connection to the PS5 (search state while there is no data)
    pub link: Link,
}

pub struct Tracker {
    snap: Snapshot,
    lap: i16,
    lap_start_tick: Option<i32>,
    last_tick: Option<i32>,
    fuel_at_lap_start: Option<f32>,
    fuel_used: Vec<f32>,
    balance: Option<f32>,
    steer_cal: dynamics::SteerCalibration,
    track: Track,
    recorder: Option<Recorder>,
    session_car: Option<i32>,
    session_open: bool,
    paused_since: Option<Instant>,
    off_track_since: Option<Instant>,
    last_packet: Option<Instant>,
    perf: perf::Detector,
    perf_store: Option<perf::Store>,
}

impl Tracker {
    pub fn new(recorder: Option<Recorder>, perf_store: Option<perf::Store>) -> Self {
        Tracker {
            snap: Snapshot::default(),
            lap: 0,
            lap_start_tick: None,
            last_tick: None,
            fuel_at_lap_start: None,
            fuel_used: Vec::new(),
            balance: None,
            steer_cal: Default::default(),
            track: Track::default(),
            recorder,
            session_car: None,
            session_open: false,
            paused_since: None,
            off_track_since: None,
            last_packet: None,
            perf: perf::Detector::default(),
            perf_store,
        }
    }

    pub fn map_json(&self) -> String {
        serde_json::to_string(&self.track.map()).unwrap_or_default()
    }

    pub fn update(&mut self, p: &Packet) -> &Snapshot {
        self.update_at(p, Instant::now())
    }

    fn update_at(&mut self, p: &Packet, now: Instant) -> &Snapshot {
        let on_track = p.flags & flags::ON_TRACK != 0;
        let paused = p.flags & flags::PAUSED != 0;

        self.last_packet = Some(now);
        if paused {
            self.paused_since.get_or_insert(now);
        } else {
            self.paused_since = None;
        }
        if on_track {
            self.off_track_since = None;
        } else {
            self.off_track_since.get_or_insert(now);
        }

        // Different car: end the session right away and drop everything
        let car_changed = self.session_car.is_some_and(|c| c != p.car_code);
        if car_changed {
            if self.session_open {
                self.end_session("car_change");
            }
            self.reset(true);
        } else if p.current_lap < self.lap || (p.current_lap == 0 && self.lap != 0) {
            // Race/time trial restarted: new lap file, map and best lap stay
            self.reset(false);
            if let (Some(r), true) = (&self.recorder, self.session_open) {
                r.send(Msg::Lap(p.current_lap));
            }
        }
        if !self.session_open && on_track && !paused {
            self.start_session(p);
        }

        if p.current_lap != self.lap {
            let crossed_line = self.last_tick.is_some();
            let finished = (self.lap > 0 && p.last_lap_ms > 0).then_some(p.last_lap_ms);
            if let Some(ms) = finished {
                self.snap.laps.push(ms);
                if self.snap.laps.len() > LAP_HISTORY {
                    self.snap.laps.remove(0);
                }
            }
            if let Some(start) = self.fuel_at_lap_start {
                let used = start - p.fuel_level;
                // Refuelling in the pit gives negative consumption -> ignore
                if used > 0.0 {
                    self.fuel_used.push(used);
                    if self.fuel_used.len() > FUEL_HISTORY {
                        self.fuel_used.remove(0);
                    }
                }
            }
            self.track.lap_started(finished, crossed_line && p.current_lap > 0);
            if let (Some(r), true) = (&self.recorder, self.session_open) {
                r.send(Msg::Lap(p.current_lap));
            }
            self.lap = p.current_lap;
            self.lap_start_tick = (p.current_lap > 0).then_some(p.packet_id);
            self.fuel_at_lap_start = (p.current_lap > 0).then_some(p.fuel_level);
        }

        // Time spent paused must not count towards the lap time
        if let (true, Some(start), Some(last)) = (paused, self.lap_start_tick.as_mut(), self.last_tick) {
            *start += p.packet_id - last;
        }
        self.last_tick = Some(p.packet_id);

        let lap_ms = self.lap_start_tick.map(|t| ((p.packet_id - t) as f64 / TICKS_PER_SEC * 1000.0) as i64);
        let dyn_ = dynamics::compute(p, self.steer_cal.ratio);
        self.steer_cal.update(p, &dyn_);
        if let Some(run) = self.perf.update(perf::Sample::from_packet(p)) {
            self.perf_finished(run, p.car_code);
        }
        let (x, z) = (p.position[0], p.position[2]);
        let mut delta = None;
        if on_track && !paused {
            // The map grows even before the line is crossed the first time (lap 0)
            self.track.add(x, z, lap_ms.unwrap_or(0));
            if let Some(ms) = lap_ms {
                delta = self.track.delta(x, z, ms);
            }
            if let (Some(r), true) = (&self.recorder, self.session_open) {
                r.send(Msg::Row(Recorder::row(p, &dyn_, lap_ms, delta)));
            }
        }
        self.balance = match (dyn_.balance, self.balance) {
            (Some(b), Some(prev)) => Some(prev + (b - prev) / BALANCE_SMOOTHING),
            (b, _) => b,
        };

        let speed = p.speed_ms;
        let mut slip = [None; 4];
        let mut wheel_speed = [0.0; 4];
        for i in 0..4 {
            let v = p.wheel_rps[i].abs() * p.tire_radius[i];
            wheel_speed[i] = v * 3.6;
            if speed > 2.0 {
                slip[i] = Some(v / speed);
            }
        }

        let fuel_per_lap = (!self.fuel_used.is_empty()).then(|| self.fuel_used.iter().sum::<f32>() / self.fuel_used.len() as f32);

        let s = &mut self.snap;
        s.live = true;
        s.on_track = on_track;
        s.paused = paused;
        s.loading = p.flags & flags::LOADING != 0;
        s.speed_kmh = speed * 3.6;
        s.rpm = p.engine_rpm;
        s.rpm_alert_min = p.rpm_alert_min;
        s.rpm_alert_max = p.rpm_alert_max;
        s.gear = p.gear;
        s.suggested_gear = (p.suggested_gear != 15).then_some(p.suggested_gear);
        s.throttle = p.throttle as f32 / 2.55;
        s.brake = p.brake as f32 / 2.55;
        s.tire_temp = p.tire_temp;
        s.tire_slip = slip;
        s.wheel_speed_kmh = wheel_speed;
        s.tire_radius = p.tire_radius;
        s.suspension = p.suspension_height;
        s.fuel = p.fuel_level;
        s.fuel_capacity = p.fuel_capacity;
        s.fuel_per_lap = fuel_per_lap;
        s.fuel_laps_left = fuel_per_lap.filter(|f| *f > 0.0).map(|f| p.fuel_level / f);
        s.lap = p.current_lap;
        s.total_laps = p.total_laps;
        s.current_lap_ms = lap_ms;
        s.last_lap_ms = (p.last_lap_ms > 0).then_some(p.last_lap_ms);
        s.best_lap_ms = (p.best_lap_ms > 0).then_some(p.best_lap_ms);
        s.live_delta_ms = delta;
        s.position = (p.start_position > 0).then_some(p.start_position);
        s.cars = (p.cars_in_race > 0).then_some(p.cars_in_race);
        s.water_temp = p.water_temp;
        s.oil_temp = p.oil_temp;
        s.oil_pressure = p.oil_pressure;
        s.boost_bar = (p.flags & flags::HAS_TURBO != 0).then_some(p.boost_bar);
        s.tcs = p.flags & flags::TCS_ACTIVE != 0;
        s.asm = p.flags & flags::ASM_ACTIVE != 0;
        s.rev_limiter = p.flags & flags::REV_LIMITER != 0;
        s.handbrake = p.flags & flags::HANDBRAKE != 0;
        s.in_gear = p.flags & flags::IN_GEAR != 0;
        s.lights = p.flags & flags::LIGHTS != 0;
        s.high_beam = p.flags & flags::HIGH_BEAM != 0;
        s.flags = p.flags;

        s.car_code = p.car_code;
        s.car_name = cars::name(p.car_code);
        s.calc_max_speed = p.calc_max_speed;
        s.transmission_top_speed_kmh = p.transmission_top_speed * 3.6;
        s.gear_ratios = p.gear_ratios.iter().copied().filter(|r| *r > 0.0).collect();
        s.clutch_pedal = p.clutch_pedal;
        s.clutch_engagement = p.clutch_engagement;
        s.rpm_clutch_gearbox = p.rpm_clutch_gearbox;
        s.energy_recovery = p.energy_recovery;
        s.time_of_day_ms = p.time_of_day_ms;

        s.pos = p.position;
        s.heading = dyn_.heading;
        s.pitch = dyn_.pitch;
        s.roll = dyn_.roll;
        s.body_height = p.body_height;
        s.road_plane_distance = p.road_plane_distance;
        s.map_version = self.track.map_version;

        s.steering_deg = p.steering.to_degrees();
        s.yaw_rate_deg = dyn_.yaw_rate.to_degrees();
        s.v_lat_kmh = dyn_.v_lat * 3.6;
        s.body_slip = dyn_.body_slip;
        s.slip_front = dyn_.slip_front;
        s.slip_rear = dyn_.slip_rear;
        s.balance = self.balance;
        s.steer_ratio = self.steer_cal.ratio;
        s.sway = p.sway;
        s.heave = p.heave;
        s.surge = p.surge;
        s.extended = p.extended;
        s.recording = self.recorder.is_some() && self.session_open;
        s.perf.clone_from(&self.perf.live);
        s.perf.saved = self.perf_store.as_ref().map_or(0, perf::Store::saved);
        s
    }

    fn perf_finished(&mut self, mut run: perf::Run, car_code: i32) {
        let car = cars::name(car_code).map_or_else(|| format!("car{car_code}"), str::to_string);
        let start = chrono::Local::now() - chrono::TimeDelta::milliseconds((run.duration_s * 1000.0) as i64);
        run.stamp(car_code, &car, start);
        let h = perf::cli::headline(&run);
        println!("{}", tr!("Messung: {h}", "Measurement: {h}"));
        if let Some(store) = &self.perf_store {
            store.save_in_background(run);
        }
    }

    pub fn set_link(&mut self, link: Link) {
        self.snap.link = link;
    }

    /// No packets any more (PS5 off, menu, network gone).
    pub fn mark_offline(&mut self) -> &Snapshot {
        self.snap.live = false;
        &self.snap
    }

    /// Called every second: ends the session after a long pause, when the track was left or when
    /// no data arrives any more.
    pub fn tick(&mut self) {
        self.tick_at(Instant::now());
    }

    fn tick_at(&mut self, now: Instant) {
        if !self.session_open {
            return;
        }
        let since = |t: Option<Instant>, limit: Duration| t.is_some_and(|t| now.duration_since(t) >= limit);
        let reason = if self.last_packet.is_none_or(|t| now.duration_since(t) >= SESSION_OFF_END) {
            Some("no_data")
        } else if since(self.paused_since, SESSION_PAUSE_END) {
            Some("pause")
        } else if since(self.off_track_since, SESSION_OFF_END) {
            Some("off_track")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.end_session(reason);
        }
    }

    /// New session, or the last one continued (same car, position on the known track).
    fn start_session(&mut self, p: &Packet) {
        let resume = self.session_car == Some(p.car_code) && self.track.is_near(p.position[0], p.position[2], RESUME_DISTANCE);
        if !resume {
            self.reset(true);
        }
        self.session_car = Some(p.car_code);
        self.session_open = true;
        if let Some(r) = &self.recorder {
            let car = cars::name(p.car_code).map_or_else(|| format!("car{}", p.car_code), str::to_string);
            r.send(Msg::Session { car, car_code: p.car_code, resume });
            // On a lap change in the same packet, the lap change block creates the file
            if p.current_lap == self.lap {
                r.send(Msg::Lap(p.current_lap));
            }
        }
        let car = cars::name(p.car_code).unwrap_or("?");
        println!(
            "{}",
            if resume {
                tr!("Session fortgesetzt ({car})", "Session continued ({car})")
            } else {
                tr!("Session gestartet ({car})", "Session started ({car})")
            }
        );
    }

    fn end_session(&mut self, reason: &'static str) {
        self.session_open = false;
        if let Some(r) = &self.recorder {
            r.send(Msg::End { reason });
        }
        let why = match reason {
            "pause" => tr!("Pause oder Menü", "pause or menu"),
            "off_track" => tr!("Strecke verlassen", "left the track"),
            "no_data" => tr!("keine Daten mehr", "no more data"),
            "car_change" => tr!("Autowechsel", "car changed"),
            other => other.to_string(),
        };
        println!("{}", tr!("Session beendet: {why}", "Session ended: {why}"));
    }

    /// Restart: `full = false` (race/time trial restarted) keeps map, best lap and steering ratio;
    /// `full = true` (new car / new track) drops everything.
    fn reset(&mut self, full: bool) {
        let mut track = std::mem::take(&mut self.track);
        if full {
            track.reset();
        } else {
            track.lap_started(None, false);
        }
        let session_car = if full { None } else { self.session_car };
        let steer_cal = if full { Default::default() } else { std::mem::take(&mut self.steer_cal) };
        let snap = Snapshot { link: std::mem::take(&mut self.snap.link), ..Default::default() };
        *self = Tracker {
            snap,
            track,
            session_car,
            steer_cal,
            session_open: self.session_open,
            paused_since: self.paused_since,
            off_track_since: self.off_track_since,
            last_packet: self.last_packet,
            ..Tracker::new(self.recorder.take(), self.perf_store.take())
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn packet(car: i32, flags: u16, x: f32) -> Packet {
        Packet { car_code: car, flags, current_lap: 1, position: [x, 0.0, 0.0], speed_ms: 30.0, packet_id: 1, ..Default::default() }
    }

    fn sessions(root: &Path) -> Vec<(PathBuf, serde_json::Value)> {
        std::thread::sleep(Duration::from_millis(300)); // the recorder writes asynchronously
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(root).unwrap().flatten().map(|e| e.path()).collect();
        dirs.sort();
        dirs.into_iter()
            .map(|d| {
                let meta = serde_json::from_str(&std::fs::read_to_string(d.join("session.json")).unwrap()).unwrap();
                (d, meta)
            })
            .collect()
    }

    /// Drives `secs` seconds of packets (60/s) with the given flags.
    fn drive(t: &mut Tracker, car: i32, flags: u16, start: Instant, secs: u64, x: &mut f32, id: &mut i32) -> Instant {
        let mut now = start;
        for _ in 0..secs * 60 {
            *id += 1;
            if flags & flags::PAUSED == 0 {
                *x += 0.5; // moving, so the map grows
            }
            let mut p = packet(car, flags, *x);
            p.packet_id = *id;
            t.update_at(&p, now);
            now += Duration::from_micros(16_667);
            if *id % 60 == 0 {
                t.tick_at(now);
            }
        }
        now
    }

    #[test]
    fn session_lifecycle() {
        let root = std::env::temp_dir().join(format!("apexline-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut t = Tracker::new(Some(Recorder::start(root.clone(), None)), None);
        let (mut x, mut id) = (0.0, 0);
        let on = flags::ON_TRACK;

        // Driving starts a session
        let now = drive(&mut t, 3475, on, Instant::now(), 5, &mut x, &mut id);
        let s = sessions(&root);
        assert_eq!(s.len(), 1);
        assert!(s[0].1["ended"].is_null());

        // A short pause ends nothing, 3 min pause/menu does
        let now = drive(&mut t, 3475, on | flags::PAUSED, now, 60, &mut x, &mut id);
        assert!(sessions(&root)[0].1["ended"].is_null());
        let now = drive(&mut t, 3475, on | flags::PAUSED, now, 125, &mut x, &mut id);
        let s = sessions(&root);
        assert_eq!(s[0].1["end_reason"], "pause");

        // Driving on with the same car on the same track continues
        let now = drive(&mut t, 3475, on, now, 2, &mut x, &mut id);
        let s = sessions(&root);
        assert_eq!(s.len(), 1);
        assert!(s[0].1["ended"].is_null());
        assert_eq!(s[0].1["resumed"], 1);

        // Changing the car ends right away and starts a new session
        std::thread::sleep(Duration::from_millis(1100)); // new folder name (seconds)
        let now = drive(&mut t, 3350, on, now, 2, &mut x, &mut id);
        let s = sessions(&root);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].1["end_reason"], "car_change");
        assert!(s[1].1["ended"].is_null());

        // Same car but a different track (far away) after the menu: new session
        let now = drive(&mut t, 3350, 0, now, 61, &mut x, &mut id);
        assert_eq!(sessions(&root)[1].1["end_reason"], "off_track");
        std::thread::sleep(Duration::from_millis(1100));
        x += 5000.0;
        let _ = drive(&mut t, 3350, on, now, 2, &mut x, &mut id);
        let s = sessions(&root);
        assert_eq!(s.len(), 3);

        // Server restart (without an end) with the same car continues the open session
        drop(t);
        let mut t2 = Tracker::new(Some(Recorder::start(root.clone(), None)), None);
        let _ = drive(&mut t2, 3350, on, Instant::now(), 2, &mut x, &mut id);
        let s = sessions(&root);
        assert_eq!(s.len(), 3);
        assert_eq!(s[2].1["resumed"], 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
