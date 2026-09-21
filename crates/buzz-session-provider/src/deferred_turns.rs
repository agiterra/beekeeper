//! Assignment wakes this provider is holding, and owes an answer for.
//!
//! # The defect this closes
//!
//! Lane 202 answered a wake whose input was not established yet with
//! `TurnDisposition::Undecided`: nothing published, nothing consumed, the
//! command still deliverable. That is the right *shape* — an early wake must
//! not cost a re-issued assignment — but nothing owned the redelivery (Astra's
//! Wave 2 review, finding 4). The live path returned without advancing its
//! watermark and then forgot the command; the replay path advanced the
//! watermark on `Ok(Undecided)` as readily as on a delivery; the watermark
//! ceiling counted only in-flight and replay-held commands; and no completion
//! callback existed, so a fetch that finished at 130 seconds released nothing.
//! An otherwise healthy connection never replayed that wake, and a newer
//! channel event could move the floor past it, defeating reconnect recovery
//! too.
//!
//! # What this is
//!
//! The durable owner. A deferred wake is written here, in full, under the
//! provider's own state directory; the channel's watermark is clamped to the
//! oldest one it holds, so no later event can move the floor past it; and
//! every tick asks whether the thing it was waiting for has settled. When it
//! has, the command is decided again — from the record written here, not from
//! a second command anybody had to send — and removed.
//!
//! It survives a restart because it is a file, and because it holds the whole
//! command: the target, the text, the attachments, the delivery class and the
//! resolved operation key, plus the channel, the signer and the `created_at`
//! the watermark is measured in.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use buzz_core::coding_session_command::{
    CodingSessionDelivery, CodingSessionTarget, TurnAttachment,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The file, under the provider's state directory.
pub const DEFERRED_TURNS_FILE: &str = "deferred-turns.json";

/// Current on-disk schema version.
pub const DEFERRED_TURNS_VERSION: u32 = 1;

/// How many deferred wakes are kept at once.
///
/// One per seat awaiting an input is the steady state. The cap bounds the
/// pathological case; over it the oldest is dropped, because a wake older than
/// the command horizon is already undeliverable.
pub const MAX_DEFERRED_TURNS: usize = 64;

/// One wake this provider is holding until its seat's input settles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeferredTurn {
    /// The command this wake carries, and the identity the answer belongs to.
    pub command_id: String,
    /// The channel it arrived on.
    pub channel_id: Uuid,
    /// Its `created_at`, which is what the watermark is measured in.
    pub created_at: u64,
    /// The signer, re-checked when the command is decided again.
    pub operator_pubkey: String,
    /// The signed event id, or empty for a provider-minted delivery.
    pub event_id: String,
    /// The execution it addressed.
    pub target: CodingSessionTarget,
    /// The prompt text, exactly as it was signed.
    pub text: String,
    /// Images the operator attached.
    #[serde(default)]
    pub attachments: Vec<TurnAttachment>,
    /// The delivery class the sender asked for.
    pub deliver: CodingSessionDelivery,
    /// The resolved operation fence key, when the prompt is a pointer.
    #[serde(default)]
    pub operation_key: Option<String>,
    /// The assignment whose establishment this wake is waiting on.
    pub assignment_ref: String,
    /// Why it is being held, in this provider's own words.
    pub reason: String,
    /// When it was first deferred, ISO-8601.
    pub deferred_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeferredTurnsFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    turns: Vec<DeferredTurn>,
}

/// The in-memory mirror, so the watermark ceiling — asked on every command —
/// costs no disk read.
///
/// Loaded from the file the first time a state directory is seen, and written
/// through on every change, so the two never disagree within a process and the
/// file is the whole truth across restarts.
fn mirror() -> &'static Mutex<Option<(PathBuf, Vec<DeferredTurn>)>> {
    static MIRROR: OnceLock<Mutex<Option<(PathBuf, Vec<DeferredTurn>)>>> = OnceLock::new();
    MIRROR.get_or_init(|| Mutex::new(None))
}

fn path_of(state_dir: &Path) -> PathBuf {
    state_dir.join(DEFERRED_TURNS_FILE)
}

