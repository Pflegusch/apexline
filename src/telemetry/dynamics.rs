//! Vehicle dynamics: velocity in the car frame, body slip angle, tyre slip angles and from them
//! the balance (understeer/oversteer).
//!
//! Balance = |slip angle front| − |slip angle rear| (single-track model):
//! positive = the front axle slides more (understeer), negative = the rear axle (oversteer).
//! A clearly sliding rear axle always counts as oversteer – even when the driver steers into the
//! slide and thereby creates a large slip angle at the front as well.

use super::packet::Packet;

/// GT7 sends no wheelbase; the centre of gravity is assumed in the middle.
const WHEELBASE: f32 = 2.7;
const CG_TO_FRONT: f32 = WHEELBASE / 2.0;
const CG_TO_REAR: f32 = WHEELBASE / 2.0;
/// Below this speed (m/s) slip angles are meaningless.
const MIN_SPEED: f32 = 8.0;

/// From this rear slip angle (degrees) on, the rear axle is past the limit of grip.
const REAR_SLIDE: f32 = 5.0;

// Axis/sign conventions of GT7, determined from recorded laps:
// local axes x = right, y = up, −z = forward; yaw rate (about y) positive = left turn;
// steering = steering wheel angle in radians (±π), positive = left.
const FWD_SIGN: f32 = -1.0;
const LEFT_SIGN: f32 = -1.0;

#[derive(Clone, Copy, Default)]
pub struct Dynamics {
    /// Velocity in the car frame (m/s): forward, lateral (+ left)
    pub v_fwd: f32,
    pub v_lat: f32,
    /// Yaw rate (rad/s, + left)
    pub yaw_rate: f32,
    /// Heading, pitch, roll (degrees)
    pub heading: f32,
    pub pitch: f32,
    pub roll: f32,
    /// Body slip and tyre slip angles (degrees), `None` at too low a speed
    pub body_slip: Option<f32>,
    pub slip_front: Option<f32>,
    pub slip_rear: Option<f32>,
    pub balance: Option<f32>,
}

/// Rotates `v` by the quaternion `q` (x, y, z, w); `inverse` rotates the other way.
pub fn rotate(q: [f32; 4], v: [f32; 3], inverse: bool) -> [f32; 3] {
    let s = if inverse { -1.0 } else { 1.0 };
    let (ux, uy, uz, w) = (q[0] * s, q[1] * s, q[2] * s, q[3]);
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let u = [ux, uy, uz];
    let t = cross(u, v).map(|c| 2.0 * c);
    let ut = cross(u, t);
    [v[0] + w * t[0] + ut[0], v[1] + w * t[1] + ut[1], v[2] + w * t[2] + ut[2]]
}

/// `steer_ratio`: steering wheel angle / wheel angle (depends on the car, see [`SteerCalibration`]).
pub fn compute(p: &Packet, steer_ratio: f32) -> Dynamics {
    let local = rotate(p.rotation, p.velocity, true);
    let v_fwd = local[2] * FWD_SIGN;
    let v_lat = local[0] * LEFT_SIGN;
    let yaw_rate = p.angular_velocity[1];

    let fwd = rotate(p.rotation, [0.0, 0.0, FWD_SIGN], false);
    let right = rotate(p.rotation, [1.0, 0.0, 0.0], false);
    let mut d = Dynamics {
        v_fwd,
        v_lat,
        yaw_rate,
        heading: fwd[0].atan2(fwd[2]).to_degrees(),
        pitch: fwd[1].clamp(-1.0, 1.0).asin().to_degrees(),
        roll: right[1].clamp(-1.0, 1.0).asin().to_degrees(),
        ..Default::default()
    };

    if v_fwd > MIN_SPEED {
        let wheel_angle = p.steering / steer_ratio;
        let front = (wheel_angle - ((v_lat + CG_TO_FRONT * yaw_rate) / v_fwd).atan()).to_degrees();
        let rear = (-((v_lat - CG_TO_REAR * yaw_rate) / v_fwd).atan()).to_degrees();
        let mut balance = front.abs() - rear.abs();
        if rear.abs() > REAR_SLIDE {
            balance = balance.min(-(rear.abs() - REAR_SLIDE + 1.5));
        }
        d.body_slip = Some((v_lat / v_fwd).atan().to_degrees());
        d.slip_front = Some(front);
        d.slip_rear = Some(rear);
        d.balance = Some(balance);
    }
    d
}

/// Learns the steering ratio of the current car: at moderate speed and little lateral acceleration
/// the tyres roll almost without slip, then wheel angle ≈ wheelbase · yaw rate / speed.
pub struct SteerCalibration {
    samples: std::collections::VecDeque<f32>,
    pub ratio: f32,
}

impl Default for SteerCalibration {
    fn default() -> Self {
        SteerCalibration { samples: Default::default(), ratio: 20.0 }
    }
}

impl SteerCalibration {
    const MAX_SAMPLES: usize = 900;
    const MIN_SAMPLES: usize = 60;

    pub fn update(&mut self, p: &Packet, d: &Dynamics) {
        let v = d.v_fwd;
        let r = d.yaw_rate;
        let gentle = (8.0..30.0).contains(&v) && p.sway.abs() < 3.0 && d.body_slip.is_some_and(|b| b.abs() < 2.0);
        if !gentle || !(0.05..2.5).contains(&p.steering.abs()) || r.abs() < 0.03 || r.signum() != p.steering.signum() {
            return;
        }
        let k = p.steering / (WHEELBASE * r / v);
        if !(5.0..60.0).contains(&k) {
            return;
        }
        self.samples.push_back(k);
        if self.samples.len() > Self::MAX_SAMPLES {
            self.samples.pop_front();
        }
        if self.samples.len() >= Self::MIN_SAMPLES && self.samples.len().is_multiple_of(30) {
            let mut sorted: Vec<f32> = self.samples.iter().copied().collect();
            sorted.sort_by(f32::total_cmp);
            self.ratio = sorted[sorted.len() / 2];
        }
    }
}
