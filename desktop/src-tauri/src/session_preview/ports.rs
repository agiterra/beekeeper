//! "Local servers": the TCP servers listening on this machine's loopback
//! interface, for the Browser's empty state and `bee preview servers`.
//!
//! **Listed, never contacted.** `lsof -F pcn` lists TCP listeners with their
//! process and port, and that is the whole scan: nothing here opens a
//! connection to a port nobody chose. An earlier design sent each new
//! listener one HTTP GET to see whether it served HTML; that steals the first
//! connection of servers that hand it to a WebSocket upgrade (flutter_tester
//! failed to load test files with "Invalid WebSocket upgrade request" under
//! T3 Code's identical PortScanner), so a list a person merely looks at would
//! break their dev servers and test harnesses. The only request a port ever
//! gets is the page load when the person or an agent opens it. The cost,
//! disclosed: the list cannot tell a page from a database, and `title` is
//! always `null`; the UI labels each row with its URL and process.
//!
//! The list is kept warm by a 3 s poll that runs only while someone is
//! asking for it (the empty state) and stops 10 s after the last ask, so an
//! idle app runs no `lsof` at all.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

/// How often the warm list is refreshed while it is being asked for.
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// How long after the last ask the poll keeps running.
pub const HOLD_AFTER_LAST_CALL: Duration = Duration::from_secs(10);

/// One TCP listener as `lsof` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    /// Owning process id.
    pub pid: u32,
    /// Owning process command name (`lsof`'s `c` field, up to 15 chars on macOS).
    pub process: String,
    /// The bound address as printed: `*`, `127.0.0.1`, `[::1]`, `[::]`, …
    pub address: String,
    /// The bound port.
    pub port: u16,
}

/// A listening loopback TCP server, as the UI and the CLI show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalServer {
    /// Listening port.
    pub port: u16,
    /// What the preview opens for it.
    pub url: String,
    /// The bound address as `lsof` printed it.
    pub address: String,
    /// Owning process id.
    pub pid: u32,
    /// Owning process command name.
    pub process: String,
    /// Always `None`: the scan never contacts a listener, so it cannot read
    /// a title (kept on the wire so the UI and the CLI need no change).
    pub title: Option<String>,
}

/// Parse `lsof -nP -iTCP -sTCP:LISTEN -F pcn` output.
///
/// The format is one field per line, tagged by its first byte: `p<pid>`
/// starts a process set, `c<command>` names it, `f<fd>` starts a file set and
/// `n<name>` is the file's name (`host:port`). Unknown tags are ignored, and
/// a name that does not end in `:<port>` is skipped rather than guessed at.
pub fn parse_lsof(output: &str) -> Vec<Listener> {
    let mut listeners = Vec::new();
    let mut pid: Option<u32> = None;
    let mut process = String::new();
    for line in output.lines() {
        let mut chars = line.chars();
        let Some(tag) = chars.next() else {
            continue;
        };
        let value = chars.as_str();
        match tag {
            'p' => {
                pid = value.trim().parse().ok();
                process.clear();
            }
            'c' => process = value.to_string(),
            'n' => {
                let Some(pid) = pid else {
                    continue;
                };
                // A listener's name has no `->peer` part; be safe anyway.
                let local = value.split("->").next().unwrap_or(value);
                let Some((address, port)) = local.rsplit_once(':') else {
                    continue;
                };
                let Ok(port) = port.parse::<u16>() else {
                    continue;
                };
                if port == 0 {
                    continue;
                }
                listeners.push(Listener {
                    pid,
                    process: process.clone(),
                    address: address.to_string(),
                    port,
                });
            }
            _ => {}
        }
    }
    listeners
}

/// The loopback address a listener is reachable on, or `None` when it is bound
/// only to a non-loopback interface (and so is not reachable from the
/// preview, which only loads loopback).
pub fn loopback_host(address: &str) -> Option<&'static str> {
    match address {
        "*" | "0.0.0.0" | "127.0.0.1" | "localhost" => Some("127.0.0.1"),
        "[::1]" | "[::]" | "::1" | "::" => Some("[::1]"),
        other if other.starts_with("127.") => None,
        _ => None,
    }
}

