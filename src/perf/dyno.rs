//! Power band and best shift points from an acceleration run – without knowing the car's mass.
//!
//! At full throttle the power that accelerates the car is P = m · a · v. Per kg (W/kg) this needs
//! no mass: it gives the shape of the power curve over rpm, the power-to-weight ratio and – the
//! useful part for driving – the best upshift points: stay in a gear as long as it delivers more
//! power at the current speed than the next gear would after the shift, i.e. shift where
//! P(rpm) = P(rpm · ratio(g+1) / ratio(g)). It is net power at the wheels (drag, rolling
//! resistance and drivetrain inertia are not added back), which is what counts for accelerating.

use super::{round, Pt};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Half window (samples) for the acceleration: ±0.1 s
const HALF: usize = 6;
const BIN_RPM: f32 = 250.0;
const MIN_PER_BIN: usize = 3;
const FULL_THROTTLE: u8 = 250;
const MIN_KMH: f32 = 15.0;
/// Engine speed / road speed may deviate this much from the gear's typical ratio; more means the
/// clutch slips or the wheels spin, so the engine power does not reach the road.
const RATIO_TOL: f32 = 0.04;
/// Samples needed to trust a measured gear ratio.
const MIN_RATIO_SAMPLES: usize = 10;
const SHIFT_STEP_RPM: f32 = 25.0;
/// Samples before a gear change in which the driven shift rpm is looked for (0.2 s)
const SHIFT_WINDOW: usize = 12;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Dyno {
    /// Best value of all gears per rpm bin, lightly smoothed: [rpm, W/kg]
    pub curve: Vec<[f32; 2]>,
    /// Per gear: [gear, rpm, W/kg]
    pub gears: Vec<[f32; 3]>,
    /// Peak of `curve`: [rpm, W/kg]
    pub peak: [f32; 2],
    /// Engine speed per road speed of each gear: [gear, rpm per km/h]
    #[serde(default)]
    pub ratios: Vec<[f32; 2]>,
    /// Highest engine speed reached at full throttle (≈ rev limit)
    #[serde(default)]
    pub max_rpm: f32,
    #[serde(default)]
    pub shifts: Vec<Shift>,
}

/// Best upshift from `gear` to the next one.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Shift {
    pub gear: u8,
    pub rpm: f32,
    /// Speed at that point (km/h)
    pub kmh: f32,
    /// The lower gear pulls harder up to the highest rpm measured: shift as late as possible
    pub late: bool,
    /// Where this run shifted (median rpm of its upshifts at full throttle)
    pub actual: Option<f32>,
}

