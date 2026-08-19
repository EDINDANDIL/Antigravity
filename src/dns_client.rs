use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

// A DNS query that is guaranteed to leave through a chosen interface.
//
// The routing table cannot deliver that: a VPN client holds its own /32 for the
// resolver addresses at a metric nothing legal can beat, and its tunnel service
// restores the route as fast as it is deleted. IP_UNICAST_IF names the outgoing
// interface on the socket itself and skips the route lookup, which is why this
// works while the tunnel is up. Measured: through the tunnel xbox-dns answers
// 172.217.114.4 (genuine Google), through the ISP link 87.228.47.204 (proxy).
//
// Resolve-DnsName has no interface parameter, so the query is assembled by hand.
// Only A records are needed - the answers are pinned into `hosts`, which is IPv4
// and IPv6 literals only, no CNAME chains.

const DNS_PORT: u16 = 53;
const TIMEOUT: Duration = Duration::from_secs(5);

/// Transaction IDs only have to differ between concurrent queries; a counter is
/// enough and keeps the crate free of a random-number dependency.
static NEXT_ID: AtomicU16 = AtomicU16::new(0x1234);

#[cfg(target_os = "windows")]
mod sys {
    const IPPROTO_IP: i32 = 0;
    const IP_UNICAST_IF: i32 = 31;

    #[link(name = "ws2_32")]
    extern "system" {
        fn setsockopt(s: usize, level: i32, optname: i32, optval: *const u8, optlen: i32) -> i32;
    }

    /// Forces unicast traffic out of `if_index`. Index 0 restores the default,
    /// i.e. "let the routing table decide".
    pub fn bind_socket_to_interface(
        sock: &std::net::UdpSocket,
        if_index: u32,
    ) -> Result<(), String> {
        use std::os::windows::io::AsRawSocket;
        // IP_UNICAST_IF takes the index in network byte order - the one Winsock
        // quirk in this call, and a silent no-op if you get it wrong.
        let value: u32 = if_index.to_be();
        let rc = unsafe {
            setsockopt(
                sock.as_raw_socket() as usize,
                IPPROTO_IP,
                IP_UNICAST_IF,
                &value as *const u32 as *const u8,
                std::mem::size_of::<u32>() as i32,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err("не удалось привязать сокет к интерфейсу".to_string())
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod sys {
    pub fn bind_socket_to_interface(_: &std::net::UdpSocket, _: u32) -> Result<(), String> {
        Err("поддерживается только на Windows".to_string())
    }
}

fn build_query(name: &str, id: u16) -> Vec<u8> {
    let mut q = Vec::with_capacity(name.len() + 18);
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00]); // standard query, recursion desired
    q.extend_from_slice(&[0x00, 0x01]); // one question
    q.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // no answer/authority/additional
    for label in name.split('.').filter(|l| !l.is_empty()) {
        let len = label.len().min(63);
        q.push(len as u8);
        q.extend_from_slice(&label.as_bytes()[..len]);
    }
    q.push(0);
    q.extend_from_slice(&[0x00, 0x01]); // A
    q.extend_from_slice(&[0x00, 0x01]); // IN
    q
}

/// Steps over a name without expanding it. Names are variable length and may end
/// in a compression pointer, which is why this cannot be a fixed offset.
fn skip_name(buf: &[u8], mut i: usize) -> Option<usize> {
    loop {
        let len = *buf.get(i)? as usize;
        if len == 0 {
            return Some(i + 1);
        }
        if len & 0xC0 == 0xC0 {
            // A pointer is always the last thing in a name.
            return if i + 1 < buf.len() { Some(i + 2) } else { None };
        }
        i += 1 + len;
    }
}

/// Every A record in the answer section. Anything malformed ends the walk
/// instead of panicking - this parses bytes from a third-party resolver.
fn parse_a_records(buf: &[u8], expect_id: u16) -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    if buf.len() < 12 || u16::from_be_bytes([buf[0], buf[1]]) != expect_id {
        return out;
    }
    let answers = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    let questions = u16::from_be_bytes([buf[4], buf[5]]) as usize;

    let mut i = 12;
    for _ in 0..questions {
        match skip_name(buf, i) {
            Some(next) => i = next + 4, // qtype + qclass
            None => return out,
        }
    }

    for _ in 0..answers {
        i = match skip_name(buf, i) {
            Some(next) => next,
            None => return out,
        };
        if i + 10 > buf.len() {
            return out;
        }
        let rtype = u16::from_be_bytes([buf[i], buf[i + 1]]);
        let rdlen = u16::from_be_bytes([buf[i + 8], buf[i + 9]]) as usize;
        i += 10;
        if i + rdlen > buf.len() {
            return out;
        }
        if rtype == 1 && rdlen == 4 {
            out.push(Ipv4Addr::new(buf[i], buf[i + 1], buf[i + 2], buf[i + 3]));
        }
        i += rdlen;
    }
    out
}

/// The question name of a query, for logging. Cheap enough to run per packet
/// and never fails loudly - a name we cannot read is simply not logged.
pub fn question_name(buf: &[u8]) -> Option<String> {
    if buf.len() < 12 {
        return None;
    }
    let mut i = 12;
    let mut name = String::new();
    loop {
        let len = *buf.get(i)? as usize;
        if len == 0 {
            break;
        }
        if len & 0xC0 == 0xC0 {
            return None; // a question name is never compressed
        }
        let label = buf.get(i + 1..i + 1 + len)?;
        if !name.is_empty() {
            name.push('.');
        }
        name.push_str(&String::from_utf8_lossy(label));
        i += 1 + len;
    }
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Sends `packet` verbatim and hands back the raw reply, forcing the query out
/// of `if_index` (0 = let the routing table pick). The forwarder relays bytes it
/// never parses, so this stays transport-only: any record type, EDNS included,
/// passes through untouched.
pub fn query_raw_via(
    packet: &[u8],
    server: Ipv4Addr,
    if_index: u32,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let sock = UdpSocket::bind(("0.0.0.0", 0)).map_err(|_| "нет UDP-сокета".to_string())?;
    if if_index != 0 {
        sys::bind_socket_to_interface(&sock, if_index)?;
    }
    sock.set_read_timeout(Some(timeout)).ok();
    sock.send_to(packet, SocketAddr::from((server, DNS_PORT)))
        .map_err(|_| "запрос не отправлен".to_string())?;

    let want_id = packet.get(0..2).map(|b| [b[0], b[1]]);
    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 4096];
    loop {
        let (n, from) = sock
            .recv_from(&mut buf)
            .map_err(|_| "резолвер не ответил".to_string())?;
        // A stray packet must not end the wait: keep reading until the deadline
        // for one that came from the resolver and answers the id we sent.
        let right_source = from.ip() == IpAddr::V4(server);
        let right_id = match (want_id, n >= 12) {
            (Some(id), true) => buf[0..2] == id,
            _ => false,
        };
        if right_source && right_id {
            return Ok(buf[..n].to_vec());
        }
        if Instant::now() >= deadline {
            return Err("резолвер не ответил".to_string());
        }
    }
}

/// Asks `server` for the A records of `host`, forcing the query out of
/// `if_index`. Pass 0 to let the routing table pick - useful for comparing what
/// the same resolver answers on the two paths.
pub fn resolve_a_via(host: &str, server: Ipv4Addr, if_index: u32) -> Result<Vec<Ipv4Addr>, String> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let reply = query_raw_via(&build_query(host, id), server, if_index, TIMEOUT)?;
    Ok(parse_a_records(&reply, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_carries_the_name_as_labels() {
        let q = build_query("a.example", 0xBEEF);
        assert_eq!(&q[0..2], &[0xBE, 0xEF]);
        assert_eq!(u16::from_be_bytes([q[4], q[5]]), 1); // one question
        assert_eq!(&q[12..], b"\x01a\x07example\x00\x00\x01\x00\x01");
    }

    #[test]
    fn empty_labels_are_dropped() {
        // A trailing dot is a legal way to write an absolute name.
        assert_eq!(build_query("a.example.", 1), build_query("a.example", 1));
    }

    /// Header + question, then `count` A answers that use a compression pointer
    /// for the name, exactly as a real resolver replies.
    fn response(id: u16, addrs: &[[u8; 4]]) -> Vec<u8> {
        let mut b = vec![];
        b.extend_from_slice(&id.to_be_bytes());
        b.extend_from_slice(&[0x81, 0x80]);
        b.extend_from_slice(&[0x00, 0x01]);
        b.extend_from_slice(&(addrs.len() as u16).to_be_bytes());
        b.extend_from_slice(&[0, 0, 0, 0]);
        b.extend_from_slice(b"\x01a\x07example\x00");
        b.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);
        for a in addrs {
            b.extend_from_slice(&[0xC0, 0x0C]); // pointer back to the question
            b.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);
            b.extend_from_slice(&[0, 0, 0, 60]);
            b.extend_from_slice(&[0x00, 0x04]);
            b.extend_from_slice(a);
        }
        b
    }

