//! Storage of measurement runs, one JSON file per run: `<perf dir>/<car code>/<run id>.json`.
//! Read back for `/api/perf`.

use super::{Dyno, Kind, Mark, Run};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Runs listed in the overview (newest first)
const LIST: usize = 40;
/// Marks measured on a steeper slope (%) are listed but don't count as best values.
const MAX_GRADE: f32 = 2.0;

/// Response of `/api/perf` (typed, so f32 values keep their short form in JSON).
#[derive(Serialize)]
struct Overview<'a> {
    car_code: i32,
    car: Option<String>,
    max_grade: f32,
    best: BTreeMap<&'a str, Best<'a>>,
    runs: Vec<Summary<'a>>,
    last_accel: Option<&'a Run>,
    best_accel: Option<&'a Run>,
    last_brake: Option<&'a Run>,
    /// Power curve of the most recent run that has one (short runs have none)
    dyno: Option<&'a Dyno>,
    dyno_time: Option<&'a str>,
}

#[derive(Serialize)]
struct Best<'a> {
    t: f32,
    d: f32,
    v: f32,
    run: &'a str,
    time: &'a str,
}

/// A run without its trace and dyno curve, for the list.
#[derive(Serialize)]
struct Summary<'a> {
    id: &'a str,
    kind: Kind,
    time: &'a str,
    v_start: f32,
    v_max: f32,
    duration_s: f32,
    dist_m: f32,
    grade_pct: f32,
    marks: &'a [Mark],
}

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
    saved: Arc<AtomicU32>,
}

/// Braking marks ("100-0") are ranked by distance, all others by time.
pub fn is_brake_key(key: &str) -> bool {
    key.ends_with("-0")
}

fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(tmp, path)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Store { dir, saved: Arc::default() }
    }

    /// Number of runs saved since start; changes tell the dashboard to reload.
    pub fn saved(&self) -> u32 {
        self.saved.load(Ordering::Relaxed)
    }

    fn car_dir(&self, car: i32) -> PathBuf {
        self.dir.join(car.to_string())
    }

    pub fn save(&self, run: &Run) -> io::Result<PathBuf> {
        let dir = self.car_dir(run.car_code);
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", run.id));
        write_atomic(&path, &serde_json::to_string(run)?)?;
        self.saved.fetch_add(1, Ordering::Relaxed);
        Ok(path)
    }

    /// Saves without blocking the telemetry loop.
    pub fn save_in_background(&self, run: Run) {
        let store = self.clone();
        std::thread::spawn(move || {
            if let Err(e) = store.save(&run) {
                eprintln!("{}", crate::tr!("Messung nicht gespeichert: {e}", "Measurement not saved: {e}"));
            }
        });
    }

    fn run_files(&self, car: i32) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = fs::read_dir(self.car_dir(car))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        // Ids start with the time: sorting by name = by time
        files.sort();
        files.reverse();
        files
    }

    /// For imported runs: a run that is already stored (saved live, or imported before with a
    /// start time a second off) keeps its id, so importing again overwrites instead of doubling.
    pub fn dedupe(&self, mut run: Run) -> Run {
        let start = |id: &str| chrono::NaiveDateTime::parse_from_str(id.get(..19)?, "%Y-%m-%d_%H-%M-%S").ok();
        let Some(t) = start(&run.id) else { return run };
        let same = self.run_files(run.car_code).into_iter().find_map(|p| {
            let id = p.file_stem()?.to_str()?.to_string();
            let close = start(&id).is_some_and(|o| (o - t).num_seconds().abs() <= 3);
            (close && id.ends_with(&run.id[19..])).then_some(id)
        });
        if let Some(id) = same {
            run.id = id;
        }
        run
    }

    /// All runs of a car, newest first.
    pub fn runs(&self, car: i32) -> Vec<Run> {
        self.run_files(car).iter().filter_map(|p| read_json(p)).collect()
    }

    /// Car with the most recent run.
    fn latest_car(&self) -> Option<i32> {
        fs::read_dir(&self.dir)
            .ok()?
            .flatten()
            .filter_map(|e| {
                let car: i32 = e.file_name().to_str()?.parse().ok()?;
                let newest = self.run_files(car).into_iter().next()?;
                Some((newest.file_name()?.to_os_string(), car))
            })
            .max()
            .map(|(_, car)| car)
    }

    /// Everything page 4 shows for one car (default: the car of the most recent run) as JSON:
    /// best values, the list of runs, and full traces of the last and the best acceleration run
    /// and of the last braking run.
    pub fn overview(&self, car: Option<i32>) -> String {
        let Some(car) = car.or_else(|| self.latest_car()) else {
            return json!({ "car_code": null, "runs": [], "best": {} }).to_string();
        };
        let runs = self.runs(car);

        let mut best: BTreeMap<&str, (f32, &Mark, &Run)> = BTreeMap::new();
        for r in &runs {
            for m in r.marks.iter().filter(|m| m.g.abs() <= MAX_GRADE) {
                let score = if is_brake_key(&m.key) { m.d } else { m.t };
                if best.get(m.key.as_str()).is_none_or(|b| score < b.0) {
                    best.insert(&m.key, (score, m, r));
                }
            }
        }
        let last_accel = runs.iter().find(|r| r.kind == Kind::Accel);
        let best_id = ["0-100", "0-50"].iter().find_map(|k| best.get(k).map(|b| b.2.id.as_str()));
        let best_accel = runs.iter().find(|r| Some(r.id.as_str()) == best_id && Some(&r.id) != last_accel.map(|l| &l.id));

        let dyno_run = runs.iter().find(|r| r.dyno.is_some());
        let overview = Overview {
            car_code: car,
            car: runs.first().map(|r| r.car.clone()).or_else(|| crate::telemetry::cars::name(car).map(str::to_string)),
            max_grade: MAX_GRADE,
            best: best.iter().map(|(k, (_, m, r))| (*k, Best { t: m.t, d: m.d, v: m.v, run: &r.id, time: &r.time })).collect(),
            runs: runs
                .iter()
                .take(LIST)
                .map(|r| Summary {
                    id: &r.id,
                    kind: r.kind,
                    time: &r.time,
                    v_start: r.v_start,
                    v_max: r.v_max,
                    duration_s: r.duration_s,
                    dist_m: r.dist_m,
                    grade_pct: r.grade_pct,
                    marks: &r.marks,
                })
                .collect(),
            last_accel,
            best_accel,
            last_brake: runs.iter().find(|r| r.kind == Kind::Brake),
            dyno: dyno_run.and_then(|r| r.dyno.as_ref()),
            dyno_time: dyno_run.map(|r| r.time.as_str()),
        };
        serde_json::to_string(&overview).unwrap_or_default()
    }
}
