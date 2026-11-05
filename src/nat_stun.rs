//! STUN recheck for the TCP-only NAT classification (feat/udp-punch US-004).
//!
//! `test_nat_type_` (src/common.rs) classifies the NAT by comparing the source
//! ports observed on two TCP connections to hbbs 21116/21115. A mapping that is
//! endpoint-independent for UDP (cone) can still differ between those two TCP
//! flows (per-destination TCP mappings are common on CN ISP CPEs), and the
//! resulting `SYMMETRIC` label makes the peer's `handle_punch_hole` go straight
//! to relay (`relay_v4`) — no punch is even attempted.
//!
//! UDP punching only cares about the UDP mapping, so when the TCP test says
//! `SYMMETRIC` this module re-checks it over real STUN UDP probes from one
//! socket: two independent servers, one Binding Request each. Equal mapped
//! ports ⇒ endpoint-independent ⇒ downgrade to `ASYMMETRIC` (RustDesk's name
//! for cone) so the punch path stays alive. Ports that differ, or any probe
//! failure, keep the original classification — behaviour identical to before.
//!
//! Budget-capped (TOTAL_BUDGET_MS) and best-effort: every failure falls back to
//! the TCP result, and the option `enable-stun-recheck` can turn it off.

use base::config::keys;
use hbb_common::{
    config::{Config, LocalConfig},
    log,
    tokio::{self, time::timeout},
};
use std::time::Duration;

const TOTAL_BUDGET_MS: u64 = 3_000;
const PROBE_TIMEOUT_MS: u64 = 800;
/// Endpoint-independent mapping needs at least two observed mappings to agree.
const DEFAULT_STUN_SERVERS: &[&str] = &["stun.miwifi.com:3478", "stun.chat.bilibili.com:3478"];

const BINDING_REQUEST: u16 = 0x0001;
const BINDING_RESPONSE: u16 = 0x0101;
const MAGIC_COOKIE: u32 = 0x2112A442;
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;
const ATTR_MAPPED_ADDRESS: u16 = 0x0001;

fn enabled() -> bool {
    let v = LocalConfig::get_option(keys::OPTION_ENABLE_STUN_RECHECK);
    // Default on: only an explicit disable turns it off.
    !matches!(v.to_lowercase().as_str(), "n" | "0" | "off" | "false")
}

/// Sends one Binding Request to each server from the SAME socket and returns
/// the mapped ports. Endpoint-independence can only be judged on one local
/// socket: separate sockets get separate NAT mappings by design, which would
/// make every NAT look symmetric.
async fn mapped_ports_same_socket(servers: &[String]) -> Vec<u16> {
    let mut ports = Vec::new();
    let Ok(socket) = tokio::net::UdpSocket::bind("0.0.0.0:0").await else {
        log::info!("stun recheck: bind failed");
        return ports;
    };
    for server in servers {
        let Some(addr) = tokio::net::lookup_host(server)
            .await
            .ok()
            .and_then(|mut i| i.find(|a| a.is_ipv4()))
        else {
            log::info!("stun recheck: {} unresolvable", server);
            continue;
        };
        let mut tid = [0u8; 12];
        getrandom(&mut tid);
        let req = binding_request(&tid);
        if socket.send_to(&req, addr).await.is_err() {
            log::info!("stun recheck: {} send failed", server);
            continue;
        }
        // Recv until the matching transaction id arrives (a late reply to the
        // previous server's request can land first).
        let deadline = std::time::Instant::now() + Duration::from_millis(PROBE_TIMEOUT_MS);
        loop {
            let mut buf = vec![0u8; 1500];
            let left = match deadline.checked_duration_since(std::time::Instant::now()) {
                Some(d) => d,
                None => break,
            };
            let Ok(n) = timeout(left, socket.recv(&mut buf)).await else {
                break;
            };
            let Some(port) = n.ok().and_then(|n| parse_mapped_port(&buf[..n], &tid)) else {
                continue;
            };
            ports.push(port);
            break;
        }
    }
    ports
}