    #[test]
    fn reads_every_a_record() {
        let msg = response(7, &[[1, 2, 3, 4], [87, 228, 47, 204]]);
        assert_eq!(
            parse_a_records(&msg, 7),
            vec![Ipv4Addr::new(1, 2, 3, 4), Ipv4Addr::new(87, 228, 47, 204)]
        );
    }

    #[test]
    fn a_reply_to_another_query_is_ignored() {
        let msg = response(7, &[[1, 2, 3, 4]]);
        assert!(parse_a_records(&msg, 8).is_empty());
    }

    #[test]
    fn truncated_input_does_not_panic() {
        let msg = response(7, &[[1, 2, 3, 4]]);
        for cut in 0..msg.len() {
            let _ = parse_a_records(&msg[..cut], 7);
        }
    }

    #[test]
    fn a_record_count_larger_than_the_body_is_survivable() {
        let mut msg = response(7, &[[1, 2, 3, 4]]);
        msg[6] = 0x00;
        msg[7] = 0x40; // claim 64 answers, ship one
        assert_eq!(parse_a_records(&msg, 7), vec![Ipv4Addr::new(1, 2, 3, 4)]);
    }

    #[test]
    fn non_a_records_are_skipped() {
        let mut b = vec![];
        b.extend_from_slice(&7u16.to_be_bytes());
        b.extend_from_slice(&[0x81, 0x80, 0x00, 0x01, 0x00, 0x02, 0, 0, 0, 0]);
        b.extend_from_slice(b"\x01a\x07example\x00");
        b.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);
        // CNAME first, then the A record it points at.
        b.extend_from_slice(&[0xC0, 0x0C, 0x00, 0x05, 0x00, 0x01, 0, 0, 0, 60, 0x00, 0x02]);
        b.extend_from_slice(&[0xC0, 0x0C]);
        b.extend_from_slice(&[0xC0, 0x0C, 0x00, 0x01, 0x00, 0x01, 0, 0, 0, 60, 0x00, 0x04]);
        b.extend_from_slice(&[9, 9, 9, 9]);
        assert_eq!(parse_a_records(&b, 7), vec![Ipv4Addr::new(9, 9, 9, 9)]);
    }
}
