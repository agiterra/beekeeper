//! What `bee sessions handover` prints, and the plain-text brief a
//! reconstructed execution starts from.
//!
//! # The brief is the checkpoint, not a summary of it
//!
//! [`render_initial_turn`] reorders the checkpoint's own fields into
//! headings and adds nothing. It never paraphrases the task, never merges the
//! unresolved questions into prose, and never omits `missing` to make the
//! handover read better: the whole point of the record is that the next
//! participant learns what did **not** travel, and a brief that dropped that
//! line would be the one thing worse than no brief.
//!
//! When the render would exceed [`MAX_INITIAL_TURN_BYTES`] it truncates at a
//! character boundary and says so in the text, rather than silently producing
//! a shorter brief that reads complete.
//!
//! # Whole-session wording
//!
//! v1 hands over the **whole session** — every execution and every assignment
//! under the umbrella (`docs/HANDOVER_IMPL.md` §1, §9). Every surface here
//! repeats [`WHOLE_SESSION_DISCLOSURE`] so nobody reads a claim as moving one
//! slice of the work.

use beekeeper_core::coding_session_handover::{
    CodingSessionHandoverArtifact, CodingSessionHandoverArtifactKind,
    CodingSessionHandoverCheckpoint, CodingSessionHandoverPreserved,
};

/// The sentence every handover surface repeats about scope.
pub const WHOLE_SESSION_DISCLOSURE: &str =
    "this hands over the whole session: every execution and every assignment under this \
     umbrella is claimed and fenced together, not one slice of the work";

/// The sentence every reconstruction repeats about what did not travel.
pub const RECONSTRUCTION_LIMIT: &str =
    "a reconstruction is a new execution: the original agent's native context did not travel \
     — its resume cursor never leaves the machine it ran on — so this starts from the \
     checkpoint's words and artifacts and nothing else";

/// Longest initial turn a reconstruction will send, in bytes.
///
/// `bee sessions send` refuses an `action.text` over 12 KiB, and a create's
/// `initialTurn` travels the same wire, so the brief is bounded here rather
/// than discovered to be too large after a claim has already been published.
pub const MAX_INITIAL_TURN_BYTES: usize = 12 * 1024;

/// Render one artifact as a line a person can check.
pub fn artifact_line(artifact: &CodingSessionHandoverArtifact) -> String {
    match artifact.kind {
        CodingSessionHandoverArtifactKind::WipRef => format!(
            "wip-ref {} at {} (repo {})",
            artifact.r#ref.as_deref().unwrap_or("<unnamed>"),
            artifact.sha.as_deref().unwrap_or("<unknown>"),
            artifact.repo_ref
        ),
        CodingSessionHandoverArtifactKind::Patch => format!(
            "patch event {} against {} ({} bytes, repo {})",
            artifact.event_id.as_deref().unwrap_or("<unknown>"),
            artifact.base_sha.as_deref().unwrap_or("<unknown>"),
            artifact.bytes.unwrap_or_default(),
            artifact.repo_ref
        ),
        CodingSessionHandoverArtifactKind::Blob => format!(
            "blob {} against {} ({} bytes, repo {})",
            artifact.hash.as_deref().unwrap_or("<unknown>"),
            artifact.base_sha.as_deref().unwrap_or("<unknown>"),
            artifact.bytes.unwrap_or_default(),
            artifact.repo_ref
        ),
    }
}

/// The word a checkpoint's `preserved` reads as in a sentence.
pub const fn preserved_word(preserved: CodingSessionHandoverPreserved) -> &'static str {
    match preserved {
        CodingSessionHandoverPreserved::All => "all",
        CodingSessionHandoverPreserved::Partial => "partial",
        CodingSessionHandoverPreserved::None => "none",
    }
}

/// Truncate `text` to at most `max_bytes`, saying so when it had to.
///
/// The marker is inside the returned string rather than a second return
/// value, because the consumer is an agent reading the brief and it is the
/// agent that must know the record continues past what it was handed.
pub fn bounded_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    const MARKER: &str = "\n\n[truncated: the full checkpoint is on the relay and this brief \
                          was cut to fit the turn size limit]";
    let room = max_bytes.saturating_sub(MARKER.len());
    let mut cut = room.min(text.len());
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{MARKER}", &text[..cut])
}