fn binding_request(tid: &[u8; 12]) -> [u8; 20] {
    let mut req = [0u8; 20];
    req[..2].copy_from_slice(&BINDING_REQUEST.to_be_bytes());
    // length 0, magic cookie, transaction id
    req[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
    req[8..20].copy_from_slice(tid);
    req
}

/// Extracts the mapped port from a Binding Response, walking TLVs (some servers
/// append extra attributes after XOR-MAPPED-ADDRESS).
fn parse_mapped_port(data: &[u8], tid: &[u8; 12]) -> Option<u16> {
    if data.len() < 20 {
        return None;
    }
    let msg_type = u16::from_be_bytes([data[0], data[1]]);
    if msg_type != BINDING_RESPONSE {
        return None;
    }
    if u32::from_be_bytes([data[4], data[5], data[6], data[7]]) != MAGIC_COOKIE {
        return None;
    }
    if data[8..20] != *tid {
        return None;
    }
    let msg_len = u16::from_be_bytes([data[2], data[3]]) as usize;
    let end = (20 + msg_len).min(data.len());
    let mut i = 20;
    while i + 4 <= end {
        let attr = u16::from_be_bytes([data[i], data[i + 1]]);
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        let body_end = (i + 4 + len).min(end);
        if attr == ATTR_XOR_MAPPED_ADDRESS && len >= 8 {
            // family u8, xport u16 (port ^ top 16 bits of cookie), xaddr 4 bytes
            let xport = u16::from_be_bytes([data[i + 6], data[i + 7]]);
            return Some(xport ^ (MAGIC_COOKIE >> 16) as u16);
        } else if attr == ATTR_MAPPED_ADDRESS && len >= 8 {
            let family = data[i + 4];
            if family == 0x01 {
                return Some(u16::from_be_bytes([data[i + 6], data[i + 7]]));
            }
        }
        i = body_end;
    }
    None
}

fn getrandom(buf: &mut [u8]) {
    // hbb_common already re-exports sodiumoxide's dep; simplest local fill
    // without a new dependency: hash-like mixing of a rising counter is NOT
    // cryptographic, but the transaction id only needs uniqueness, not
    // unpredictability — std::time::nanos + address entropy suffices for a
    // 12-byte STUN tid.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut state = nanos as u64 ^ (buf.as_ptr() as u64) ^ 0x9E3779B97F4A7C15;
    for b in buf.iter_mut() {
        // xorshift64*
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        *b = (state.wrapping_mul(0x2545F4914F6CDD1D) >> 32) as u8;
    }
}

/// Re-checks a `SYMMETRIC` verdict with real STUN probes and downgrades it to
/// `ASYMMETRIC` when the UDP mapping is endpoint-independent. Any other input
/// verdict, disabled option, or probe failure returns the input unchanged.
pub async fn recheck(t: NatTypeVerdict) -> NatTypeVerdict {
    if t != NatTypeVerdict::Symmetric || !enabled() {
        return t;
    }
    let start = std::time::Instant::now();
    let servers: Vec<String> = DEFAULT_STUN_SERVERS.iter().map(|s| s.to_string()).collect();
    let ports = timeout(
        Duration::from_millis(TOTAL_BUDGET_MS),
        mapped_ports_same_socket(&servers),
    )
    .await
    .unwrap_or_default();
    if ports.len() < 2 {
        log::info!(
            "stun recheck: insufficient probes ({:?}), keep SYMMETRIC",
            ports
        );
        return t;
    }
    let all_same = ports.iter().all(|p| *p == ports[0]);
    log::info!(
        "stun recheck: ports={:?} all_same={} in {:?}",
        ports,
        all_same,
        start.elapsed()
    );
    if all_same {
        NatTypeVerdict::Asymmetric
    } else {
        NatTypeVerdict::Symmetric
    }
}

/// Mirrors `rendezvous_proto::NatType` so this module stays dependency-light
/// and unit-testable without protobuf codegen.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum NatTypeVerdict {
    Asymmetric,
    Symmetric,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xor_resp(port: u16, tid: [u8; 12]) -> Vec<u8> {
        let mut out = vec![0u8; 20];
        out[..2].copy_from_slice(&BINDING_RESPONSE.to_be_bytes());
        out[2..4].copy_from_slice(&8u16.to_be_bytes());
        out[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
        out[8..20].copy_from_slice(&tid);
        // XOR-MAPPED-ADDRESS: tag 0x0020, len 8, reserved 0, family 1
        out.extend_from_slice(&ATTR_XOR_MAPPED_ADDRESS.to_be_bytes());
        out.extend_from_slice(&8u16.to_be_bytes());
        out.push(0);
        out.push(0x01);
        let xport = port ^ (MAGIC_COOKIE >> 16) as u16;
        out.extend_from_slice(&xport.to_be_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out
    }

    #[test]
    fn test_xor_mapped_port() {
        let tid = [7u8; 12];
        let resp = xor_resp(53_121, tid);
        assert_eq!(parse_mapped_port(&resp, &tid), Some(53_121));
    }

    #[test]
    fn test_wrong_tid_rejected() {
        let tid = [7u8; 12];
        let resp = xor_resp(53_121, tid);
        let mut other = tid;
        other[0] ^= 0xff;
        assert_eq!(parse_mapped_port(&resp, &other), None);
    }

    #[test]
    fn test_malformed_rejected() {
        assert_eq!(parse_mapped_port(&[], &[0u8; 12]), None);
        assert_eq!(parse_mapped_port(&[0u8; 19], &[0u8; 12]), None);
    }

    #[test]
    fn test_trailing_attrs_skipped() {
        // miwifi-style reply: XOR-MAPPED-ADDRESS followed by more attributes.
        let tid = [9u8; 12];
        let mut resp = xor_resp(1234, tid);
        resp.extend_from_slice(&[0x80, 0x28, 0x00, 0x04, 1, 2, 3, 4]); // SOFTWARE-ish padding
        assert_eq!(parse_mapped_port(&resp, &tid), Some(1234));
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    #[tokio::test]
    async fn test_recheck_symmetric_real_network() {
        // On a cone NAT (this LAN) the recheck must downgrade Symmetric.
        // On a genuinely symmetric NAT or offline, it must keep Symmetric.
        // Both are acceptable outcomes; this pins "no panic, verdict in set".
        let v = recheck(NatTypeVerdict::Symmetric).await;
        assert!(matches!(v, NatTypeVerdict::Asymmetric | NatTypeVerdict::Symmetric));
    }

    #[tokio::test]
    async fn test_recheck_passthrough() {
        assert_eq!(
            recheck(NatTypeVerdict::Asymmetric).await,
            NatTypeVerdict::Asymmetric
        );
    }
}
