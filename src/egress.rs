use crate::utils::powershell;

// Finding the ISP-facing interface, and why that is the whole job here.
//
// With a full-tunnel VPN up, the NRPT rules resolve to nothing useful: xbox-dns
// substitutes addresses only for clients it geolocates to a blocked region, and
// through the tunnel it sees a foreign address and forwards the genuine Google
// one. The region gate then answers 400 as if the unlocker were not installed.
//
// Routing the resolver addresses back onto the ISP link does not work either,
// and this is worth remembering rather than rediscovering: AmneziaVPN already
// holds its own /32 for exactly those addresses. Equal prefix length hands the
// decision to RouteMetric + InterfaceMetric, where the tunnel's 5 beats
// Ethernet's 26; no legal RouteMetric closes that gap, the only other lever -
// the interface metric - would drag every other route off the VPN with it, and
// the WireGuard tunnel service restores its route as fast as it is deleted.
//
// What does work is not touching the routing table at all: `dns_client` names
// the outgoing interface on the socket via IP_UNICAST_IF, which skips the route
// lookup entirely. Verified against a live tunnel - the same resolver answers
// 172.217.119.4 (genuine Google) through the tunnel and 87.228.47.204 (proxy)
// through the ISP link, and a query to an address nothing holds a route for
// still leaves through the named interface. So this module only has to say
// which interface that is; `hosts_pin` does the rest.

/// xbox-dns.ru resolvers. Single source of truth - `dns.rs` builds the NRPT
/// nameserver list from these and resolves through the first one directly.
pub const NS_V4: &[&str] = &["111.88.96.50", "111.88.96.51"];
pub const NS_V6: &[&str] = &["2a00:ab00:1233:26::50", "2a00:ab00:1233:26::51"];

/// Prefixes an earlier build pinned to the physical adapter. They are harmless
/// but pointless, and the persistent ones outlive the tool, so cleanup drops
/// them. Remove this once no installed build can still have written them.
const LEGACY_PINNED_PREFIXES: &[&str] = &[
    "111.88.96.50/32",
    "111.88.96.51/32",
    "2a00:ab00:1233:26::50/128",
    "2a00:ab00:1233:26::51/128",
];

#[derive(Debug, Clone)]
pub struct Egress {
    pub if_index: u32,
    /// True when some non-physical adapter also holds a default route - i.e. a
    /// tunnel is up and the NRPT path alone cannot work.
    pub vpn_active: bool,
}

/// `ifIndex|gateway|vpn`. The gateway itself is not kept - nothing routes any
/// more - but an interface without one is not an internet-facing link, so its
/// presence still decides whether the line describes a usable egress.
fn parse_egress(line: &str) -> Option<Egress> {
    let mut parts = line.trim().split('|');
    let if_index = parts.next()?.trim().parse::<u32>().ok()?;
    let gateway = parts.next()?.trim();
    let vpn = parts.next()?.trim();

    if gateway.is_empty() || gateway == "-" {
        return None;
    }
    Some(Egress {
        if_index,
        vpn_active: vpn.eq_ignore_ascii_case("true"),
    })
}

/// The default route that belongs to real hardware. `Get-NetAdapter -Physical`
/// drops WireGuard/TAP tunnels and the Hyper-V/VMware virtual switches in one
/// go, and requiring a real next hop drops host-only adapters, which carry a
/// gateway address but no default route.
pub fn detect() -> Option<Egress> {
    const SCRIPT: &str = "\
$phys=@(Get-NetAdapter -Physical -ErrorAction SilentlyContinue | \
  Where-Object {$_.Status -eq 'Up'} | ForEach-Object {$_.ifIndex}); \
$def=@(Get-NetRoute -DestinationPrefix '0.0.0.0/0' -PolicyStore ActiveStore -ErrorAction SilentlyContinue); \
$mine=@($def | Where-Object {$phys -contains $_.ifIndex -and $_.NextHop -ne '0.0.0.0'}); \
if ($mine.Count -eq 0) { 'none' } else { \
  $best=$mine | Sort-Object {[int]$_.RouteMetric + \
    [int](Get-NetIPInterface -InterfaceIndex $_.ifIndex -AddressFamily IPv4 \
      -ErrorAction SilentlyContinue).InterfaceMetric} | Select-Object -First 1; \
  $vpn=@($def | Where-Object {$phys -notcontains $_.ifIndex}).Count -gt 0; \
  '{0}|{1}|{2}' -f $best.ifIndex,$best.NextHop,$vpn }";

    let out = powershell(SCRIPT)?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .and_then(parse_egress)
}

