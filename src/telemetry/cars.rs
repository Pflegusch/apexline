//! Car names from the community database (github.com/ddm999/gt7info).

use std::collections::HashMap;
use std::sync::OnceLock;

static CARS: OnceLock<HashMap<i32, String>> = OnceLock::new();

fn load() -> HashMap<i32, String> {
    let makers: HashMap<&str, &str> = include_str!("../../data/maker.csv")
        .lines()
        .skip(1)
        .filter_map(|l| {
            let mut f = l.split(',');
            Some((f.next()?, f.next()?))
        })
        .collect();
    include_str!("../../data/cars.csv")
        .lines()
        .skip(1)
        .filter_map(|l| {
            let mut f = l.split(',');
            let id = f.next()?.parse().ok()?;
            let name = f.next()?;
            let maker = f.next().and_then(|m| makers.get(m)).copied().unwrap_or("");
            Some((id, format!("{maker} {name}").trim().to_string()))
        })
        .collect()
}

pub fn name(code: i32) -> Option<&'static str> {
    CARS.get_or_init(load).get(&code).map(String::as_str)
}
