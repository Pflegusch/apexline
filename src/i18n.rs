//! Language and units of the texts the server writes itself: coach feedback and analysis files,
//! console reports and the log. The dashboard translates its own texts (`web/i18n.js`).
//!
//! Set at start from the configuration and again when the settings change on the dashboard;
//! until then (e.g. in tests) German and metric. Values are stored metric everywhere and
//! converted only when formatted.

use crate::config::{Language, Units};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    De,
    En,
}

static ENGLISH: AtomicBool = AtomicBool::new(false);
static IMPERIAL: AtomicBool = AtomicBool::new(false);

pub const KMH_PER_MPH: f64 = 1.609344;
const FT_PER_M: f64 = 3.280_84;
const GAL_PER_L: f64 = 0.264_172;

/// Language of the operating system (`LC_ALL`, `LC_MESSAGES`, `LANG`): German or else English.
pub fn system_lang() -> Lang {
    let german = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
        .is_some_and(|l| l.starts_with("de"));
    if german {
        Lang::De
    } else {
        Lang::En
    }
}

/// Effective language and units for a configuration ("auto" resolved).
pub fn resolve(language: Language, units: Units) -> (Lang, bool) {
    let lang = match language {
        Language::De => Lang::De,
        Language::En => Lang::En,
        Language::Auto => system_lang(),
    };
    let imperial = match units {
        Units::Metric => false,
        Units::Imperial => true,
        Units::Auto => lang == Lang::En,
    };
    (lang, imperial)
}

pub fn set(language: Language, units: Units) {
    let (lang, imperial) = resolve(language, units);
    ENGLISH.store(lang == Lang::En, Ordering::Relaxed);
    IMPERIAL.store(imperial, Ordering::Relaxed);
}

pub fn german() -> bool {
    !ENGLISH.load(Ordering::Relaxed)
}

pub fn imperial() -> bool {
    IMPERIAL.load(Ordering::Relaxed)
}

/// German or English text; the strings may capture variables like `format!`.
#[macro_export]
macro_rules! tr {
    ($de:literal, $en:literal $(,)?) => {
        if $crate::i18n::german() {
            format!($de)
        } else {
            format!($en)
        }
    };
}

/// Number with the decimal separator of the language.
pub fn num(v: f64, digits: usize) -> String {
    let s = format!("{v:.digits$}");
    if german() {
        s.replace('.', ",")
    } else {
        s
    }
}

pub fn speed_unit() -> &'static str {
    if imperial() {
        "mph"
    } else {
        "km/h"
    }
}

/// km/h in the display unit (value only).
pub fn speed_value(kmh: f64) -> f64 {
    if imperial() {
        kmh / KMH_PER_MPH
    } else {
        kmh
    }
}

/// "62 km/h" or "39 mph" (rounded).
pub fn speed(kmh: f64) -> String {
    format!("{:.0} {}", speed_value(kmh), speed_unit())
}

/// "12 m" or "39 ft" (rounded).
pub fn dist(m: f64) -> String {
    if imperial() {
        format!("{:.0} ft", m * FT_PER_M)
    } else {
        format!("{m:.0} m")
    }
}

/// Like [`dist`] with decimals.
pub fn dist_prec(m: f64, digits: usize) -> String {
    if imperial() {
        format!("{} ft", num(m * FT_PER_M, digits))
    } else {
        format!("{} m", num(m, digits))
    }
}

/// "92 °C" or "198 °F" (rounded).
pub fn temp(c: f64) -> String {
    if imperial() {
        format!("{:.0} °F", c * 9.0 / 5.0 + 32.0)
    } else {
        format!("{c:.0} °C")
    }
}

/// Fuel amount: "2,31 L" or "0.61 gal".
pub fn fuel(l: f64, digits: usize) -> String {
    if imperial() {
        format!("{} gal", num(l * GAL_PER_L, digits))
    } else {
        format!("{} L", num(l, digits))
    }
}

/// Corner label: "K3" (Kurve) or "T3" (turn).
pub fn corner(n: usize) -> String {
    if german() {
        format!("K{n}")
    } else {
        format!("T{n}")
    }
}

/// Engine speed unit.
pub fn rpm_unit() -> &'static str {
    if german() {
        "U/min"
    } else {
        "rpm"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_auto() {
        assert_eq!(resolve(Language::En, Units::Auto), (Lang::En, true));
        assert_eq!(resolve(Language::De, Units::Auto), (Lang::De, false));
        assert_eq!(resolve(Language::De, Units::Imperial), (Lang::De, true));
        // Not initialised in tests: German, metric
        assert_eq!((num(1.25, 2), speed(100.0), dist(12.4), temp(92.0)), ("1,25".into(), "100 km/h".into(), "12 m".into(), "92 °C".into()));
    }
}
