//! SV-29 rewind: the `rewind` object on a kind:44224 receipt.
//!
//! A `session.rewind` (NIP-CSL) is answered with the ordinary lifecycle
//! statuses — `resumed`/`resumed_without_context` when generation N+1 opened,
//! `failed` otherwise — plus this optional seventh key when the command got
//! past its checks. It says exactly what happened: which checkpoint, where the
//! record was cut, which generation was detached, what became of the files,
//! the `pre_rewind` checkpoint that makes the rewind undoable, and HEAD's
//! unchanged oid. Every key is required (absent is not null), and unknown keys
//! are refused, the discipline every receipt reader holds.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::coding_session_command::MAX_SAFE_GENERATION;
use crate::coding_session_lifecycle_command::validate_event_id_hex;
use crate::coding_session_payload::{LifecycleReceipt, ReceiptStatus, REWIND_NOT_RESTARTED};

/// What a rewind did to the working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewindFilesOutcome {
    /// `files: keep`: nothing was written.
    Kept,
    /// `files: restore`: the tree was returned to the checkpoint's `baseTree`.
    Restored,
    /// `files: restore` failed part way; the `pre_rewind` checkpoint holds
    /// the tree as it was. Only on a `failed` receipt.
    RestoreFailed,
}

/// The `rewind` object of a receipt answering a `session.rewind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReceiptRewind {
    /// The kind:44231 checkpoint the command named.
    pub checkpoint: String,
    /// The generation the cut falls in. Seqs restart at 1 per generation,
    /// so `cutAfterSeq` alone cannot name a turn.
    pub cut_generation: u64,
    /// The last seq of `cutGeneration` kept: the checkpoint's `fromSeq − 1`.
    pub cut_after_seq: u64,
    /// The generation that was detached (never truncated).
    pub previous_generation: u64,
    /// What became of the working tree.
    pub files: RewindFilesOutcome,
    /// The `pre_rewind` kind:44231 published before anything was touched,
    /// or `null` for a chat-only rewind outside a repository.
    pub pre_rewind_checkpoint: Option<String>,
    /// HEAD's oid, which the rewind did not move, or `null` with no HEAD.
    pub head: Option<String>,
}

const FIELDS: [&str; 7] = [
    "checkpoint",
    "cutGeneration",
    "cutAfterSeq",
    "previousGeneration",
    "files",
    "preRewindCheckpoint",
    "head",
];

/// Check the raw `rewind` value has exactly the seven keys, so a missing
/// nullable key is refused rather than read as `null`.
pub fn require_receipt_rewind_shape(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "receipt rewind must be an object".to_owned())?;
    if object.len() != FIELDS.len() || FIELDS.iter().any(|field| !object.contains_key(*field)) {
        return Err("receipt rewind has missing or unsupported fields".into());
    }
    Ok(())
}

fn is_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Validate a decoded `rewind` against its receipt.
///
/// Present only on `resumed`/`resumed_without_context` (whose target is
/// `previousGeneration + 1`) or `failed` with [`REWIND_NOT_RESTARTED`];
/// `restore_failed` only on the latter.
pub fn validate_receipt_rewind(
    receipt: &LifecycleReceipt,
    rewind: &ReceiptRewind,
) -> Result<(), String> {
    validate_event_id_hex("rewind.checkpoint", &rewind.checkpoint)?;
    if let Some(pre) = &rewind.pre_rewind_checkpoint {
        validate_event_id_hex("rewind.preRewindCheckpoint", pre)?;
    }
    if rewind.head.as_deref().is_some_and(|head| !is_oid(head)) {
        return Err("rewind.head must be a lowercase 40- or 64-hex object id".into());
    }
    let generation_ok = |generation: u64| (1..=MAX_SAFE_GENERATION).contains(&generation);
    if !generation_ok(rewind.cut_generation)
        || !generation_ok(rewind.previous_generation)
        || rewind.cut_generation > rewind.previous_generation
        || rewind.cut_after_seq > MAX_SAFE_GENERATION
    {
        return Err("rewind generations must name the cut within the detached lineage".into());
    }
    match receipt.status {
        ReceiptStatus::Resumed | ReceiptStatus::ResumedWithoutContext => {
            let next = receipt
                .session
                .as_ref()
                .map(|target| target.generation)
                .unwrap_or_default();
            if next != rewind.previous_generation.saturating_add(1) {
                return Err("rewind.previousGeneration must precede the receipt's target".into());
            }
            if rewind.files == RewindFilesOutcome::RestoreFailed {
                return Err("rewind.files restore_failed is only on a failed receipt".into());
            }
        }
        ReceiptStatus::Failed
            if receipt
                .error
                .as_ref()
                .is_some_and(|error| error.code == REWIND_NOT_RESTARTED) => {}
        _ => {
            return Err(
                "receipt rewind is only on resumed, resumed_without_context, or failed \
                 REWIND_NOT_RESTARTED"
                    .into(),
            )
        }
    }
    Ok(())
}

