//! Agent rows the *app* contributes, held for as long as it keeps saying so.
//!
//! The host knows its own coding sessions: it reads them out of the provider's
//! atomically-replaced state file. It does **not** know about managed agents
//! (`beekeeper-acp`), which are still the desktop app's children and still die with
//! it — so the app pushes those rows here, and the menu bar shows a complete
//! picture without the host having to grow a relay connection or an opinion
//! about a tier it does not own.
//!
//! # Why a lease rather than a connection
//!
//! The rows have to **disappear when the app quits**, because that is the
//! truth: those agents died with it. A menu that kept showing them would be
//! claiming a dead agent is working, which is the same class of defect as a
//! status reading Idle over a disconnected provider.
//!
//! The control protocol is one request per connection, so there is no
//! connection to hang the rows on. Instead each push carries a lease: rows
//! live for [`LEASE`] and the app re-pushes well inside that. When the app
//! quits, the rows age out on their own — no goodbye message to lose, and a
//! crashed app behaves exactly like a quit one.
//!
//! The lease is deliberately several times the push interval. Too tight and an
//! app that was briefly busy blinks its rows out; too loose and a quit app's
//! agents linger on display. A few seconds of lag after quitting is the right
//! side of that trade: it is late, not wrong.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How long a pushed row survives without being re-pushed.
///
/// The app pushes on its own tray cadence; this is a small multiple of it, so
/// one missed push does not blink the rows out.
pub const LEASE: Duration = Duration::from_secs(20);

/// One agent row the app contributed.
///
/// Deliberately close to the tray's own row shape, minus the formatted
/// elapsed: like every other timestamp on this socket, the wire carries the
/// absolute start and the client formats it from its own clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushedActivity {
    /// Stable per-row id, so a re-push updates rather than duplicates.
    pub activity_id: String,
    pub agent_name: String,
    /// The agent's identity. Two projects may each have a `Builder`, and the
    /// name alone would render two rows a person cannot tell apart.
    #[serde(default)]
    pub agent_pubkey: String,
    pub channel_id: String,
    pub channel_name: String,
    /// When this turn started, milliseconds since the Unix epoch.
    pub started_at_ms: i64,
    /// Whether this row is finished work rather than a running turn.
    #[serde(default)]
    pub recent: bool,
}

/// What the app most recently pushed, and when.
#[derive(Default)]
pub struct ActivityStore {
    inner: Mutex<Option<Lease>>,
}

struct Lease {
    rows: Vec<PushedActivity>,
    pushed_at: Instant,
}

impl ActivityStore {
    /// Replace the app's rows wholesale.
    ///
    /// Wholesale rather than merged: the app knows its complete set, and a
    /// merge would need a way to say "this one is gone" — which is a second
    /// mechanism for something the lease already handles.
    pub fn push(&self, rows: Vec<PushedActivity>) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some(Lease {
                rows,
                pushed_at: Instant::now(),
            });
        }
    }

    /// The rows still under lease, or empty.
    pub fn current(&self) -> Vec<PushedActivity> {
        self.current_at(Instant::now())
    }

    /// [`Self::current`] against an explicit clock, so the lease is provable.
    pub fn current_at(&self, now: Instant) -> Vec<PushedActivity> {
        let Ok(guard) = self.inner.lock() else {
            return Vec::new();
        };
        match guard.as_ref() {
            Some(lease) if now.duration_since(lease.pushed_at) < LEASE => lease.rows.clone(),
            // Expired, or never pushed. Both mean the same thing to a menu:
            // this host has nothing to say about managed agents right now.
            _ => Vec::new(),
        }
    }

    /// Whether an app is currently holding a lease — used to say *why* there
    /// are no managed-agent rows, rather than leaving a person to guess.
    pub fn has_live_lease(&self) -> bool {
        self.has_live_lease_at(Instant::now())
    }

    pub fn has_live_lease_at(&self, now: Instant) -> bool {
        self.inner
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .map(|lease| now.duration_since(lease.pushed_at) < LEASE)
            })
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str) -> PushedActivity {
        PushedActivity {
            activity_id: id.to_string(),
            agent_name: "Builder".to_string(),
            agent_pubkey: "a".repeat(64),
            channel_id: "planning".to_string(),
            channel_name: "planning".to_string(),
            started_at_ms: 1_750_000_000_000,
            recent: false,
        }
    }

    #[test]
    fn nothing_pushed_is_no_rows_and_no_lease() {
        let store = ActivityStore::default();
        assert!(store.current().is_empty());
        assert!(!store.has_live_lease());
    }

    /// The property this exists for: when the app stops pushing, its rows go
    /// away. A menu that kept them would claim a dead agent is working.
    #[test]
    fn rows_expire_when_the_app_stops_pushing() {
        let store = ActivityStore::default();
        store.push(vec![row("one"), row("two")]);
        let pushed_at = Instant::now();

        assert_eq!(store.current_at(pushed_at).len(), 2);
        assert!(store.has_live_lease_at(pushed_at));

        // Just inside the lease: still shown, so one slow push does not blink
        // the rows out.
        let nearly = pushed_at + LEASE - Duration::from_millis(1);
        assert_eq!(store.current_at(nearly).len(), 2);
        assert!(store.has_live_lease_at(nearly));

        // Past it: gone, and the lease is gone with them.
        let after = pushed_at + LEASE + Duration::from_millis(1);
        assert!(store.current_at(after).is_empty());
        assert!(!store.has_live_lease_at(after));
    }

    /// A re-push replaces rather than merges: the app knows its whole set, and
    /// a merge would need a separate way to say "this one is finished".
    #[test]
    fn a_push_replaces_the_previous_rows_wholesale() {
        let store = ActivityStore::default();
        store.push(vec![row("one"), row("two")]);
        store.push(vec![row("three")]);
        let ids: Vec<String> = store
            .current()
            .into_iter()
            .map(|row| row.activity_id)
            .collect();
        assert_eq!(ids, vec!["three"]);
        // Including pushing nothing, which is how an app with no live agents
        // says so while still holding its lease.
        store.push(Vec::new());
        assert!(store.current().is_empty());
        assert!(
            store.has_live_lease(),
            "an empty push is an app saying 'none', not an app going away"
        );
    }

    /// The wire carries an absolute start, never a formatted duration — the
    /// same rule as every other timestamp on this socket.
    #[test]
    fn the_wire_carries_milliseconds_and_no_elapsed_string() {
        let json = serde_json::to_string(&row("one")).expect("encode");
        assert!(json.contains("\"startedAtMs\":1750000000000"), "{json}");
        assert!(!json.contains("elapsed"), "{json}");
        // And `agentPubkey` survives, because two `Builder`s must be
        // distinguishable.
        assert!(json.contains("\"agentPubkey\""), "{json}");
    }

    /// An older app that does not send `recent` or `agentPubkey` still parses.
    #[test]
    fn a_row_without_the_optional_fields_still_parses() {
        let parsed: PushedActivity = serde_json::from_str(
            r#"{"activityId":"x","agentName":"Scout","channelId":"c",
                "channelName":"planning","startedAtMs":1}"#,
        )
        .expect("decode");
        assert!(parsed.agent_pubkey.is_empty());
        assert!(!parsed.recent);
    }
}
