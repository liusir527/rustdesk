//! Client-side relay candidate latency probe (Phase 2 of "auto nearest relay").
//!
//! hbbs (relay-geo) may send an ordered candidate relay list in
//! `PunchHole.relay_servers` / `PunchHoleResponse.relay_servers`. The side that
//! *initiates* a relayed connection probes these candidates with a bare TCP
//! connect (timed, then dropped immediately) and picks the fastest one.
//! hbbr treats such connections as a peer that never sent a `RequestRelay` and
//! cleans them up silently, so probing has no server-side side effects.
//!
//! Rules:
//! - Only probes when there are >= 2 candidates and the `relay-probe` option is
//!   not disabled (values N/n/0/off/false disable; anything else enables).
//! - Per-candidate timeout 500 ms, overall budget 700 ms.
//! - RTT cache with 300 s TTL so consecutive connection attempts within the TTL
//!   reuse previous measurements (probing is skipped entirely on a cache hit).
//! - Empty candidate list, all-timeout, or any error => fall back to the
//!   server's primary pick (`fallback`), i.e. behaviour identical to before.
//! - Ties keep the server's original order (stable, no flapping).

use hbb_common::{
    config::Config,
    log,
    socket_client::{check_port, connect_tcp, ipv4_to_ipv6},
    tokio::time::{timeout, Instant},
};use std::collections::HashMap;

const PROBE_TIMEOUT_MS: u64 = 500;
const BUDGET_MS: u64 = 700;
const CACHE_TTL_MS: u128 = 300_000;
const MAX_CANDIDATES: usize = 4;

static CACHE: std::sync::Mutex<Option<HashMap<String, (u64, Instant)>>> =
    std::sync::Mutex::new(None);

/// Returns false when the option value explicitly disables probing.
fn enabled() -> bool {
    let v = Config::get_option("relay-probe");
    !matches!(v.to_lowercase().as_str(), "n" | "0" | "off" | "false" | "")
}

fn cache_get(addr: &str) -> Option<u64> {
    let mut guard = CACHE.lock().ok()?;
    let map = guard.as_mut()?;
    let (rtt, at) = map.get(addr).copied()?;
    if at.elapsed().as_millis() > CACHE_TTL_MS {
        map.remove(addr);
        return None;
    }
    Some(rtt)
}

fn cache_put(addr: String, rtt: u64) {
    if let Ok(mut guard) = CACHE.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        if map.len() > 64 {
            map.clear(); // crude bound; addresses are few and stable
        }
        map.insert(addr, (rtt, Instant::now()));
    }
}

/// Trim, drop empties, dedup keeping order, cap at MAX_CANDIDATES.
fn sanitize(candidates: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(candidates.len().min(MAX_CANDIDATES));
    for c in candidates {
        let c = c.trim();
        if c.is_empty() || out.iter().any(|x| x == c) {
            continue;
        }
        out.push(c.to_owned());
        if out.len() >= MAX_CANDIDATES {
            break;
        }
    }
    out
}

async fn probe_one(addr: String) -> Option<u64> {
    let target = check_port(addr.trim(), hbb_common::config::RELAY_PORT);
    let start = Instant::now();
    match timeout(
        std::time::Duration::from_millis(PROBE_TIMEOUT_MS),
        connect_tcp(ipv4_to_ipv6(target, false), PROBE_TIMEOUT_MS),
    )
    .await
    {
        Ok(Ok(_)) => {
            let rtt = start.elapsed().as_millis() as u64;
            // Drop the connection immediately; hbbr cleans it up quietly.
            Some(rtt)
        }
        _ => None,
    }
}

/// Pick the nearest relay among `candidates` by measured TCP connect RTT.
/// `fallback` is returned when probing is disabled, the candidate list is too
/// small, or nothing answered in time. Never blocks longer than BUDGET_MS.
pub async fn pick(candidates: &[String], fallback: &str) -> String {
    let list = sanitize(candidates);
    if list.len() < 2 || !enabled() || fallback.is_empty() {
        return fallback.to_owned();
    }
    // All candidates already cached? Pick best without touching the network.
    if let Some(cached) = list.iter().map(|c| cache_get(c)).collect::<Option<Vec<_>>>() {
        let (best_idx, _) = cached
            .iter()
            .enumerate()
            .min_by_key(|(_, rtt)| **rtt)
            .unwrap();
        let best = list[best_idx].clone();
        if best != fallback {
            log::info!( "relay probe: cache hit, picked {} ({:?} ms) over fallback {}", best, cached[best_idx], fallback);
        }
        return best;
    }

    let deadline = Instant::now() + std::time::Duration::from_millis(BUDGET_MS);

    // Spawn probes keeping address association.
    let mut futs = Vec::new();
    for c in list.iter().cloned() {
        futs.push(hbb_common::tokio::spawn(async move {
            let r = probe_one(c.clone()).await;
            (c, r)
        }));
    }
    let mut best: Option<(String, u64)> = None;
    for fut in futs {
        let elapsed = deadline.saturating_duration_since(Instant::now());
        if elapsed.is_zero() {
            fut.abort();
            continue;
        }
        match timeout(elapsed, fut).await {
            Ok(Ok((addr, Some(rtt)))) => {
                cache_put(addr.clone(), rtt);
                if best.as_ref().map(|(_, r)| rtt < *r).unwrap_or(true) {
                    best = Some((addr, rtt));
                }
            }
            _ => {}
        }
    }
    match best {
        Some((addr, rtt)) => {
            if addr != fallback {
                log::info!("relay probe: picked {} ({} ms) over fallback {}", addr, rtt, fallback);
            }
            addr
        }
        None => fallback.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize() {
        let v: Vec<String> = vec![
            " a:1 ".to_owned(),
            "".to_owned(),
            "a:1".to_owned(),
            "b:2".to_owned(),
            "c:3".to_owned(),
            "d:4".to_owned(),
            "e:5".to_owned(),
        ];
        let s = sanitize(&v);
        assert_eq!(s, vec!["a:1".to_owned(), "b:2".to_owned(), "c:3".to_owned(), "d:4".to_owned()]);
        assert!(sanitize(&[]).is_empty());
        assert_eq!(sanitize(&["".to_owned()]).len(), 0);
    }

    #[test]
    fn test_enabled_values() {
        // Explicit disable values.
        for v in ["N", "n", "0", "off", "OFF", "false", "FALSE"] {
            assert!(!enabled_with(v), "expected disabled for {:?}", v);
        }
        // Explicit enable values.
        for v in ["Y", "1", "on", "true"] {
            assert!(enabled_with(v), "expected enabled for {:?}", v);
        }
    }

    fn enabled_with(v: &str) -> bool {
        !matches!(
            v.to_lowercase().as_str(),
            "n" | "0" | "off" | "false" | ""
        )
    }
}
