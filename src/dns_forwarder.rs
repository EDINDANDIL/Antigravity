use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use crate::dns_client;
use crate::egress;
use crate::resolvers::{self, Verdict};

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

/// Log prefix for what the answer turned out to be. `substituted` is the only
/// one that means the region gate is actually being defeated for that name;
/// `PASSTHROUGH` shouts because it is the failure the old `ok` used to hide.
fn verdict_tag(v: Verdict) -> &'static str {
    match v {
        Verdict::Substituted => "substituted",
        Verdict::Sibling => "sibling",
        Verdict::Passthrough => "PASSTHROUGH",
        Verdict::Unknown => "ok",
    }
}

/// How long a *detected* interface is trusted. Long on purpose: detection
/// shells out to PowerShell, and an interface that dies is caught by
/// `invalidate_interface()` on the first failed relay, so re-probing on a timer
/// buys nothing and only spawns processes on the user's machine.
const EGRESS_TTL: Duration = Duration::from_secs(30 * 60);
/// How long a *failed* detection is remembered. Short, because it is usually
/// the network not being up yet at logon - but not zero, or a machine with no
/// physical egress at all would spawn a probe per query.
const EGRESS_RETRY: Duration = Duration::from_secs(30);
const LOG_LIMIT_BYTES: u64 = 64 * 1024;

/// Interface index, when it was learned, and how long that answer is good for.
static EGRESS_CACHE: Mutex<Option<(u32, Instant, Duration)>> = Mutex::new(None);

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
        if let Some((idx, at, good_for)) = *cache {
            if at.elapsed() < good_for {
                return idx;
            }
        }
    }
    // 0 means "use the routing table" - the right degradation when there is no
    // physical egress to name, but a poor thing to remember for long: at logon
    // the network is often simply not up yet, and caching the miss would leave
    // half an hour of unsubstituted answers.
    let (idx, good_for) = match egress::detect() {
        Some(eg) => (eg.if_index, EGRESS_TTL),
        None => (0, EGRESS_RETRY),
    };
    if let Ok(mut cache) = EGRESS_CACHE.lock() {
        *cache = Some((idx, Instant::now(), good_for));
    }
    idx
}

/// Relays one query and reports which provider answered and whether that answer
/// was actually substituted.
///
/// The choice is no longer a constant. A provider can drop a name from its
/// list without any error - the query still resolves, just to the genuine
/// Google address - so every provider is asked at once and compared against a
/// reference resolver that substitutes nothing. See `resolvers`.
fn relay(query: &[u8]) -> Option<(Vec<u8>, &'static str, resolvers::Verdict)> {
    let if_index = isp_interface();
    match resolvers::resolve_best(query, if_index) {
        Some(hit) => Some(hit),
        None => {
            // Nobody answered: the interface may have gone away under us, so
            // the next query re-detects instead of retrying a dead one.
            invalidate_interface();
            None
        }
    }
}

