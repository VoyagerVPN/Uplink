use super::{read_raw, rooms, saved_urls, update};
use crate::transport::{self, PingError};
use serde_json::Value;
use std::{
    io,
    net::Ipv4Addr,
    path::Path,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn same_peer(a: &Value, b: &Value) -> bool {
    a["node"] == b["node"] && a["ip"] == b["ip"]
}

// Пока HTTPS ждал ответа, поток ICMP уже мог записать более свежий замер.
pub(super) fn retain_readings(next: &mut Value, current: &Value) {
    if next["own_ip"].as_str().unwrap_or_default().is_empty()
        || next["own_ip"] != current["own_ip"]
        || next["session"] != current["session"]
    {
        return;
    }
    let own = next["node"].clone();
    let (Some(rows), Some(previous)) = (next["rows"].as_array_mut(), current["rows"].as_array())
    else {
        return;
    };
    for row in rows.iter_mut().filter(|r| r["node"] != own) {
        if let Some(old) = previous.iter().find(|p| same_peer(row, p)) {
            row["ping_ms"] = old["ping_ms"].clone();
        }
    }
}

fn address(raw: &str) -> String {
    raw.parse::<Ipv4Addr>()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "unknown".into())
}

fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn log_samples(path: &Path, lines: &str) -> io::Result<()> {
    super::diagnostics::append(&path.with_file_name("peer-ping.log"), lines)
}

fn sample(
    source: &str,
    target: &str,
    probe: &impl Fn(&str, &str) -> Result<u32, PingError>,
) -> (Option<u32>, String) {
    use std::fmt::Write as _;
    let mut lines = String::new();
    for attempt in 1..=2 {
        let time = timestamp();
        let start = Instant::now();
        let result = probe(source, target);
        let elapsed = start.elapsed().as_millis();
        let _ = write!(
            lines,
            "{time} src={} dst={} attempt={attempt} elapsed_ms={elapsed} timeout_ms={} ",
            address(source),
            address(target),
            transport::ECHO_TIMEOUT_MS
        );
        match result {
            Ok(ms) => {
                let _ = writeln!(lines, "result=ok rtt_ms={ms}");
                return (Some(ms), lines);
            }
            Err(error) => {
                let _ = writeln!(lines, "{error}");
                if !error.timeout() {
                    break;
                }
            }
        }
    }
    (None, lines)
}

pub(super) fn poll_with(
    path: &Path,
    probe: impl Fn(&str, &str) -> Result<u32, PingError> + Sync,
) -> io::Result<bool> {
    let prefs = read_raw(path)?;
    if prefs["power"] == false || prefs["pending_power"] == false {
        return Ok(false);
    }
    let mut changed = false;
    for net in rooms(path)?.into_iter().filter_map(|room| room.live) {
        if net.availability.membership_gone() || net.rows.is_empty() {
            continue;
        }
        if net.own_ip.is_empty() {
            let _ = log_samples(
                path,
                &format!("{} result=skipped reason=no_adapter_ip\n", timestamp()),
            );
            continue;
        }
        let mut readings = Vec::new();
        for batch in net.rows.chunks(8) {
            thread::scope(|scope| {
                let mut workers = Vec::new();
                for row in batch.iter().filter(|row| row.node != net.node) {
                    workers.push((row, scope.spawn(|| sample(&net.own_ip, &row.ip, &probe))));
                }
                for (row, worker) in workers {
                    let (ms, lines) = worker.join().expect("peer probe panicked");
                    let _ = log_samples(path, &lines);
                    if ms != row.ping_ms {
                        readings.push((row, ms));
                    }
                }
            });
        }
        if readings.is_empty() {
            continue;
        }
        let key = net.key();
        update(path, |prefs| {
            if prefs["power"] == false
                || prefs["pending_power"] == false
                || !saved_urls(prefs)?.contains(&key.as_str())
            {
                return Ok(());
            }
            let current = &mut prefs["live"][&key];
            if current["session"] != net.session || current["own_ip"] != net.own_ip {
                return Ok(());
            }
            if let Some(rows) = current["rows"].as_array_mut() {
                for (peer, ms) in readings {
                    if let Some(row) = rows
                        .iter_mut()
                        .find(|r| r["node"] == peer.node && r["ip"] == peer.ip)
                    {
                        row["ping_ms"] = ms.into();
                        changed = true;
                    }
                }
            }
            Ok(())
        })?;
    }
    Ok(changed)
}

fn next_poll(last: Instant, now: Instant) -> Instant {
    let period = Duration::from_secs(5);
    let mut next = last + period;
    while next < now {
        next += period;
    }
    next
}

pub fn run_peer_polls(path: &Path) {
    let mut tick = Instant::now();
    loop {
        match poll_with(path, transport::peer_ping) {
            Ok(true) => {
                let _ = crate::changed::notify();
            }
            Err(e) => {
                let _ = log_samples(
                    path,
                    &format!(
                        "{} result=error operation=poll kind={:?}\n",
                        timestamp(),
                        e.kind()
                    ),
                );
            }
            _ => {}
        }
        let now = Instant::now();
        tick = next_poll(tick, now);
        thread::sleep(tick.saturating_duration_since(now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polling_keeps_five_second_slots_and_skips_missed_slots() {
        let start = Instant::now();
        assert_eq!(
            next_poll(start, start + Duration::from_millis(700)),
            start + Duration::from_secs(5)
        );
        assert_eq!(
            next_poll(start, start + Duration::from_secs(5)),
            start + Duration::from_secs(5)
        );
        assert_eq!(
            next_poll(start, start + Duration::from_millis(10600)),
            start + Duration::from_secs(15)
        );
    }

    #[test]
    fn sync_retains_only_readings_for_the_same_session_adapter_and_peer() {
        use serde_json::json;
        let current = json!({"node":"a","session":"s","own_ip":"10.1.1.2","rows":[
            {"node":"b","ip":"10.1.1.3","ping_ms":31},
            {"node":"c","ip":"10.1.1.4","ping_ms":42}]});
        let snapshot = json!({"node":"a","session":"s","own_ip":"10.1.1.2","rows":[
            {"node":"a","ip":"10.1.1.2","ping_ms":60},
            {"node":"b","ip":"10.1.1.3","ping_ms":null},
            {"node":"d","ip":"10.1.1.4","ping_ms":null}]});
        let mut merged = snapshot.clone();
        retain_readings(&mut merged, &current);
        assert_eq!(merged["rows"][0]["ping_ms"], 60);
        assert_eq!(merged["rows"][1]["ping_ms"], 31);
        assert!(merged["rows"][2]["ping_ms"].is_null());
        for field in ["session", "own_ip"] {
            let mut replacement = snapshot.clone();
            replacement[field] = "changed".into();
            retain_readings(&mut replacement, &current);
            assert!(replacement["rows"][1]["ping_ms"].is_null());
        }
    }
}