/// Keep one listener per port, preferring an IPv4-reachable entry (a dev
/// server commonly binds both `127.0.0.1` and `[::1]`).
pub fn dedupe_by_port(listeners: Vec<Listener>) -> Vec<Listener> {
    let mut by_port: HashMap<u16, Listener> = HashMap::new();
    for listener in listeners {
        if loopback_host(&listener.address).is_none() {
            continue;
        }
        match by_port.get(&listener.port) {
            Some(existing) if loopback_host(&existing.address) == Some("127.0.0.1") => {}
            _ => {
                by_port.insert(listener.port, listener);
            }
        }
    }
    let mut out: Vec<Listener> = by_port.into_values().collect();
    out.sort_by_key(|listener| listener.port);
    out
}

/// Run `lsof` for TCP listeners. Synchronous; call off the async runtime.
fn run_lsof() -> Result<String, String> {
    let lsof = if std::path::Path::new("/usr/sbin/lsof").exists() {
        "/usr/sbin/lsof"
    } else {
        "lsof"
    };
    let output = std::process::Command::new(lsof)
        .args(["-nP", "-w", "-iTCP", "-sTCP:LISTEN", "-F", "pcn"])
        .output()
        .map_err(|e| format!("lsof could not run: {e}"))?;
    // lsof exits 1 when nothing matched; that is an empty list, not a failure.
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[derive(Default)]
struct Watch {
    last_call: Option<Instant>,
    polling: bool,
    servers: Vec<LocalServer>,
    scanned: bool,
}

static WATCH: Mutex<Option<Watch>> = Mutex::new(None);

fn with_watch<T>(f: impl FnOnce(&mut Watch) -> T) -> T {
    let mut guard = WATCH
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(guard.get_or_insert_with(Watch::default))
}

/// Ports never listed: this app's own listeners, and the dev frontend.
fn excluded(listener: &Listener, refused_port: Option<u16>) -> bool {
    listener.pid == std::process::id() || Some(listener.port) == refused_port
}

/// The servers one `lsof` listing names: loopback-reachable, one per port,
/// minus this app's own listeners and the dev frontend. Pure: it takes the
/// listing as text and has no way to reach the network.
pub fn servers_from_listing(raw: &str, refused_port: Option<u16>) -> Vec<LocalServer> {
    dedupe_by_port(parse_lsof(raw))
        .into_iter()
        .filter(|listener| !excluded(listener, refused_port))
        .map(|listener| LocalServer {
            port: listener.port,
            url: format!("http://localhost:{}/", listener.port),
            address: listener.address,
            pid: listener.pid,
            process: listener.process,
            title: None,
        })
        .collect()
}

/// One full scan: run `lsof`, list what it names. No listener is contacted.
pub async fn scan(refused_port: Option<u16>) -> Result<Vec<LocalServer>, String> {
    let raw = tokio::task::spawn_blocking(run_lsof)
        .await
        .map_err(|e| format!("lsof task failed: {e}"))??;
    let servers = servers_from_listing(&raw, refused_port);
    with_watch(|watch| {
        watch.servers = servers.clone();
        watch.scanned = true;
    });
    Ok(servers)
}

/// The current list, keeping the poll alive. The first call scans inline so
/// it never answers with an empty list it has not looked for.
pub async fn servers(refused_port: Option<u16>) -> Result<Vec<LocalServer>, String> {
    let (scanned, start_poll) = with_watch(|watch| {
        watch.last_call = Some(Instant::now());
        let start = !watch.polling;
        watch.polling = true;
        (watch.scanned, start)
    });
    if start_poll {
        tauri::async_runtime::spawn(poll_while_held(refused_port));
    }
    if scanned {
        return Ok(with_watch(|watch| watch.servers.clone()));
    }
    scan(refused_port).await
}

async fn poll_while_held(refused_port: Option<u16>) {
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        let held = with_watch(|watch| {
            let held = watch
                .last_call
                .is_some_and(|at| at.elapsed() < HOLD_AFTER_LAST_CALL);
            if !held {
                watch.polling = false;
                // A stale list must not be served as current next time.
                watch.scanned = false;
            }
            held
        });
        if !held {
            return;
        }
        if let Err(error) = scan(refused_port).await {
            eprintln!("session-preview: local server scan failed: {error}");
        }
    }
}

#[cfg(test)]
#[path = "ports_tests.rs"]
mod tests;
