//! Synthetic packets for trying the dashboard without a PS5 (`apexline demo`).

use crate::session::tracker::Tracker;
use crate::telemetry::packet::{flags, Packet};
use crate::web::Publisher;
use std::time::Duration;

/// Feeds simulated packets at 60 per second until the program ends.
pub async fn run(mut tracker: Tracker, mut out: Publisher) {
    let mut demo = Demo::new();
    let mut tick = tokio::time::interval(Duration::from_micros(16_667));
    loop {
        tick.tick().await;
        let snap = tracker.update(&demo.next()).clone();
        out.publish(&tracker, &snap);
    }
}

pub struct Demo {
    tick: i32,
    fuel: f32,
    lap: i16,
    lap_start: i32,
    last: i32,
    best: i32,
}

impl Demo {
    pub fn new() -> Self {
        Demo { tick: 0, fuel: 65.0, lap: 1, lap_start: 0, last: -1, best: -1 }
    }

    pub fn next(&mut self) -> Packet {
        self.tick += 1;
        let t = self.tick as f32 / 60.0;
        let lap_len = 60 * 45 + (self.lap as i32 % 3) * 40; // ~45 s laps
        if self.tick - self.lap_start >= lap_len {
            let ms = (self.tick - self.lap_start) * 1000 / 60;
            self.last = ms;
            self.best = if self.best < 0 { ms } else { self.best.min(ms) };
            self.lap += 1;
            self.lap_start = self.tick;
        }
        self.fuel = (self.fuel - 0.004).max(0.0);

        // Alternating straights and corners
        let phase = (t * 0.35).sin();
        let speed_kmh = 150.0 + 110.0 * phase;
        let braking = (t * 0.35).cos() < -0.6 && phase > 0.0;
        let gear = ((speed_kmh / 45.0) as u8 + 1).clamp(1, 6);
        let rpm = 3500.0 + (speed_kmh % 45.0) / 45.0 * 4800.0;
        let heat = |base: f32, off: f32| base + 18.0 * (t * 0.05 + off).sin() + 4.0 * phase;

        let speed = speed_kmh / 3.6;
        // Made-up oval track, one round per lap
        let th = std::f32::consts::TAU * (self.tick - self.lap_start) as f32 / lap_len as f32;
        let (rx, rz) = (420.0, 170.0);
        let (dx, dz) = (-rx * th.sin(), rz * th.cos());
        let yaw = dx.atan2(dz) + std::f32::consts::PI; // GT7: forward = local −z axis
        let norm = (dx * dx + dz * dz).sqrt();
        let (hs, hc) = ((yaw / 2.0).sin(), (yaw / 2.0).cos());
        let radius = 0.33;
        let slip = if braking { 0.82 } else { 1.0 };
        let rps = speed / radius * slip;

        Packet {
            engine_rpm: rpm,
            fuel_level: self.fuel,
            fuel_capacity: 65.0,
            speed_ms: speed,
            boost_bar: 0.0,
            oil_pressure: 4.5,
            water_temp: 88.0,
            oil_temp: 102.0,
            tire_temp: [heat(78.0, 0.0), heat(84.0, 1.0), heat(70.0, 2.0), heat(96.0, 3.0)],
            packet_id: self.tick,
            current_lap: self.lap,
            total_laps: 10,
            best_lap_ms: self.best,
            last_lap_ms: self.last,
            start_position: 3,
            cars_in_race: 16,
            rpm_alert_min: 7600,
            rpm_alert_max: 8300,
            flags: flags::ON_TRACK | if braking { 0 } else { flags::TCS_ACTIVE * ((t as i32 % 7 == 0) as u16) },
            gear,
            suggested_gear: if braking { gear.saturating_sub(1).max(1) } else { 15 },
            throttle: if braking { 0 } else { (180.0 + 75.0 * phase) as u8 },
            brake: if braking { 220 } else { 0 },
            wheel_rps: [rps, rps, speed / radius, speed / radius],
            tire_radius: [radius; 4],
            suspension_height: [0.08; 4],
            car_code: 3475,
            position: [rx * th.cos(), 0.0, rz * th.sin() + 40.0 * (2.0 * th).sin()],
            velocity: [speed * dx / norm, 0.0, speed * dz / norm],
            rotation: [0.0, hs, 0.0, hc],
            angular_velocity: [0.0, 0.25 * phase, 0.0],
            steering: 0.08 * phase,
            sway: 9.0 * phase,
            surge: if braking { -12.0 } else { 3.0 },
            extended: true,
            ..Default::default()
        }
    }
}
