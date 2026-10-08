mod coach;
mod config;
mod demo;
mod i18n;
mod perf;
mod session;
mod telemetry;
mod web;

use clap::{Parser, Subcommand};
use config::{Config, DataPaths};
use session::{recorder::Recorder, tracker::Tracker};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

#[derive(Parser)]
#[command(name = "apexline", version, about = "Telemetry dashboard and driving coach for Gran Turismo 7")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// IP address of the PS5 [default: from config.toml, otherwise automatic search]
    #[arg(long, global = true)]
    ps5: Option<Ipv4Addr>,

    /// HTTP port of the dashboard [default: from config.toml, otherwise 8080]
    #[arg(long, global = true)]
    port: Option<u16>,

    /// Data folder for sessions, coach and measurements [default: from config.toml, otherwise the OS data folder]
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    /// Configuration file [default: config.toml in the OS config folder]
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Dashboard updates per second (max. 60)
    #[arg(long, global = true, default_value_t = 30)]
    hz: u32,

    /// Do not record laps
    #[arg(long, global = true)]
    no_record: bool,

    /// Do not run the coach
    #[arg(long, global = true)]
    no_coach: bool,

    /// Serve the dashboard from this folder instead of the built-in copy (development, e.g. `--web-dir web`)
    #[arg(long, global = true)]
    web_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Simulated telemetry instead of the PS5 (nothing is recorded)
    Demo,
    /// Analysis on the console: a session folder (corner comparison) or a lap file (driving style)
    Analyze {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Run only the coach (without the server)
    Coach {
        /// Analyse all pending laps and sessions once, then exit
        #[arg(long)]
        once: bool,
    },
    /// Find measurement runs (0–100, braking …) in recorded sessions or lap files
    Perf {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Store the runs found (they then show up on dashboard page 4)
        #[arg(long)]
        save: bool,
    },
    /// Show the configuration file, the data folder and the settings in effect
    Config,
}

fn main() {
    let args = Args::parse();
    let config_file = args.config.clone().unwrap_or_else(config::default_config_file);
    let cfg = match Config::load(&config_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", tr!("Fehler in der Konfiguration: {e}", "Error in the configuration: {e}"));
            std::process::exit(2);
        }
    };
    i18n::set(cfg.language, cfg.units);
    let data = DataPaths::resolve(args.data_dir.as_deref(), &cfg);

    match args.command {
        Some(Command::Analyze { ref paths }) => {
            // Exit quietly when the output is piped into e.g. `head` (only here: the server needs
            // the default "ignore SIGPIPE" for its sockets).
            #[cfg(unix)]
            unsafe {
                libc::signal(libc::SIGPIPE, libc::SIG_DFL);
            }
            for path in paths {
                let res = if path.is_dir() { coach::report::session(path) } else { coach::report::lap_file(path) };
                if let Err(e) = res {
                    eprintln!("{}: {e}", path.display());
                }
            }
        }
        Some(Command::Coach { once }) => {
            let mut c = coach::worker::Coach::new(data.recordings.clone(), data.coach_feed.clone());
            if once {
                c.tick();
            } else {
                let (_wake_tx, wake_rx) = std::sync::mpsc::channel();
                c.run(wake_rx);
            }
        }
        Some(Command::Perf { ref paths, save }) => {
            let store = save.then(|| perf::Store::new(data.perf.clone()));
            perf::cli::run(paths, store.as_ref());
        }
        Some(Command::Config) => print_config(&config_file, &cfg, &data),
        Some(Command::Demo) => run_server(&args, cfg, &config_file, data, true),
        None => run_server(&args, cfg, &config_file, data, false),
    }
}

fn print_config(file: &Path, cfg: &Config, data: &DataPaths) {
    let state = config::State::load(&data.state);
    let (f, missing) = (
        file.display(),
        if file.exists() { String::new() } else { tr!(" (noch nicht angelegt, Standardwerte)", " (not created yet, defaults)") },
    );
    let ps5 = match (cfg.ps5(), state.ps5_found) {
        (Some(ip), _) => tr!("{ip} (fest eingestellt)", "{ip} (fixed)"),
        (None, Some(ip)) => tr!("automatische Suche, zuletzt gefunden: {ip}", "automatic search, last found: {ip}"),
        (None, None) => tr!("automatische Suche", "automatic search"),
    };
    let (d, port, lang, units) = (data.root.display(), cfg.http_port, cfg.language.as_str(), cfg.units.as_str());
    println!("{}", tr!("Konfiguration: {f}{missing}", "Configuration: {f}{missing}"));
    println!("{}", tr!("Datenordner:   {d}", "Data folder:   {d}"));
    println!("PS5:           {ps5}");
    println!("Port:          {port}");
    println!("{}", tr!("Sprache:       {lang}, Einheiten: {units}", "Language:      {lang}, units: {units}"));
}

#[tokio::main]
async fn run_server(args: &Args, cfg: Config, config_file: &Path, data: DataPaths, demo: bool) {
    if let Err(e) = server(args, cfg, config_file, data, demo).await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

async fn server(args: &Args, cfg: Config, config_file: &Path, data: DataPaths, demo: bool) -> std::io::Result<()> {
    if !config_file.exists() {
        match cfg.save(config_file) {
            Ok(()) => {
                let f = config_file.display();
                println!("{}", tr!("Konfiguration angelegt: {f}", "Configuration created: {f}"));
            }
            Err(e) => {
                let f = config_file.display();
                eprintln!("{}", tr!("Konfiguration nicht angelegt ({f}): {e}", "Configuration not created ({f}): {e}"));
            }
        }
    } else {
        let f = config_file.display();
        println!("{}", tr!("Konfiguration: {f}", "Configuration: {f}"));
    }
    let d = data.root.display();
    println!("{}", tr!("Daten: {d}", "Data: {d}"));

    let port = args.port.unwrap_or(cfg.http_port);
    let (ps5_tx, ps5_rx) = watch::channel(args.ps5.or(cfg.ps5()));
    let (publisher, feeds) = web::Publisher::new();
    let recording = !args.no_record && !demo;
    // The coach runs in its own thread; the recorder wakes it when a lap or session ends.
    let wake = (recording && !args.no_coach).then(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        let c = coach::worker::Coach::new(data.recordings.clone(), data.coach_feed.clone());
        std::thread::spawn(move || c.run(rx));
        tx
    });
    let recorder = recording.then(|| Recorder::start(data.recordings.clone(), wake));
    let perf_store = perf::Store::new(data.perf.clone());
    // Demo data is measured live but not stored
    let tracker = Tracker::new(recorder, (!demo).then(|| perf_store.clone()));

    if demo {
        println!("{}", tr!("Demo-Modus: simulierte Telemetrie", "Demo mode: simulated telemetry"));
        tokio::spawn(demo::run(tracker, publisher));
    } else {
        let sock = telemetry::bind().await?;
        tokio::spawn(telemetry::run(sock, ps5_rx, data.state.clone(), tracker, publisher));
    }

    let state = web::api::AppState {
        feeds,
        hz: args.hz.clamp(1, 60),
        web_dir: args.web_dir.clone(),
        perf: perf_store,
        settings: Arc::new(web::api::Settings { config: Mutex::new(cfg), config_file: config_file.to_path_buf(), data, ps5: ps5_tx, port }),
    };
    web::server::serve(port, state).await
}