/// Drops the host routes an earlier build pinned. Deleted per interface rather
/// than by prefix alone, so a route stranded on an adapter the machine no longer
/// uses goes too; netsh cleans up whatever the cmdlet could not.
pub fn remove_legacy_routes() {
    let list = LEGACY_PINNED_PREFIXES
        .iter()
        .map(|p| format!("'{}'", p))
        .collect::<Vec<_>>()
        .join(",");
    let cmd = format!(
        "foreach ($d in @({})) {{ \
           $fam=$(if ($d -like '*:*') {{'ipv6'}} else {{'ipv4'}}); \
           foreach ($s in @('ActiveStore','PersistentStore')) {{ \
             $st=$(if ($s -eq 'ActiveStore') {{'active'}} else {{'persistent'}}); \
             foreach ($r in @(Get-NetRoute -DestinationPrefix $d -PolicyStore $s \
                 -ErrorAction SilentlyContinue)) {{ \
               Remove-NetRoute -DestinationPrefix $d -InterfaceIndex $r.ifIndex \
                 -PolicyStore $s -Confirm:$false -ErrorAction SilentlyContinue; \
               if (@(Get-NetRoute -DestinationPrefix $d -PolicyStore $s -ErrorAction SilentlyContinue | \
                   Where-Object {{$_.ifIndex -eq $r.ifIndex}}).Count -gt 0) {{ \
                 netsh interface $fam delete route prefix=$d interface=$($r.ifIndex) store=$st | Out-Null }} }} }} }}",
        list
    );
    powershell(&cmd);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dns_client;
    use std::net::Ipv4Addr;

    #[test]
    fn egress_line_is_parsed() {
        let eg = parse_egress("17|192.168.0.1|True").expect("parses");
        assert_eq!(eg.if_index, 17);
        assert!(eg.vpn_active);

        let plain = parse_egress(" 12|10.0.0.1|False ").expect("parses");
        assert!(!plain.vpn_active);
    }

    #[test]
    fn egress_line_rejects_garbage() {
        assert!(parse_egress("none").is_none());
        assert!(parse_egress("").is_none());
        assert!(parse_egress("17|-|False").is_none());
        assert!(parse_egress("17|192.168.0.1").is_none());
    }

    /// The one thing unit tests cannot cover: that the socket really leaves
    /// through the interface we named. Needs a live network, and only says
    /// something with a VPN up - that is when the two answers must differ.
    #[test]
    #[ignore = "needs a live network; run with --ignored"]
    fn resolves_past_the_tunnel() {
        let eg = detect().expect("physical egress");
        let server: Ipv4Addr = NS_V4[0].parse().unwrap();
        let host = "cloudcode-pa.googleapis.com";
        let isp = dns_client::resolve_a_via(host, server, eg.if_index);
        let tunnelled = dns_client::resolve_a_via(host, server, 0);
        println!("egress if{} (vpn: {})", eg.if_index, eg.vpn_active);
        println!("  via ISP:     {:?}", isp);
        println!("  via default: {:?}", tunnelled);
        assert!(
            isp.as_ref().map_or(false, |a| !a.is_empty()),
            "no answer over the ISP link"
        );
        if eg.vpn_active {
            assert_ne!(
                isp.unwrap(),
                tunnelled.unwrap(),
                "the tunnel was not bypassed"
            );
        }
    }

    /// Forcing the interface must work for a destination nothing holds a route
    /// for - that is what makes the host-route pinning unnecessary.
    #[test]
    #[ignore = "needs a live network; run with --ignored"]
    fn unicast_if_needs_no_host_route() {
        let eg = detect().expect("physical egress");
        let unrouted: Ipv4Addr = "1.1.1.1".parse().unwrap();
        let answer = dns_client::resolve_a_via("github.com", unrouted, eg.if_index);
        println!("via if{} to 1.1.1.1: {:?}", eg.if_index, answer);
        assert!(
            answer.as_ref().map_or(false, |a| !a.is_empty()),
            "IP_UNICAST_IF needs a host route after all"
        );
    }
}