fn median(v: &mut [f32]) -> f32 {
    v.sort_by(f32::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn percentile(v: &mut [f32], p: f32) -> f32 {
    v.sort_by(f32::total_cmp);
    v[((v.len() - 1) as f32 * p).round() as usize]
}

/// Linear interpolation in a curve sorted by x; `None` outside it.
fn at(curve: &[[f32; 2]], x: f32) -> Option<f32> {
    let i = curve.windows(2).position(|w| w[0][0] <= x && x <= w[1][0])?;
    let (a, b) = (curve[i], curve[i + 1]);
    Some(if b[0] > a[0] { a[1] + (b[1] - a[1]) * (x - a[0]) / (b[0] - a[0]) } else { a[1] })
}

/// rpm per km/h of each gear. Measured from the run (low percentile: wheelspin only raises it);
/// with the transmission ratios from the packets, all gears follow from the best measured one.
fn gear_ratios(pts: &[Pt], full: impl Fn(&Pt) -> bool, transmission: [f32; 8]) -> BTreeMap<u8, f32> {
    let mut samples: BTreeMap<u8, Vec<f32>> = BTreeMap::new();
    for p in pts.iter().filter(|p| full(p)) {
        samples.entry(p.gear).or_default().push(p.rpm / p.v);
    }
    let counts: BTreeMap<u8, usize> = samples.iter().map(|(g, v)| (*g, v.len())).collect();
    let measured: BTreeMap<u8, f32> =
        samples.into_iter().filter(|(_, v)| v.len() >= MIN_RATIO_SAMPLES).map(|(g, mut v)| (g, percentile(&mut v, 0.2))).collect();
    let tr = |g: u8| transmission.get(g as usize - 1).copied().filter(|r| *r > 0.0);
    // Reference: the measured gear with the most samples that has a transmission ratio
    let reference = measured.keys().filter(|g| tr(**g).is_some()).max_by_key(|g| counts[*g]);
    match reference {
        Some(&rg) => {
            let (base, r0) = (measured[&rg], tr(rg).unwrap_or(1.0));
            (1..=8u8).filter_map(|g| tr(g).map(|r| (g, base * r / r0))).collect()
        }
        None => measured,
    }
}

pub(super) fn compute(pts: &[Pt], transmission: [f32; 8]) -> Option<Dyno> {
    let full = |p: &Pt| p.thr >= FULL_THROTTLE && p.brk == 0 && (1..=10).contains(&p.gear) && p.v >= MIN_KMH && p.rpm > 500.0;

    // Typical rpm per km/h of each gear for the slip filter
    let mut by_gear: BTreeMap<u8, Vec<f32>> = BTreeMap::new();
    for p in pts.iter().filter(|p| full(p)) {
        by_gear.entry(p.gear).or_default().push(p.rpm / p.v);
    }
    let typical: BTreeMap<u8, f32> = by_gear.into_iter().map(|(g, mut r)| (g, median(&mut r))).collect();

    let mut bins: BTreeMap<(u8, i32), Vec<f32>> = BTreeMap::new();
    for i in HALF..pts.len().saturating_sub(HALF) {
        let (p, a, b) = (&pts[i], &pts[i - HALF], &pts[i + HALF]);
        let win = &pts[i - HALF..=i + HALF];
        // Same gear at full throttle, engine speed rising (not on the limiter), no slip
        if !win.iter().all(|q| full(q) && q.gear == p.gear) || b.rpm <= a.rpm {
            continue;
        }
        let r = typical[&p.gear];
        if win.iter().any(|q| (q.rpm / q.v / r - 1.0).abs() > RATIO_TOL) {
            continue;
        }
        let acc = (b.v - a.v) / 3.6 / (b.t - a.t);
        bins.entry((p.gear, (p.rpm / BIN_RPM).round() as i32)).or_default().push(acc * p.v / 3.6);
    }

    let gears: Vec<[f32; 3]> = bins
        .into_iter()
        .filter(|(_, v)| v.len() >= MIN_PER_BIN)
        .map(|((g, bin), mut v)| [g as f32, bin as f32 * BIN_RPM, round(median(&mut v), 1)])
        .filter(|x| x[2] > 0.0)
        .collect();

    // Envelope: the gear that put the most power on the road at each rpm (low gears lose to
    // wheelspin and inertia, high gears to drag)
    let mut best: BTreeMap<i32, f32> = BTreeMap::new();
    for [_, rpm, w] in &gears {
        let e = best.entry(*rpm as i32).or_insert(0.0);
        *e = e.max(*w);
    }
    let raw: Vec<(f32, f32)> = best.into_iter().map(|(r, w)| (r as f32, w)).collect();
    if raw.len() < 3 {
        return None;
    }
    let curve: Vec<[f32; 2]> = raw
        .iter()
        .map(|&(rpm, _)| {
            let near: Vec<f32> = raw.iter().filter(|(r, _)| (r - rpm).abs() <= BIN_RPM).map(|x| x.1).collect();
            [rpm, round(near.iter().sum::<f32>() / near.len() as f32, 1)]
        })
        .collect();
    let peak = curve.iter().copied().max_by(|a, b| a[1].total_cmp(&b[1]))?;
    let max_rpm = pts.iter().filter(|p| full(p)).map(|p| p.rpm).fold(0.0, f32::max);
    let ratios = gear_ratios(pts, full, transmission);
    let shifts = shift_points(&curve, &ratios, max_rpm, pts);
    Some(Dyno {
        curve,
        gears,
        peak,
        ratios: ratios.iter().map(|(g, r)| [*g as f32, round(*r, 3)]).collect(),
        max_rpm: max_rpm.round(),
        shifts,
    })
}

/// Best upshift per gear: where P(rpm) = P(rpm · k) with k = ratio(g+1) / ratio(g).
fn shift_points(curve: &[[f32; 2]], ratios: &BTreeMap<u8, f32>, max_rpm: f32, pts: &[Pt]) -> Vec<Shift> {
    let (lo_curve, hi_curve) = (curve[0][0], curve[curve.len() - 1][0]);
    let hi = hi_curve.min(max_rpm);
    let mut out = Vec::new();
    for (&g, &rg) in ratios {
        let Some(&rn) = ratios.get(&(g + 1)) else { continue };
        let k = rn / rg;
        if !(0.3..1.0).contains(&k) {
            continue;
        }
        // After the shift the engine must still be inside the measured curve
        let lo = (lo_curve / k).max(lo_curve);
        if lo >= hi {
            continue;
        }
        let gain = |r: f32| Some(at(curve, r)? - at(curve, r * k)?);
        let mut prev: Option<(f32, f32)> = None;
        let mut found = None;
        let mut r = lo;
        while r <= hi {
            if let Some(f) = gain(r) {
                if f <= 0.0 {
                    found = Some(match prev {
                        Some((pr, pf)) => pr + (r - pr) * pf / (pf - f),
                        None => r,
                    });
                    break;
                }
                prev = Some((r, f));
            }
            r += SHIFT_STEP_RPM;
        }
        let (rpm, late) = match found {
            Some(r) => (r, false),
            None => (max_rpm.max(hi), true),
        };
        // Highest rpm in the last 0.2 s before each upshift that was driven at full throttle
        // (the throttle is often lifted for the shift itself)
        let mut actual: Vec<f32> = (1..pts.len())
            .filter(|&i| pts[i - 1].gear == g && pts[i].gear == g + 1)
            .filter_map(|i| {
                let before: Vec<&Pt> = pts[i.saturating_sub(SHIFT_WINDOW)..i].iter().filter(|p| p.gear == g).collect();
                before.iter().any(|p| p.thr >= FULL_THROTTLE).then(|| before.iter().map(|p| p.rpm).fold(0.0, f32::max))
            })
            .collect();
        out.push(Shift {
            gear: g,
            rpm: (rpm / 10.0).round() * 10.0,
            kmh: round(rpm / rg, 1),
            late,
            actual: (!actual.is_empty()).then(|| median(&mut actual).round()),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run through gears 2 and 3 with a given power curve (W/kg over rpm), shifting at `shift`.
    fn run(power: impl Fn(f32) -> f32, ratios: [f32; 2], shift: f32) -> Vec<Pt> {
        let mut pts = Vec::new();
        let (mut t, mut v, mut gear) = (0.0f32, 40.0f32, 2u8);
        while v < 200.0 {
            let rpm = v * ratios[gear as usize - 2];
            if gear == 2 && rpm >= shift {
                gear = 3;
                continue;
            }
            pts.push(Pt { t, v, rpm, gear, thr: 255, ..Default::default() });
            v += power(rpm) / (v / 3.6) * 3.6 / 60.0;
            t += 1.0 / 60.0;
        }
        pts
    }

    #[test]
    fn constant_power_gives_flat_curve() {
        let d = compute(&run(|_| 200.0, [80.0, 60.0], 7000.0), [0.0; 8]).expect("curve");
        assert!(d.curve.len() > 10);
        assert!(d.curve.iter().all(|c| (c[1] - 200.0).abs() < 4.0), "{:?}", d.curve);
    }

    #[test]
    fn finds_the_best_shift_point() {
        // Power peaks at 6000 rpm and falls off; gear 3 = 0.75 × gear 2. Best shift where
        // P(r) = P(0.75 r): for a parabola around 6000 that is r = 2 · 6000 / 1.75 ≈ 6857.
        let power = |r: f32| (300.0 - ((r - 6000.0) / 100.0).powi(2) * 0.6).max(10.0);
        let pts = run(power, [80.0, 60.0], 8000.0);
        let d = compute(&pts, [0.0, 3.2, 2.4, 0.0, 0.0, 0.0, 0.0, 0.0]).expect("curve");
        let s = d.shifts.iter().find(|s| s.gear == 2).expect("shift 2→3");
        assert!(!s.late && (s.rpm - 6857.0).abs() < 120.0, "{s:?}");
        assert!((s.kmh - s.rpm / 80.0).abs() < 2.0);
        assert_eq!(s.actual, Some(8000.0f32.min(pts.iter().filter(|p| p.gear == 2).map(|p| p.rpm).fold(0.0, f32::max)).round()));
    }
}