/// Whether the generation a rewind opened carries the record forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RewindMemory {
    /// A new conversation seeded from the signed record, cut at the rewind.
    Seeded,
    /// Restarted with no memory: no context package could be attached.
    None,
}

impl RewindMemory {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Seeded => "seeded",
            Self::None => "none",
        }
    }
}

/// The `session_rewound` status item that opens the generation a rewind
/// minted (NIP-CST). Its own shape, like `model_switched`: the closed reason
/// set of `status_item_with_reason` cannot carry a cut. `reason` is added only
/// when it is a member of
/// [`CONTEXT_UNAVAILABLE_REASONS`](crate::coding_session_payload::CONTEXT_UNAVAILABLE_REASONS).
pub fn session_rewound_item(
    command_id: &str,
    rewind: &ReceiptRewind,
    memory: RewindMemory,
    reason: Option<&str>,
) -> Value {
    let mut item = serde_json::json!({
        "kind": "status",
        "status": crate::coding_session_payload::SESSION_REWOUND_STATUS,
        "commandId": command_id,
        "checkpoint": rewind.checkpoint,
        "cutGeneration": rewind.cut_generation,
        "cutAfterSeq": rewind.cut_after_seq,
        "previousGeneration": rewind.previous_generation,
        "files": rewind.files,
        "memory": memory.as_str(),
    });
    if let (Some(object), Some(reason)) = (item.as_object_mut(), reason) {
        if memory == RewindMemory::None
            && crate::coding_session_payload::CONTEXT_UNAVAILABLE_REASONS.contains(&reason)
        {
            object.insert("reason".into(), Value::from(reason));
        }
    }
    item
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_session_command::CodingSessionTarget;
    use crate::coding_session_payload::{
        decode_coding_session_lifecycle_receipt, SESSION_BUSY, TREE_BUSY,
    };

    fn target(generation: u64) -> CodingSessionTarget {
        CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "instance-1".into(),
            session_id: "session-1".into(),
            generation,
        }
    }

    fn rewind(files: RewindFilesOutcome) -> ReceiptRewind {
        ReceiptRewind {
            checkpoint: "ab".repeat(32),
            cut_generation: 4,
            cut_after_seq: 40,
            previous_generation: 4,
            files,
            pre_rewind_checkpoint: Some("cd".repeat(32)),
            head: Some("ef".repeat(20)),
        }
    }

    fn json(receipt: &LifecycleReceipt) -> String {
        serde_json::to_string(receipt).unwrap()
    }

    #[test]
    fn coding_session_rewound_item_has_its_exact_keys() {
        let rewind = rewind(RewindFilesOutcome::Restored);
        let item = session_rewound_item("rewind-1", &rewind, RewindMemory::Seeded, Some("x"));
        let keys: Vec<&String> = item.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 9, "{item}");
        assert_eq!(item["status"], "session_rewound");
        assert_eq!(item["files"], "restored");
        assert_eq!(item["memory"], "seeded");
        let none = session_rewound_item(
            "rewind-1",
            &rewind,
            RewindMemory::None,
            Some("no_umbrella_context"),
        );
        assert_eq!(none["reason"], "no_umbrella_context");
        let unknown = session_rewound_item("rewind-1", &rewind, RewindMemory::None, Some("/tmp/x"));
        assert!(unknown.get("reason").is_none());
    }

    #[test]
    fn coding_session_rewind_receipts_round_trip() {
        let mut resumed = LifecycleReceipt::resumed("rewind-1", &target(5));
        resumed.rewind = Some(rewind(RewindFilesOutcome::Restored));
        let mut not_restarted =
            LifecycleReceipt::failed("rewind-1", REWIND_NOT_RESTARTED, "still remembers");
        not_restarted.rewind = Some(rewind(RewindFilesOutcome::RestoreFailed));
        for receipt in [resumed, not_restarted] {
            assert_eq!(
                decode_coding_session_lifecycle_receipt(&json(&receipt)).unwrap(),
                receipt
            );
        }
        // Without the key, the five-key bytes are exactly the old ones.
        let plain = json(&LifecycleReceipt::resumed("rewind-1", &target(5)));
        assert!(!plain.contains("rewind\":"));
    }

    #[test]
    fn coding_session_rewind_receipts_refuse_inconsistent_shapes() {
        let mut wrong_generation = LifecycleReceipt::resumed("r", &target(9));
        wrong_generation.rewind = Some(rewind(RewindFilesOutcome::Kept));
        let mut failed_restore_on_success = LifecycleReceipt::resumed("r", &target(5));
        failed_restore_on_success.rewind = Some(rewind(RewindFilesOutcome::RestoreFailed));
        let mut refusal_with_rewind = LifecycleReceipt::failed("r", TREE_BUSY, "busy");
        refusal_with_rewind.rewind = Some(rewind(RewindFilesOutcome::Kept));
        let mut busy_with_rewind = LifecycleReceipt::failed("r", SESSION_BUSY, "busy");
        busy_with_rewind.rewind = Some(rewind(RewindFilesOutcome::Kept));
        let mut stopped = LifecycleReceipt::stopped("r", &target(5));
        stopped.rewind = Some(rewind(RewindFilesOutcome::Kept));
        let mut bad_cut = LifecycleReceipt::resumed("r", &target(5));
        let mut cut = rewind(RewindFilesOutcome::Kept);
        cut.cut_generation = 5;
        bad_cut.rewind = Some(cut);
        let mut bad_head = LifecycleReceipt::resumed("r", &target(5));
        let mut head = rewind(RewindFilesOutcome::Kept);
        head.head = Some("HEAD".into());
        bad_head.rewind = Some(head);
        for receipt in [
            wrong_generation,
            failed_restore_on_success,
            refusal_with_rewind,
            busy_with_rewind,
            stopped,
            bad_cut,
            bad_head,
        ] {
            assert!(
                decode_coding_session_lifecycle_receipt(&json(&receipt)).is_err(),
                "accepted {receipt:?}"
            );
        }
        // A missing nullable key is not null, and an extra key is refused.
        let mut ok = LifecycleReceipt::resumed("r", &target(5));
        ok.rewind = Some(rewind(RewindFilesOutcome::Kept));
        let mut value = serde_json::to_value(&ok).unwrap();
        value["rewind"].as_object_mut().unwrap().remove("head");
        assert!(decode_coding_session_lifecycle_receipt(&value.to_string()).is_err());
        let mut value = serde_json::to_value(&ok).unwrap();
        value["rewind"]["extra"] = Value::Bool(true);
        assert!(decode_coding_session_lifecycle_receipt(&value.to_string()).is_err());
        value["rewind"] = Value::Null;
        assert!(decode_coding_session_lifecycle_receipt(&value.to_string()).is_err());
    }
}