/// Runs until killed. Never returns `Ok` - the only way out is a bind failure,
/// which is worth reporting because it means something else holds the address.
pub fn run() -> Result<(), String> {
    let sock = UdpSocket::bind((LISTEN_IP, LISTEN_PORT))
        .map_err(|e| format!("не удалось занять {}:{} — {}", LISTEN_IP, LISTEN_PORT, e))?;
    log(&format!("start: {}:{}", LISTEN_IP, LISTEN_PORT));
    // Detect once up front. Otherwise the first query pays for a cold probe,
    // which is long enough that Windows gives up on us and falls back to the
    // direct resolvers - and then caches that unsubstituted answer for its full
    // TTL, so one slow startup is felt for minutes.
    log(&format!("egress: if{}", isp_interface()));

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
                Some((reply, provider, verdict)) => {
                    out.send_to(&reply, from).ok();
                    // The verdict is the part worth logging: "ok" used to mean
                    // only that bytes came back, which is precisely what it
                    // still said while the answers had stopped being
                    // substituted.
                    log(&format!(
                        "{:<12} {} [{}]",
                        verdict_tag(verdict),
                        name,
                        provider
                    ));
                }
                None => log(&format!("fail         {}", name)),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

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
            *c = Some((17, Instant::now(), EGRESS_TTL));
        }
        // Served from the cache: no detection runs, so no process is spawned.
        assert_eq!(isp_interface(), 17);
        invalidate_interface();
        assert!(EGRESS_CACHE.lock().unwrap().is_none());
    }

    /// An expired entry must not be served - that is what makes the short retry
    /// after a failed detection actually retry.
    #[test]
    fn an_expired_entry_is_not_served() {
        invalidate_interface();
        if let Ok(mut c) = EGRESS_CACHE.lock() {
            // Learned long ago, and only ever good for a moment.
            *c = Some((17, Instant::now() - Duration::from_secs(60), EGRESS_RETRY));
        }
        let stale = EGRESS_CACHE
            .lock()
            .unwrap()
            .map(|(_, at, good_for)| at.elapsed() >= good_for);
        assert_eq!(stale, Some(true));
        invalidate_interface();
    }

    /// A failed detection has to be forgotten quickly: at logon it usually just
    /// means the network is not up yet.
    #[test]
    fn a_failed_detection_is_remembered_only_briefly() {
        assert!(EGRESS_RETRY < EGRESS_TTL);
        assert!(EGRESS_RETRY <= Duration::from_secs(60));
    }

    /// Relays a real query end to end through the running upstream, and prints
    /// the verdict for every routed name.
    ///
    /// Needs a live network and the VPN OFF: through a tunnel every provider
    /// sees a foreign client and substitutes nothing, so every verdict comes
    /// back `Passthrough` and the run says nothing about the providers.
    ///
    /// The verdict is the assertion that matters now. The old version only
    /// checked that bytes came back - which stayed true through the whole
    /// outage that motivated `resolvers`, because an unsubstituted answer is
    /// still a perfectly well-formed answer.
    #[test]
    #[ignore = "needs a live network, VPN off; run with --ignored"]
    fn relays_a_real_query() {
        let id: u16 = 0x4242;
        let mut substituted = Vec::new();

        for name in [
            "cloudcode-pa.googleapis.com",
            "daily-cloudcode-pa.googleapis.com",
            "generativelanguage.googleapis.com",
            "antigravity-unleash.goog",
        ] {
            let mut query = vec![];
            query.extend_from_slice(&id.to_be_bytes());
            query.extend_from_slice(&[0x01, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
            for label in name.split('.') {
                query.push(label.len() as u8);
                query.extend_from_slice(label.as_bytes());
            }
            query.extend_from_slice(&[0, 0x00, 0x01, 0x00, 0x01]);

            let (reply, provider, verdict) = relay(&query).expect("a provider answered");
            assert_eq!(&reply[0..2], &id.to_be_bytes(), "id must be echoed");
            assert_eq!(
                dns_client::question_name(&reply).as_deref(),
                Some(name),
                "the reply must answer the question we asked"
            );
            assert!(
                u16::from_be_bytes([reply[6], reply[7]]) > 0,
                "{} came back with no answer",
                name
            );
            println!(
                "{:<38} {:?} via {:<12} {:?}",
                name,
                verdict,
                provider,
                dns_client::answer_addrs(&reply)
            );
            if verdict == Verdict::Substituted {
                substituted.push(name);
            }
        }

        // Not an assertion on any single name: which names a provider proxies
        // is theirs to change, and this test exists to report that, not to fail
        // on it. But if nothing at all is substituted, either the VPN is up or
        // every provider has dropped the whole list - both worth failing on.
        assert!(
            !substituted.is_empty(),
            "no routed name is substituted by any provider - VPN up, or the \
             providers dropped every name"
        );
        println!("substituted: {:?}", substituted);
    }
}
