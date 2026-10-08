//! Configuration and data locations.
//!
//! `config.toml` lives in the OS config folder (Linux `~/.config/apexline/`, macOS
//! `~/Library/Application Support/apexline/`, Windows `%APPDATA%\apexline\config\`). It is created
//! with defaults and comments on the first start and rewritten when settings are changed on the
//! dashboard. Command line options override it for the current run.
//!
//! All data goes into one data folder (default: the OS data folder, Linux
//! `~/.local/share/apexline/`): `recordings/` (sessions), `coach/feed.json`, `perf/` and
//! `state.json` (things the program remembers by itself, e.g. the PS5 address it found).

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

const APP: &str = "apexline";
pub const DEFAULT_PORT: u16 = 8080;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Dashboard: browser language; texts written by the server: system language
    #[default]
    Auto,
    De,
    En,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    /// German → metric, English → imperial
    #[default]
    Auto,
    Metric,
    Imperial,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Empty: search the PS5 automatically
    pub ps5_ip: String,
    pub http_port: u16,
    pub language: Language,
    pub units: Units,
    /// Empty: OS data folder
    pub data_dir: String,
}

impl Default for Config {
    fn default() -> Self {
        Config { ps5_ip: String::new(), http_port: DEFAULT_PORT, language: Language::Auto, units: Units::Auto, data_dir: String::new() }
    }
}

/// TOML string literal (quoted and escaped).
fn quote(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

impl Config {
    /// The PS5 address from the settings; `None` = automatic search.
    pub fn ps5(&self) -> Option<Ipv4Addr> {
        self.ps5_ip.trim().parse().ok()
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.ps5_ip.trim().is_empty() && self.ps5().is_none() {
            let ip = &self.ps5_ip;
            return Err(crate::tr!("ps5_ip: \"{ip}\" ist keine IPv4-Adresse", "ps5_ip: \"{ip}\" is not an IPv4 address"));
        }
        if self.http_port == 0 {
            return Err("http_port: 1–65535".into());
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Config, String> {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let cfg: Config = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        cfg.validate().map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(cfg)
    }

    /// Writes the file with comments (any comments of your own are not kept).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, self.to_toml())?;
        fs::rename(tmp, path)
    }

    /// Values equal to the default are written commented out, so later default changes apply.
    fn to_toml(&self) -> String {
        let d = Config::default();
        let line = |key: &str, value: String, default: bool| format!("{}{key} = {value}\n", if default { "# " } else { "" });
        let mut out = String::from(
            "# Apexline configuration. Command line options override these values for one run.\n\
             # This file is rewritten when settings are changed on the dashboard; lines starting with\n\
             # \"#\" show the default value.\n",
        );
        let mut add = |comment: &str, l: String| {
            out.push('\n');
            for c in comment.lines() {
                out.push_str("# ");
                out.push_str(c);
                out.push('\n');
            }
            out.push_str(&l);
        };
        add(
            "IP address of the PS5. Empty: search automatically on the local network\n(the address found is remembered in state.json in the data folder).",
            line("ps5_ip", quote(&self.ps5_ip), self.ps5_ip == d.ps5_ip),
        );
        add(
            "Port of the dashboard: http://<this computer>:<port>. Takes effect after a restart.",
            line("http_port", self.http_port.to_string(), self.http_port == d.http_port),
        );
        add(
            "Language of dashboard and coach: \"de\", \"en\" or \"auto\" (dashboard: browser language,\ncoach and console: system language)",
            line("language", quote(self.language.as_str()), self.language == d.language),
        );
        add(
            "\"auto\" (follows the language – German: km/h, °C, L, bar; English: mph, °F, gal, psi),\n\"metric\" or \"imperial\"",
            line("units", quote(self.units.as_str()), self.units == d.units),
        );
        add(
            &format!("Folder for sessions, coach and measurements. Empty: {}", default_data_dir().display()),
            line("data_dir", quote(&self.data_dir), self.data_dir == d.data_dir),
        );
        out
    }
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Language::Auto => "auto",
            Language::De => "de",
            Language::En => "en",
        }
    }
}

impl Units {
    pub fn as_str(self) -> &'static str {
        match self {
            Units::Auto => "auto",
            Units::Metric => "metric",
            Units::Imperial => "imperial",
        }
    }
}

fn dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "", APP)
}

pub fn default_config_file() -> PathBuf {
    dirs().map_or_else(|| PathBuf::from("config.toml"), |d| d.config_dir().join("config.toml"))
}

pub fn default_data_dir() -> PathBuf {
    dirs().map_or_else(|| PathBuf::from("apexline-data"), |d| d.data_dir().to_path_buf())
}

/// Where everything is stored.
#[derive(Clone, Debug)]
pub struct DataPaths {
    pub root: PathBuf,
    pub recordings: PathBuf,
    pub coach_feed: PathBuf,
    pub perf: PathBuf,
    pub state: PathBuf,
}

impl DataPaths {
    /// `cli` (--data-dir) beats the config file, which beats the OS default.
    pub fn resolve(cli: Option<&Path>, cfg: &Config) -> Self {
        let root = match (cli, cfg.data_dir.trim()) {
            (Some(p), _) => p.to_path_buf(),
            (None, "") => default_data_dir(),
            (None, p) => PathBuf::from(p),
        };
        DataPaths {
            recordings: root.join("recordings"),
            coach_feed: root.join("coach").join("feed.json"),
            perf: root.join("perf"),
            state: root.join("state.json"),
            root,
        }
    }
}

/// Things the program remembers by itself (not settings), stored in `state.json`.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
#[serde(default)]
pub struct State {
    /// PS5 address found by the automatic search
    pub ps5_found: Option<Ipv4Addr>,
}

impl State {
    pub fn load(path: &Path) -> State {
        fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!("apexline-cfg-{}", std::process::id()));
        let path = dir.join("config.toml");
        let _ = fs::remove_dir_all(&dir);

        // Missing file: defaults
        assert_eq!(Config::load(&path).unwrap(), Config::default());

        let cfg = Config {
            ps5_ip: "192.168.0.33".into(),
            http_port: 8081,
            language: Language::En,
            units: Units::Imperial,
            data_dir: "/data/\"x\"".into(),
        };
        cfg.save(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# IP address of the PS5") && text.contains("\nps5_ip = \"192.168.0.33\""), "{text}");
        // Defaults are written commented out
        assert!(Config::default().to_toml().contains("\n# http_port = 8080"));
        let back = Config::load(&path).unwrap();
        assert_eq!(back, cfg);
        assert_eq!(back.ps5(), Some(Ipv4Addr::new(192, 168, 0, 33)));

        // Partial file: the rest from the defaults; errors name the file
        fs::write(&path, "http_port = 9000\n").unwrap();
        assert_eq!(Config::load(&path).unwrap(), Config { http_port: 9000, ..Default::default() });
        fs::write(&path, "ps5_ip = \"192.168.0\"\n").unwrap();
        assert!(Config::load(&path).unwrap_err().contains("config.toml"));
        fs::write(&path, "http_prot = 9000\n").unwrap();
        assert!(Config::load(&path).is_err(), "typos are reported");
        let _ = fs::remove_dir_all(&dir);
    }
}
