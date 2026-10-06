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

use beekeeper_core::coding_session_command::{
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
    /// The **signed command, whole**, as it arrived.
    ///
    /// This is what makes a release a re-admission rather than a re-start
    /// (Astra's Wave 2 re-check, R4): the release hands these same bytes back
    /// to the one function that admits a command, so the consumed/in-flight
    /// check, the freshness horizon, the target generation and the signer's
    /// authority are all asked again, about the original command, under its
    /// original identity. Reconstructing a `TurnDecision` skipped every one
    /// of them.
    ///
    /// `#[serde(default)]`: a record written before this field replays
    /// nothing and is expired by the pass instead, which is the honest
    /// answer for a command whose bytes were never kept.
    #[serde(default)]
    pub content: String,
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

/// The state directory a mirror belongs to, and the wakes it holds.
type Mirror = Option<(PathBuf, Vec<DeferredTurn>)>;

/// The in-memory mirror, so the watermark ceiling — asked on every command —
/// costs no disk read.
///
/// Loaded from the file the first time a state directory is seen, and written
/// through on every change, so the two never disagree within a process and the
/// file is the whole truth across restarts.
fn mirror() -> &'static Mutex<Mirror> {
    static MIRROR: OnceLock<Mutex<Mirror>> = OnceLock::new();
    MIRROR.get_or_init(|| Mutex::new(None))
}

fn path_of(state_dir: &Path) -> PathBuf {
    state_dir.join(DEFERRED_TURNS_FILE)
}

/// Read the held set, or say why it cannot be read.
///
/// A missing file is an empty set — nothing has ever been held. Anything else
/// is a **failure**, not an empty set (Astra's third look, R4): a store whose
/// contents cannot be read may be holding wakes this provider owes answers
/// for, and treating it as empty silently discards every one of them. The
/// file is left exactly as it is, deferral refuses while the condition lasts,
/// and the condition is visible.
fn read_file(state_dir: &Path) -> Result<Vec<DeferredTurn>, String> {
    let path = path_of(state_dir);
    let raw = match std::fs::read(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "the deferred-wake file at {} could not be read: {error}",
                path.display()
            ))
        }
    };
    match serde_json::from_slice::<DeferredTurnsFile>(&raw) {
        Ok(file) if file.version == DEFERRED_TURNS_VERSION => Ok(file.turns),
        Ok(file) => Err(format!(
            "the deferred-wake file is version {}, which this build does not read",
            file.version
        )),
        Err(error) => Err(format!(
            "the deferred-wake file could not be decoded: {error}"
        )),
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
///
/// A failed write is returned, and the mirror is **not** updated (R4): a
/// store that claims durable custody of a wake and then keeps it only in
/// memory is the same lie as not keeping it at all. On failure the mirror
/// still holds what the file holds, so the next caller reads the truth.
fn with_turns<T>(
    state_dir: &Path,
    act: impl FnOnce(&mut Vec<DeferredTurn>) -> T,
) -> Result<T, String> {
    let mut guard = match mirror().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let loaded = match guard.as_ref() {
        Some((path, turns)) if path == state_dir => turns.clone(),
        // A store this provider cannot read is not an empty one, and it is
        // not this call's to overwrite either.
        _ => read_file(state_dir)?,
    };
    let mut turns = loaded.clone();
    let answer = act(&mut turns);
    if turns == loaded {
        *guard = Some((state_dir.to_path_buf(), turns));
        return Ok(answer);
    }
    write_file(state_dir, &turns)?;
    *guard = Some((state_dir.to_path_buf(), turns));
    Ok(answer)
}

/// Whether this provider can read the wakes it may be holding.
///
/// `Ok(())` when the file is readable or absent. The error is the sentence a
/// caller publishes when it refuses to hold anything more.
pub fn readable(state_dir: &Path) -> Result<(), String> {
    let readable = {
        let guard = match mirror().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        matches!(guard.as_ref(), Some((path, _)) if path == state_dir)
    };
    if readable {
        return Ok(());
    }
    read_file(state_dir).map(|_| ())
}

/// Hold one wake until its seat's input settles.
///
/// Idempotent by command id: a wake deferred twice is one wake, and the
/// original `created_at` — the one the watermark is measured against — is the
/// one kept.
///
/// Two failures are returned rather than swallowed (R4). A write that does
/// not reach the disk means nothing is holding this wake, and the caller has
/// to answer the command instead of pretending. A full store **refuses the
/// new deferral**; it never evicts an older owed wake to make room, because
/// the evicted one would be owed by nobody and answered by nothing.
pub fn defer(state_dir: &Path, turn: DeferredTurn) -> Result<(), String> {
    let command_id = turn.command_id.clone();
    let outcome = with_turns(state_dir, move |turns| {
        if let Some(existing) = turns
            .iter_mut()
            .find(|existing| existing.command_id == turn.command_id)
        {
            existing.reason.clone_from(&turn.reason);
            return Ok(());
        }
        if turns.len() >= MAX_DEFERRED_TURNS {
            return Err(format!(
                "this provider is already holding {MAX_DEFERRED_TURNS} deferred wakes, and \
                 nothing owed is dropped to make room"
            ));
        }
        turns.push(turn);
        Ok(())
    })?;
    if let Err(error) = &outcome {
        tracing::error!(
            target: "csp::deferred_turns",
            %command_id,
            %error,
            "a wake could not be held"
        );
    }
    outcome
}

/// Forget a wake, because it has been decided.
pub fn release(state_dir: &Path, command_id: &str) -> Result<(), String> {
    with_turns(state_dir, |turns| {
        turns.retain(|turn| turn.command_id != command_id);
    })
}

/// Every wake this provider is holding, oldest first.
#[must_use]
pub fn held(state_dir: &Path) -> Vec<DeferredTurn> {
    let mut turns = match with_turns(state_dir, |turns| turns.clone()) {
        Ok(turns) => turns,
        Err(error) => {
            // Said out loud on every pass: a store that cannot be read is a
            // set of unanswered wakes nobody can see, and pretending it is
            // empty is how they would be lost silently.
            tracing::error!(
                target: "csp::deferred_turns",
                %error,
                code = "deferred_store_unreadable",
                "the held wakes cannot be read; no wake is being released and none will be held"
            );
            Vec::new()
        }
    };
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
    with_turns(state_dir, |turns| {
        turns
            .iter()
            .filter(|turn| turn.channel_id == channel_id)
            .map(|turn| turn.created_at)
            .min()
    })
    .ok()
    .flatten()
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
