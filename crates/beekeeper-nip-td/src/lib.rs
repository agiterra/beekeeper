//! NIP-TD: shared project to-do lists (kind 44248), as pure functions.
//!
//! The wire contract is `docs/nips/NIP-TD.md`; the fold's rules are pinned by
//! `conformance/project-todo-fold/`. `beekeeper-core` re-exports everything
//! here unchanged and adds the one thing that needs a Nostr runtime — taking
//! a signed `nostr::Event` apart. Everything else lives here so that a
//! WebAssembly plugin can link it: the Planning plugin folds records and
//! drafts ops with exactly this code, never a copy of it.

pub mod fold;
pub mod fractional_rank;
pub mod project_todo;

/// Kind of a project to-do op. The wire contract is `docs/nips/NIP-TD.md`;
/// `beekeeper_core::kind::KIND_PROJECT_TODO_OP` is this constant.
pub const KIND_PROJECT_TODO_OP: u32 = 44248;

/// How far a signed event's `created_at` may sit from the relay's clock at
/// ingest, either way. Relay ingest refuses anything outside it
/// (`beekeeper-relay` `handlers::ingest`, whose `MAX_TIMESTAMP_DRIFT_SECS`
/// a compile-time assertion there ties to this value).
pub const INGEST_TIMESTAMP_DRIFT_SECS: u64 = 900;

/// How far behind the checker's clock a plugin's drafted op may be stamped.
/// The draft is checked when it is drafted and again when it is signed, but
/// ingest judges it later, after the reply, the signing and the publish. The
/// [`INGEST_TIMESTAMP_DRIFT_SECS`] − this = 300 s is that transport margin:
/// a draft that passes here is not one ingest refuses as stale on arrival.
pub const DRAFT_PAST_SECS: u64 = 600;

/// How far ahead of the checker's clock a drafted op may be stamped. A
/// later ingest only makes a future stamp safer, so this sits just inside
/// [`INGEST_TIMESTAMP_DRIFT_SECS`]; a plugin that orders ops by bumping
/// past the latest stamp must stay inside it (the Planning plugin does).
pub const DRAFT_FUTURE_SECS: u64 = 890;

const _: () = assert!(DRAFT_PAST_SECS + 300 <= INGEST_TIMESTAMP_DRIFT_SECS);
const _: () = assert!(DRAFT_FUTURE_SECS < INGEST_TIMESTAMP_DRIFT_SECS);

/// Whether a drafted op's `created_at` is one every checker of a plugin's
/// drafts — the relay's plugin host, `bee` and the desktop — accepts at
/// `now` (all in Unix seconds): inside `now − DRAFT_PAST_SECS ..= now +
/// DRAFT_FUTURE_SECS`, both ends inclusive. It never rewrites the stamp:
/// `created_at` orders an op against its rivals in the fold, so a draft
/// outside the window is refused, not re-stamped.
pub fn check_draft_created_at(created_at: u64, now: u64) -> Result<(), String> {
    if created_at.saturating_add(DRAFT_PAST_SECS) < now
        || created_at > now.saturating_add(DRAFT_FUTURE_SECS)
    {
        return Err(format!(
            "created_at {created_at} is outside {DRAFT_PAST_SECS} s behind ..= {DRAFT_FUTURE_SECS} s ahead of {now}"
        ));
    }
    Ok(())
}

/// Normalize a project coordinate string to `30621:<lowercase-hex>:<dtag>`,
/// or `None` if it is not a well-formed 30621 coordinate.
///
/// Only the live project kind is accepted: this relay never stored the
/// fork-era `30178:` project coordinates, and 30178 is the team-catalog kind
/// here — accepting it would alias an unrelated kind into project refs.
/// `beekeeper_core::kind::normalize_project_coordinate` is this function.
pub fn normalize_project_coordinate(value: &str) -> Option<String> {
    let mut parts = value.splitn(3, ':');
    let kind = parts.next()?;
    let pubkey = parts.next()?;
    let dtag = parts.next()?;
    if kind != "30621" {
        return None;
    }
    if pubkey.len() != 64 || !pubkey.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if dtag.is_empty() || dtag.chars().count() > 64 || dtag.chars().any(char::is_control) {
        return None;
    }
    Some(format!("30621:{}:{dtag}", pubkey.to_ascii_lowercase()))
}

#[cfg(test)]
mod draft_window_tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;

    #[test]
    fn the_window_is_inclusive_at_both_edges_and_closed_beyond_them() {
        assert!(check_draft_created_at(NOW, NOW).is_ok());
        assert!(check_draft_created_at(NOW - DRAFT_PAST_SECS, NOW).is_ok());
        assert!(check_draft_created_at(NOW - DRAFT_PAST_SECS - 1, NOW).is_err());
        assert!(check_draft_created_at(NOW + DRAFT_FUTURE_SECS, NOW).is_ok());
        assert!(check_draft_created_at(NOW + DRAFT_FUTURE_SECS + 1, NOW).is_err());
        // The cases the old hour-long window let through, and a zero stamp.
        assert!(check_draft_created_at(NOW - 1_800, NOW).is_err());
        assert!(check_draft_created_at(NOW + 86_400, NOW).is_err());
        assert!(check_draft_created_at(0, NOW).is_err());
    }

    /// The cross-language contract: the desktop seals against the same
    /// window, and its test (`desktop/src/shared/plugins/pluginDrafts.test.mjs`)
    /// reads the same file, so a change on either side fails one of them.
    #[test]
    fn the_window_is_the_one_the_desktop_is_pinned_to() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../testdata/plugins/draft-window.json"))
                .expect("the draft-window fixture is JSON");
        assert_eq!(fixture["pastSecs"].as_u64(), Some(DRAFT_PAST_SECS));
        assert_eq!(fixture["futureSecs"].as_u64(), Some(DRAFT_FUTURE_SECS));
    }
}
