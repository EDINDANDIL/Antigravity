use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{Ipv4Addr, UdpSocket};
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use crate::dns_client;
use crate::egress;

// A loopback DNS relay, so the answers stay fresh.
//
// Pinning the substituted address into `hosts` works but breaks its own
// contract: xbox-dns answers with **TTL 60**, i.e. it explicitly reserves the
// right to move the address every minute, and a static file holds it forever.
//
// Instead the NRPT rules point at this listener, which relays the query - raw
// bytes, never parsed - to xbox-dns over the ISP link via IP_UNICAST_IF, and
// relays the answer back verbatim. Windows then caches it for exactly the TTL
// the resolver asked for and comes back when it expires, so a rotated address is
// picked up within a minute with no file to go stale. It behaves identically
// with and without a VPN: without one the ISP link is the default path anyway.
//
// Verified before building: the Windows DNS client does send NRPT queries to a
// 127.0.0.0/8 nameserver (a probe rule delivered the question to a listener on
// 127.0.0.53:53).
//
// UDP only, deliberately. Relaying verbatim means a truncated answer would need
// a TCP listener as well, but the routed names answer with a single A record in
// well under 100 bytes, so TC is never set. If that ever changes, Windows falls
// back to the next nameserver in the rule rather than failing outright.

/// Where the NRPT rules point. Any 127.0.0.0/8 address works; .53 keeps it
/// recognisable and clear of anything bound to 127.0.0.1.
pub const LISTEN_IP: &str = "127.0.0.53";
pub const LISTEN_PORT: u16 = 53;

/// Closes the console Windows hands a console subsystem process. Without this
/// the scheduled task leaves an empty black window on screen for as long as the
/// relay lives, which is forever. Everything it has to say goes to the log file
/// anyway, so losing stdout costs nothing.
#[cfg(target_os = "windows")]
pub fn detach_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn FreeConsole() -> i32;
    }
    unsafe {
        FreeConsole();
    }
}

#[cfg(not(target_os = "windows"))]
pub fn detach_console() {}

const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(4);
/// How long a detected interface is trusted. Detection shells out to
/// PowerShell, so it is cached; the lookup is lazy, so an idle machine spawns
/// nothing at all.
const EGRESS_TTL: Duration = Duration::from_secs(300);
const LOG_LIMIT_BYTES: u64 = 64 * 1024;

static EGRESS_CACHE: Mutex<Option<(u32, Instant)>> = Mutex::new(None);

/// The log lives under the user profile, not next to the exe: the relay runs
/// unelevated, and the directory an administrator installed it into is not
/// writable for it.
pub fn log_dir() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("AGUnlocker")
}

pub fn log_path() -> PathBuf {
    log_dir().join("forwarder.log")
}

/// Best-effort logging: a background process with no console is otherwise
/// impossible to diagnose. Truncated rather than rotated - nothing here is
/// worth keeping across sessions.
fn log(line: &str) {
    let path = log_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).ok();
    }
    if fs::metadata(&path).map_or(false, |m| m.len() > LOG_LIMIT_BYTES) {
        fs::remove_file(&path).ok();
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        writeln!(f, "{}", line).ok();
    }
}

/// The only way a failed start can be reported: the console is gone by then.
pub fn log_fatal(message: &str) {
    log(&format!("fatal: {}", message));
}

fn invalidate_interface() {
    if let Ok(mut c) = EGRESS_CACHE.lock() {
        *c = None;
    }
}

fn isp_interface() -> u32 {
    if let Ok(cache) = EGRESS_CACHE.lock() {
        if let Some((idx, at)) = *cache {
            if at.elapsed() < EGRESS_TTL {
                return idx;
            }
        }
    }
    // 0 means "use the routing table" - the right degradation when there is no
    // physical egress to name.
    let idx = egress::detect().map_or(0, |e| e.if_index);
    if let Ok(mut cache) = EGRESS_CACHE.lock() {
        *cache = Some((idx, Instant::now()));
    }
    idx
}

fn relay(query: &[u8]) -> Option<Vec<u8>> {
    let if_index = isp_interface();
    for server in egress::NS_V4 {
        let Ok(ip) = server.parse::<Ipv4Addr>() else {
            continue;
        };
        if let Ok(reply) = dns_client::query_raw_via(query, ip, if_index, UPSTREAM_TIMEOUT) {
            return Some(reply);
        }
    }
    // Both resolvers silent: the interface may have gone away under us, so the
    // next query re-detects instead of retrying a dead one.
    invalidate_interface();
    None
}

/// Runs until killed. Never returns `Ok` - the only way out is a bind failure,
/// which is worth reporting because it means something else holds the address.
pub fn run() -> Result<(), String> {
    let sock = UdpSocket::bind((LISTEN_IP, LISTEN_PORT))
        .map_err(|e| format!("не удалось занять {}:{} — {}", LISTEN_IP, LISTEN_PORT, e))?;
    log(&format!("start: {}:{}", LISTEN_IP, LISTEN_PORT));

    let mut buf = [0u8; 4096];
    loop {
        let (n, from) = match sock.recv_from(&mut buf) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // Anything shorter than a header is not a query worth relaying.
        if n < 12 {
            continue;
        }
        let query = buf[..n].to_vec();
        let out = match sock.try_clone() {
            Ok(s) => s,
            Err(_) => continue,
        };
        // One thread per query: the volume is a handful per minute, and a slow
        // upstream must not stall the queries behind it.
        thread::spawn(move || {
            let name = dns_client::question_name(&query).unwrap_or_else(|| "?".to_string());
            match relay(&query) {
                Some(reply) => {
                    out.send_to(&reply, from).ok();
                    log(&format!("ok   {}", name));
                }
                None => log(&format!("fail {}", name)),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listener_address_is_loopback() {
        let ip: Ipv4Addr = LISTEN_IP.parse().expect("valid address");
        assert!(ip.is_loopback());
        // Not .1: something else on the machine may already own it.
        assert_ne!(ip, Ipv4Addr::LOCALHOST);
    }

    #[test]
    fn a_detected_interface_is_cached_and_droppable() {
        invalidate_interface();
        assert!(EGRESS_CACHE.lock().unwrap().is_none());
        if let Ok(mut c) = EGRESS_CACHE.lock() {
            *c = Some((17, Instant::now()));
        }
        assert_eq!(isp_interface(), 17);
        invalidate_interface();
        assert!(EGRESS_CACHE.lock().unwrap().is_none());
    }

    /// Relays a real query end to end through the running upstream. Needs a live
    /// network; the point is that raw bytes in produce a usable answer out.
    #[test]
    #[ignore = "needs a live network; run with --ignored"]
    fn relays_a_real_query() {
        let id: u16 = 0x4242;
        let mut query = vec![];
        query.extend_from_slice(&id.to_be_bytes());
        query.extend_from_slice(&[0x01, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
        for label in "cloudcode-pa.googleapis.com".split('.') {
            query.push(label.len() as u8);
            query.extend_from_slice(label.as_bytes());
        }
        query.extend_from_slice(&[0, 0x00, 0x01, 0x00, 0x01]);

        let reply = relay(&query).expect("upstream answered");
        assert_eq!(&reply[0..2], &id.to_be_bytes(), "id must be echoed");
        let answers = u16::from_be_bytes([reply[6], reply[7]]);
        println!("reply {} bytes, {} answers", reply.len(), answers);
        assert!(answers > 0, "the reply carries no answer");
        assert_eq!(
            dns_client::question_name(&reply).as_deref(),
            Some("cloudcode-pa.googleapis.com"),
            "the reply must answer the question we asked"
        );
    }
}
