//! Finding the PS5 on the local network.
//!
//! GT7 answers a heartbeat on UDP port 33739 by streaming telemetry to the sender. Without a
//! configured address the heartbeat goes to the address found last time (remembered in
//! `state.json`) and to the broadcast address of every network interface (plus
//! 255.255.255.255, which some systems only send out on one interface). The first console that
//! answers is used and remembered.

use if_addrs::IfAddr;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

/// Network interfaces can change (Wi-Fi, VPN): re-read them this often while searching.
const REFRESH: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct Search {
    broadcast: Vec<Ipv4Addr>,
    refreshed: Option<Instant>,
}

impl Search {
    /// Heartbeat targets while searching: the remembered address first, then all broadcasts.
    pub fn targets(&mut self, remembered: Option<Ipv4Addr>) -> Vec<Ipv4Addr> {
        if self.refreshed.is_none_or(|t| t.elapsed() > REFRESH) {
            self.broadcast = broadcast_addresses();
            self.refreshed = Some(Instant::now());
        }
        remembered.into_iter().chain(self.broadcast.iter().copied()).collect()
    }
}

/// Broadcast address of an interface (all host bits set).
fn directed(ip: Ipv4Addr, netmask: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) | !u32::from(netmask))
}

pub fn broadcast_addresses() -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_p2p)
        .filter_map(|i| match i.addr {
            IfAddr::V4(a) if !a.ip.is_loopback() && !a.ip.is_link_local() && a.prefixlen < 31 => {
                Some(a.broadcast.unwrap_or_else(|| directed(a.ip, a.netmask)))
            }
            _ => None,
        })
        .collect();
    out.push(Ipv4Addr::BROADCAST);
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_targets() {
        assert_eq!(directed(Ipv4Addr::new(192, 168, 0, 117), Ipv4Addr::new(255, 255, 255, 0)), Ipv4Addr::new(192, 168, 0, 255));
        assert_eq!(directed(Ipv4Addr::new(10, 1, 2, 3), Ipv4Addr::new(255, 255, 0, 0)), Ipv4Addr::new(10, 1, 255, 255));
        let remembered = Ipv4Addr::new(192, 168, 0, 33);
        let t = Search::default().targets(Some(remembered));
        assert_eq!(t[0], remembered);
        assert!(t.contains(&Ipv4Addr::BROADCAST));
    }
}
