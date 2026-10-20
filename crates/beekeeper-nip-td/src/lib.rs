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
