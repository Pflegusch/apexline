//! Decryption and parsing of the GT7 telemetry packets.
//!
//! After a heartbeat to UDP port 33739, GT7 sends about 60 packets/s to port 33740.
//! The heartbeat letter selects the format: "A" (296 bytes), "B" (316) or "~" (344 bytes, adds
//! steering angle and accelerations). We use "~".
//! The packets are encrypted with Salsa20; the nonce sits (unencrypted) at offset 0x40.
//! Layout after Nenkai/PDTools (SimulatorPacket.cs).

use salsa20::cipher::{KeyIvInit, StreamCipher};
use salsa20::Salsa20;

pub const HEARTBEAT: &[u8] = b"~";
const IV_XOR: u32 = 0x55FA_BB4F;
const MIN_SIZE: usize = 0x128;
const EXT_SIZE: usize = 0x158;
const KEY: &[u8; 32] = b"Simulator Interface Packet GT7 v"; // first 32 bytes of "...GT7 ver 0.0"
const MAGIC: u32 = 0x4737_5330; // "0S7G"

pub mod flags {
    pub const ON_TRACK: u16 = 1 << 0;
    pub const PAUSED: u16 = 1 << 1;
    pub const LOADING: u16 = 1 << 2;
    pub const IN_GEAR: u16 = 1 << 3;
    pub const HAS_TURBO: u16 = 1 << 4;
    pub const REV_LIMITER: u16 = 1 << 5;
    pub const HANDBRAKE: u16 = 1 << 6;
    pub const LIGHTS: u16 = 1 << 7;
    pub const HIGH_BEAM: u16 = 1 << 8;
    pub const ASM_ACTIVE: u16 = 1 << 10;
    pub const TCS_ACTIVE: u16 = 1 << 11;
}

/// Order of all 4-element arrays: FL, FR, RL, RR.
#[derive(Debug, Clone, Default)]
pub struct Packet {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    /// Orientation as a quaternion (x, y, z, w)
    pub rotation: [f32; 4],
    pub angular_velocity: [f32; 3],
    pub body_height: f32,
    pub engine_rpm: f32,
    pub fuel_level: f32,
    pub fuel_capacity: f32,
    pub speed_ms: f32,
    pub boost_bar: f32,
    pub oil_pressure: f32,
    pub water_temp: f32,
    pub oil_temp: f32,
    pub tire_temp: [f32; 4],
    pub packet_id: i32,
    pub current_lap: i16,
    pub total_laps: i16,
    pub best_lap_ms: i32,
    pub last_lap_ms: i32,
    pub time_of_day_ms: i32,
    pub start_position: i16,
    pub cars_in_race: i16,
    pub rpm_alert_min: i16,
    pub rpm_alert_max: i16,
    pub calc_max_speed: i16,
    pub flags: u16,
    pub gear: u8,
    pub suggested_gear: u8,
    pub throttle: u8,
    pub brake: u8,
    pub road_plane: [f32; 3],
    pub road_plane_distance: f32,
    pub wheel_rps: [f32; 4],
    pub tire_radius: [f32; 4],
    pub suspension_height: [f32; 4],
    pub clutch_pedal: f32,
    pub clutch_engagement: f32,
    pub rpm_clutch_gearbox: f32,
    pub transmission_top_speed: f32,
    pub gear_ratios: [f32; 8],
    pub car_code: i32,
    // Only in the extended packet ("B"/"~"):
    /// Steering angle in radians
    pub steering: f32,
    /// Lateral, vertical, longitudinal acceleration
    pub sway: f32,
    pub heave: f32,
    pub surge: f32,
    pub unknown_bytes: [u8; 4],
    pub unknown_floats: [f32; 4],
    pub energy_recovery: f32,
    pub unknown_float: f32,
    pub extended: bool,
}

pub fn decrypt(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < MIN_SIZE {
        return None;
    }
    let iv1 = u32::from_le_bytes(data[0x40..0x44].try_into().ok()?);
    let iv2 = iv1 ^ IV_XOR;
    let mut nonce = [0u8; 8];
    nonce[..4].copy_from_slice(&iv2.to_le_bytes());
    nonce[4..].copy_from_slice(&iv1.to_le_bytes());

    let mut buf = data.to_vec();
    let mut cipher = Salsa20::new(KEY.into(), &nonce.into());
    cipher.apply_keystream(&mut buf);

    (u32::from_le_bytes(buf[0..4].try_into().ok()?) == MAGIC).then_some(buf)
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn f32(&self, o: usize) -> f32 {
        f32::from_le_bytes(self.0[o..o + 4].try_into().unwrap())
    }
    fn i32(&self, o: usize) -> i32 {
        i32::from_le_bytes(self.0[o..o + 4].try_into().unwrap())
    }
    fn i16(&self, o: usize) -> i16 {
        i16::from_le_bytes(self.0[o..o + 2].try_into().unwrap())
    }
    fn arr<const N: usize>(&self, o: usize) -> [f32; N] {
        std::array::from_fn(|i| self.f32(o + 4 * i))
    }
}

pub fn parse(data: &[u8]) -> Option<Packet> {
    let buf = decrypt(data)?;
    let r = Reader(&buf);
    let gears = buf[0x90];
    let mut p = Packet {
        position: r.arr(0x04),
        velocity: r.arr(0x10),
        rotation: r.arr(0x1C),
        angular_velocity: r.arr(0x2C),
        body_height: r.f32(0x38),
        engine_rpm: r.f32(0x3C),
        fuel_level: r.f32(0x44),
        fuel_capacity: r.f32(0x48),
        speed_ms: r.f32(0x4C),
        boost_bar: r.f32(0x50) - 1.0,
        oil_pressure: r.f32(0x54),
        water_temp: r.f32(0x58),
        oil_temp: r.f32(0x5C),
        tire_temp: r.arr(0x60),
        packet_id: r.i32(0x70),
        current_lap: r.i16(0x74),
        total_laps: r.i16(0x76),
        best_lap_ms: r.i32(0x78),
        last_lap_ms: r.i32(0x7C),
        time_of_day_ms: r.i32(0x80),
        start_position: r.i16(0x84),
        cars_in_race: r.i16(0x86),
        rpm_alert_min: r.i16(0x88),
        rpm_alert_max: r.i16(0x8A),
        calc_max_speed: r.i16(0x8C),
        flags: r.i16(0x8E) as u16,
        gear: gears & 0x0F,
        suggested_gear: gears >> 4,
        throttle: buf[0x91],
        brake: buf[0x92],
        road_plane: r.arr(0x94),
        road_plane_distance: r.f32(0xA0),
        wheel_rps: r.arr(0xA4),
        tire_radius: r.arr(0xB4),
        suspension_height: r.arr(0xC4),
        clutch_pedal: r.f32(0xF4),
        clutch_engagement: r.f32(0xF8),
        rpm_clutch_gearbox: r.f32(0xFC),
        transmission_top_speed: r.f32(0x100),
        gear_ratios: r.arr(0x104),
        car_code: r.i32(0x124),
        ..Default::default()
    };
    if buf.len() >= EXT_SIZE {
        p.extended = true;
        p.steering = r.f32(0x128);
        p.sway = r.f32(0x130);
        p.heave = r.f32(0x134);
        p.surge = r.f32(0x138);
        p.unknown_bytes = buf[0x13C..0x140].try_into().unwrap();
        p.unknown_floats = r.arr(0x140);
        p.energy_recovery = r.f32(0x150);
        p.unknown_float = r.f32(0x154);
    }
    Some(p)
}