/// Render the plain-text brief a reconstructed execution starts from.
///
/// `checkpoint_ref` and `author` are named in the text so the new execution's
/// first turn cites the record it came from, and a reader can fetch it.
pub fn render_initial_turn(
    checkpoint: &CodingSessionHandoverCheckpoint,
    checkpoint_ref: Option<&str>,
    author: &str,
    session_ref: &str,
) -> String {
    let mut out = String::new();
    out.push_str("You are continuing another participant's work.\n\n");
    out.push_str(&format!(
        "Session: {session_ref}\nCheckpoint: {} by {author}\n\n",
        checkpoint_ref.unwrap_or("none (no authorized checkpoint existed)")
    ));
    out.push_str(&format!(
        "{RECONSTRUCTION_LIMIT}\n{WHOLE_SESSION_DISCLOSURE}\n\n"
    ));

    out.push_str("## Task\n");
    out.push_str(checkpoint.task.trim());
    out.push_str("\n\n");

    if !checkpoint.decisions.is_empty() {
        out.push_str("## Decisions already taken\n");
        for decision in &checkpoint.decisions {
            out.push_str(&format!("- {} ({})\n", decision.summary, decision.event_id));
        }
        out.push('\n');
    }

    out.push_str("## Revision\n");
    let revision = &checkpoint.revision;
    out.push_str(&format!(
        "- repo: {}\n- branch: {}\n- head: {}\n- base: {}\n- uncommitted changes existed: {}\n- \
         of those bytes, preserved: {}\n\n",
        revision.repo_ref.as_deref().unwrap_or("not stated"),
        revision.branch.as_deref().unwrap_or("not stated"),
        revision.head_sha.as_deref().unwrap_or("not stated"),
        revision.base_sha.as_deref().unwrap_or("not stated"),
        revision.dirty,
        preserved_word(revision.preserved),
    ));

    if !checkpoint.artifacts.is_empty() {
        out.push_str("## Artifacts\n");
        for artifact in &checkpoint.artifacts {
            out.push_str(&format!("- {}\n", artifact_line(artifact)));
        }
        out.push('\n');
    }

    if !checkpoint.tests.is_empty() {
        out.push_str("## Tests, as the previous participant last ran them\n");
        for test in &checkpoint.tests {
            out.push_str(&format!(
                "- {}: {} ({})\n",
                test.name,
                match test.outcome {
                    beekeeper_core::coding_session_handover::CodingSessionHandoverTestOutcome::Passed =>
                        "passed",
                    beekeeper_core::coding_session_handover::CodingSessionHandoverTestOutcome::Failed =>
                        "failed",
                    beekeeper_core::coding_session_handover::CodingSessionHandoverTestOutcome::NotRun =>
                        "not run",
                },
                test.command
            ));
        }
        out.push_str("These are the previous participant's measurements, not this run's.\n\n");
    }

    if !checkpoint.unresolved.is_empty() {
        out.push_str("## Unresolved\n");
        for line in &checkpoint.unresolved {
            out.push_str(&format!("- {line}\n"));
        }
        out.push('\n');
    }

    out.push_str("## Next action\n");
    out.push_str(checkpoint.next_action.trim());
    out.push_str("\n\n");

    out.push_str("## Not preserved\n");
    if checkpoint.missing.is_empty() {
        out.push_str("- nothing was reported missing\n");
    } else {
        for line in &checkpoint.missing {
            out.push_str(&format!("- {line}\n"));
        }
    }
    out.push_str(
        "\nDo not assume anything above is still true of the checkout you are in: verify the \
         head commit and re-run the tests before acting on their outcomes.\n",
    );

    bounded_text(&out, MAX_INITIAL_TURN_BYTES)
}

/// One "what this command verified, and what it did not" block.
///
/// Every handover command prints one. The two lists are separate on purpose:
/// a command that only printed what it checked would leave a reader to infer
/// the rest was checked too.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerificationNotes {
    /// Facts this run established, each with how.
    pub verified: Vec<String>,
    /// Facts this run did **not** establish, each with why.
    pub not_verified: Vec<String>,
}

impl VerificationNotes {
    /// Record something this run established.
    pub fn verified(&mut self, line: impl Into<String>) {
        self.verified.push(line.into());
    }

    /// Record something this run did not establish.
    pub fn not_verified(&mut self, line: impl Into<String>) {
        self.not_verified.push(line.into());
    }

    /// Render both lists as the block every command ends with.
    pub fn render(&self) -> String {
        let mut out = String::from("verified:\n");
        if self.verified.is_empty() {
            out.push_str("  (nothing)\n");
        }
        for line in &self.verified {
            out.push_str(&format!("  - {line}\n"));
        }
        out.push_str("not verified:\n");
        if self.not_verified.is_empty() {
            out.push_str("  (nothing outstanding)\n");
        }
        for line in &self.not_verified {
            out.push_str(&format!("  - {line}\n"));
        }
        out
    }

    /// The same two lists as JSON, for `--json`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "verified": self.verified,
            "notVerified": self.not_verified,
        })
    }
}