fn read_file(state_dir: &Path) -> Vec<DeferredTurn> {
    let path = path_of(state_dir);
    let Ok(raw) = std::fs::read(&path) else {
        return Vec::new();
    };
    match serde_json::from_slice::<DeferredTurnsFile>(&raw) {
        Ok(file) if file.version == DEFERRED_TURNS_VERSION => file.turns,
        Ok(file) => {
            tracing::warn!(
                target: "csp::deferred_turns",
                version = file.version,
                "ignoring deferred wakes written by a version this build does not read"
            );
            Vec::new()
        }
        Err(error) => {
            tracing::warn!(target: "csp::deferred_turns", %error, "the deferred-wake file could not be read");
            Vec::new()
        }
    }
}

fn write_file(state_dir: &Path, turns: &[DeferredTurn]) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(&DeferredTurnsFile {
        version: DEFERRED_TURNS_VERSION,
        turns: turns.to_vec(),
    })
    .map_err(|error| format!("failed to serialize deferred wakes: {error}"))?;
    std::fs::create_dir_all(state_dir)
        .map_err(|error| format!("failed to create the provider state directory: {error}"))?;
    let path = path_of(state_dir);
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, &payload)
        .map_err(|error| format!("failed to write deferred wakes: {error}"))?;
    std::fs::rename(&temporary, &path)
        .map_err(|error| format!("failed to replace the deferred-wake file: {error}"))
}

/// Run `act` against the current list, writing it back if it changed.
fn with_turns<T>(state_dir: &Path, act: impl FnOnce(&mut Vec<DeferredTurn>) -> T) -> (T, bool) {
    let mut guard = match mirror().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let loaded = match guard.as_ref() {
        Some((path, turns)) if path == state_dir => turns.clone(),
        _ => read_file(state_dir),
    };
    let mut turns = loaded.clone();
    let answer = act(&mut turns);
    let changed = turns != loaded;
    if changed {
        while turns.len() > MAX_DEFERRED_TURNS {
            turns.remove(0);
        }
        if let Err(error) = write_file(state_dir, &turns) {
            tracing::error!(target: "csp::deferred_turns", %error, "a deferred wake could not be saved");
        }
    }
    *guard = Some((state_dir.to_path_buf(), turns));
    (answer, changed)
}

/// Hold one wake until its seat's input settles.
///
/// Idempotent by command id: a wake deferred twice is one wake, and the
/// original `created_at` — the one the watermark is measured against — is the
/// one kept.
pub fn defer(state_dir: &Path, turn: DeferredTurn) {
    let (_, _) = with_turns(state_dir, |turns| {
        if let Some(existing) = turns
            .iter_mut()
            .find(|existing| existing.command_id == turn.command_id)
        {
            existing.reason.clone_from(&turn.reason);
            return;
        }
        turns.push(turn);
    });
}

/// Forget a wake, because it has been decided.
pub fn release(state_dir: &Path, command_id: &str) {
    let (_, _) = with_turns(state_dir, |turns| {
        turns.retain(|turn| turn.command_id != command_id);
    });
}

/// Every wake this provider is holding, oldest first.
#[must_use]
pub fn held(state_dir: &Path) -> Vec<DeferredTurn> {
    let (mut turns, _) = with_turns(state_dir, |turns| turns.clone());
    turns.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.command_id.cmp(&right.command_id))
    });
    turns
}

/// The oldest held wake on one channel, which the watermark may not pass.
///
/// This is the whole of the clamp: `record_watermark` only ever advances, so a
/// floor that moved past a wake this provider still owes would skip it at the
/// next subscription — the exact loss the deferral exists to prevent.
#[must_use]
pub fn floor_for_channel(state_dir: &Path, channel_id: Uuid) -> Option<u64> {
    let (floor, _) = with_turns(state_dir, |turns| {
        turns
            .iter()
            .filter(|turn| turn.channel_id == channel_id)
            .map(|turn| turn.created_at)
            .min()
    });
    floor
}

/// Forget everything this process mirrored, so a test can start clean.
#[cfg(test)]
pub fn forget_mirror() {
    let mut guard = match mirror().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = None;
}

#[cfg(test)]
#[path = "deferred_turns_tests.rs"]
mod tests;
