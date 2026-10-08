//! Telemetry from the PS5: heartbeat and receive loop (here), decryption and parsing
//! ([`packet`]), vehicle dynamics ([`dynamics`]), car names ([`cars`]) and finding the PS5 on the
//! network ([`discovery`]).

pub mod cars;
pub mod discovery;
pub mod dynamics;
pub mod packet;

use crate::config::State;
use crate::session::tracker::Tracker;
use crate::tr;
use crate::web::Publisher;
use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tokio::time::{self, Instant};

/// GT7 listens for heartbeats here …
pub const SEND_PORT: u16 = 33739;
/// … and streams telemetry to this port of the sender.
pub const RECV_PORT: u16 = 33740;
/// No packet for this long: the connection counts as lost.
const SILENCE: Duration = Duration::from_secs(2);

/// Connection state, part of every dashboard snapshot.
#[derive(Serialize, Clone, Default, Debug, PartialEq)]
pub struct Link {
    /// PS5 address from the settings; `null` = automatic search
    pub configured: Option<Ipv4Addr>,
    /// Where the data comes from, or the remembered address while searching
    pub ps5: Option<Ipv4Addr>,
    /// Searching by broadcast right now
    pub searching: bool,
}

/// Receives telemetry until the program ends. `configured` carries the PS5 address from the
/// settings (`None` = search) and may change at runtime; `state_file` remembers the address found.
pub async fn run(
    sock: UdpSocket,
    mut configured: watch::Receiver<Option<Ipv4Addr>>,
    state_file: PathBuf,
    mut tracker: Tracker,
    mut out: Publisher,
) {
    let mut fixed = *configured.borrow_and_update();
    let mut state = State::load(&state_file);
    let mut search = discovery::Search::default();
    let mut heartbeat = time::interval(Duration::from_secs(1));
    let mut buf = [0u8; 1024];
    let mut last_packet: Option<Instant> = None;
    let mut source: Option<Ipv4Addr> = None;
    let mut send_error = String::new();
    // GT7 keeps the format of a running stream (e.g. "A" requested by another app). If only short
    // packets arrive, pause the heartbeat until that stream ends, then request the extended format.
    let mut short_packets = 0u32;
    let mut silent_until: Option<Instant> = None;

    match fixed {
        Some(ip) => println!("{}", tr!("Sende Heartbeat an PS5 {ip}", "Sending heartbeat to PS5 {ip}")),
        None => match state.ps5_found {
            Some(ip) => println!(
                "{}",
                tr!("Suche PS5 (zuletzt {ip}, außerdem per Broadcast) …", "Searching for the PS5 (last {ip}, also by broadcast) …")
            ),
            None => println!("{}", tr!("Suche PS5 per Broadcast …", "Searching for the PS5 by broadcast …")),
        },
    }

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                let receiving = last_packet.is_some_and(|t| t.elapsed() < SILENCE);
                let targets = match (fixed, source.filter(|_| receiving)) {
                    (Some(ip), _) | (None, Some(ip)) => vec![ip],
                    (None, None) => search.targets(state.ps5_found),
                };
                if silent_until.is_none_or(|t| Instant::now() >= t) {
                    silent_until = None;
                    for ip in &targets {
                        if let Err(e) = sock.send_to(packet::HEARTBEAT, (*ip, SEND_PORT)).await {
                            // Report each new error once instead of every second
                            let msg = tr!("Heartbeat an {ip} fehlgeschlagen: {e}", "Heartbeat to {ip} failed: {e}");
                            if msg != send_error {
                                eprintln!("{msg}");
                                send_error = msg;
                            }
                        }
                    }
                }
                tracker.tick();
                let link = Link {
                    configured: fixed,
                    ps5: if receiving { source } else { fixed.or(state.ps5_found) },
                    searching: fixed.is_none() && !receiving,
                };
                tracker.set_link(link);
                if !receiving {
                    if last_packet.take().is_some() {
                        println!("{}", tr!("Keine Daten mehr – warte auf GT7 …", "No more data – waiting for GT7 …"));
                    }
                    // Every second, so the dashboard shows the search status
                    let snap = tracker.mark_offline().clone();
                    out.publish(&tracker, &snap);
                }
            }
            Ok(()) = configured.changed() => {
                fixed = *configured.borrow_and_update();
                match fixed {
                    Some(ip) => println!("{}", tr!("PS5-Adresse geändert: {ip}", "PS5 address changed: {ip}")),
                    None => println!("{}", tr!("PS5-Adresse gelöscht: automatische Suche", "PS5 address cleared: automatic search")),
                }
                last_packet = None;
                source = None;
                heartbeat.reset_immediately();
            }
            res = sock.recv_from(&mut buf) => match res {
                Ok((n, from)) => {
                    let IpAddr::V4(ip) = from.ip() else { continue };
                    // After changing the address, the old console streams on for a few seconds
                    if fixed.is_some_and(|f| f != ip) {
                        continue;
                    }
                    let Some(p) = packet::parse(&buf[..n]) else { continue };
                    if last_packet.is_none() || source != Some(ip) {
                        println!("{}", tr!("Telemetrie empfangen von {ip} ({n} Bytes)", "Receiving telemetry from {ip} ({n} bytes)"));
                    }
                    if fixed.is_none() && state.ps5_found != Some(ip) {
                        state.ps5_found = Some(ip);
                        match state.save(&state_file) {
                            Ok(()) => println!("{}", tr!("PS5 gefunden: {ip} (gemerkt)", "PS5 found: {ip} (remembered)")),
                            Err(e) => eprintln!("{}", tr!("PS5 gefunden: {ip}, aber nicht gespeichert: {e}", "PS5 found: {ip}, but not saved: {e}")),
                        }
                    }
                    let was_receiving = last_packet.is_some();
                    last_packet = Some(Instant::now());
                    source = Some(ip);
                    if !was_receiving {
                        tracker.set_link(Link { configured: fixed, ps5: Some(ip), searching: false });
                    }
                    if p.extended {
                        short_packets = 0;
                    } else {
                        short_packets += 1;
                        if short_packets == 180 {
                            println!("{}", tr!("Kurzes Paketformat – fordere erweitertes Format neu an …", "Short packet format – requesting the extended format again …"));
                            silent_until = Some(Instant::now() + Duration::from_secs(8));
                        }
                    }
                    let snap = tracker.update(&p).clone();
                    out.publish(&tracker, &snap);
                }
                Err(e) => eprintln!("{}", tr!("UDP-Fehler: {e}", "UDP error: {e}")),
            }
        }
    }
}

/// Binds the receive port; explains the usual cause when it is taken.
pub async fn bind() -> std::io::Result<UdpSocket> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, RECV_PORT)).await.map_err(|e| {
        let port = RECV_PORT;
        std::io::Error::new(
            e.kind(),
            tr!(
                "UDP-Port {port} nicht verfügbar ({e}) – läuft Apexline oder ein anderes Telemetrie-Programm schon?",
                "UDP port {port} not available ({e}) – is Apexline or another telemetry program already running?"
            ),
        )
    })?;
    sock.set_broadcast(true)?;
    Ok(sock)
}
