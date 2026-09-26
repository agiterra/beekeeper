//! A seat's clone of the project's agents repository (spec § 4.11).
//!
//! When `team.yml` grants a role `workspace.agents_repo: read | write`, the
//! host cuts a clone of `<slug>-beekeeper-agents` beside the seat's worktree
//! (`<worktree>-agents`) from its own packs cache, points `origin` at the
//! relay so a `write` seat's push lands there, and records the path on the
//! seat's worktree record so disposal removes it with the tree. What the
//! seat may do in it is the relay's push gate and, on Claude, the write
//! fence's call — this module only puts the files where the briefing says.
//!
//! **Why the cache and not the relay.** The clone is cut from this host's
//! packs cache, which is the branch's last sync rather than the relay's tip
//! (ledger 162), and `origin` is pointed at the relay immediately after — so
//! a `write` seat has a real upstream from its first command: `git fetch`
//! brings it to the relay's tip and `git push` goes to the relay under the
//! seat's own credentials and its owner's tier. Cutting from the relay URL
//! instead would make every hire wait on a network clone, would need the
//! host's credentials for a repository the *seat* is meant to authenticate
//! to, and would fetch bytes the cache already holds — for a repository the
//! host had, by definition, just synced to stage this seat's role pack. Spec
//! § 4.11 says "clones the repository from its packs cache", and that is what
//! this does.
//!
//! **Why the clone names a commit, never `--branch`.** `git clone --branch
//! main` resolves the cache's *local* `refs/heads/main` — the branch the
//! very first clone of that cache created — not `refs/remotes/origin/main`,
//! which is the ref [`packs_cache`]'s sync advances. A cache synced after
//! its first clone therefore has a local `main` that never moves, so
//! `--branch main` seated every later seat on the seed commit while the
//! staged pack, the grant, and the seat's briefing all named the synced tip
//! (ledger 172). The fix clones without `--branch`, then checks the clone
//! out to the exact `sha` the caller already staged the pack from — the
//! `packRef.sha` on the seat's custody entry — and verifies `git rev-parse
//! HEAD` before returning, so a seat's agents clone can never silently trail
//! the commit its role pack was read from.
//!
//! **Why a re-stage reuses the directory it finds.** A seat staged a second
//! time — the composer's Reconnect, a restart, a new generation after a
//! relaunch — already has `<worktree>-agents` from its first generation. A
//! second clone there is impossible (git refuses a non-empty destination) and
//! pointless (the cache holds the same objects), so the reuse path fetches
//! the newly staged commit and lands on it. What it will *not* do is adopt
//! whatever directory happens to carry that name: the folder must prove it is
//! this project's agents clone by its `origin` — the packs cache it was cut
//! from, or the relay URL the first stage re-pointed it at — and any other
//! directory is refused by name rather than fetched into (ledger 187).

use std::path::{Path, PathBuf};

use tauri::AppHandle;

use crate::app_state::AppState;
use crate::coding_sessions::workdir_store::{
    load_workdir_store_readonly, CodingSessionWorkdirStore,
};
use crate::commands::project_git_exec::{
    build_local_clone_git_auth_config, run_git, GitAuthConfig,
};
use crate::managed_agents::packs_cache;

/// Where a seat's agents clone goes: the worktree's sibling `<name>-agents`.
///
/// Composed in `buzz-core` and re-exported here, because `bee` — running
/// *inside* a seat — composes the same sibling to find the project's model
/// registry (`buzz_core::model_registry_source`, ledger 178(a)). A host that
/// cut `-agents` while the CLI looked for another name would leave a seat
/// unable to find the repository it was given.
pub use buzz_core_pkg::model_registry_source::seat_agents_clone_path;

/// Who the clone is for, so it can be given a commit identity of its own.
///
/// Borrowed rather than owned: every field already exists on the staging
/// command's arguments, and copying them here would be a second place for the
/// seat's key to live.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SeatCloneIdentity<'a> {
    /// The seat's own pubkey, 64 lowercase hex.
    pub pubkey: &'a str,
    /// The seat's role word, for the author name in `git log`.
    pub role: &'a str,
}

/// Cut (or reuse) the seat's clone of the agents repository `source` names,
/// landed on `sha` — the resolved commit the caller already staged the
/// seat's pack from — and record it on the seat's worktree record.
///
/// Refuses a source that is not an agents repository (its `path` is not the
/// repository root) or that pins a sha rather than a branch: a seat that
/// may write needs a branch to push. Also refuses when the packs cache does
/// not hold `sha` at all, by name.
pub(crate) fn cut_seat_agents_clone(
    app: &AppHandle,
    state: &AppState,
    source: &packs_cache::ProjectPackSource,
    worktree: &Path,
    sha: &str,
    seat: SeatCloneIdentity<'_>,
) -> Result<PathBuf, String> {
    if !buzz_core_pkg::project_pack_source::is_root_pack_path(&source.path) {
        return Err(format!(
            "the project's role source ({}, path {}) is not an agents repository, so this seat \
             cannot be given one",
            source.repo, source.path
        ));
    }
    let Some(ref_name) = source.git_ref.as_deref() else {
        return Err(format!(
            "the project's role source ({}) pins a commit rather than a branch, so a seat has \
             no branch of the agents repository to work on",
            source.repo
        ));
    };
    let branch = ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name);
    let (owner, id) = packs_cache::parse_repo_coordinate(&source.repo)?;
    let packs_root = packs_cache::packs_root(app)?;
    let cache = packs_cache::packs_checkout_dir(&packs_root, &owner, &id);
    if !cache.join(".git").is_dir() {
        return Err(format!(
            "this computer has no packs cache for {} to clone the agents repository from",
            source.repo
        ));
    }
    let dest = seat_agents_clone_path(worktree).ok_or_else(|| {
        format!(
            "{} has no name to put an agents clone beside",
            worktree.display()
        )
    })?;
    // The clone, the fetch-by-sha on reuse, and the `set-url` after them are
    // all local: the remote is a directory in this host's packs cache, and
    // nothing here talks to the relay. So this runs with the local
    // configuration, which carries no credential helper and no nsec — and,
    // unlike every remote configuration, allows git's `file` transport. With
    // the remote configuration this clone failed outright, `fatal: transport
    // 'file' not allowed`, and the hire it was staging died with it (ledger
    // 169).
    let auth = build_local_clone_git_auth_config()?;
    land_seat_agents_clone_on_sha(
        &cache,
        &dest,
        branch,
        sha,
        &relay_clone_url_suffix(&owner, &id),
        &auth,
    )?;
    let relay_http =
        crate::relay::relay_http_base_url(&crate::relay::relay_ws_url_with_override(state));
    let origin = packs_cache::packs_clone_url(&relay_http, &owner, &id);
    run_git(
        &["remote", "set-url", "origin", "--", &origin],
        Some(&dest),
        &auth,
    )?;
    // A clone is a checkout the seat may commit in, and it inherits no
    // identity either — the same gap that parked a finished lane for 49
    // minutes in the seat's own worktree (ledger 236(a), 239). Best-effort and
    // named on failure: a clone with no identity is still a usable read.
    if let Err(error) = crate::commands::coding_session_seat_hooks::ensure_seat_commit_identity(
        &dest,
        seat.pubkey,
        seat.role,
        // The repository's own identifier, which is what a person reading this
        // clone's `git log` would recognise — not the `30617:…` coordinate.
        Some(id.as_str()),
    ) {
        tracing::warn!(
            target: "seat_agents_clone",
            %error,
            "the seat's agents clone has no commit identity; a commit made in it would have to \
             invent an author"
        );
    }
    if let Err(error) =
        crate::coding_sessions::workdir_store::attach_agents_clone(app, state, worktree, &dest)
    {
        tracing::warn!(
            target: "seat_agents_clone",
            %error,
            "the seat's agents clone was cut but not recorded on its worktree; it will not be \
             removed with the tree"
        );
    }
    Ok(dest)
}

/// The worktree a seat's agents clone (spec § 4.11) goes beside, resolved
/// from the caller when it named one and from this host's own records of the
/// execution when it did not.
///
/// # Why there are three records and not one (ledger 187, ledger 188)
///
/// A seat is staged twice in its life: once by the create that cut its tree,
/// and again by every reconnect, restart and post-relaunch generation. Only
/// the first caller holds the directory, and refusing the second for want of
/// it made a granted lead unreconnectable. Ledger 187 answered from the
/// desktop's `worktrees` records, keyed by provider session id — and that
/// answered for nothing, because **44 of the 47 records on the machine where
/// it was proven carry no session id at all**: only the solo create path
/// (`useCodingSessionWorktreeRecorder`) ever wrote one, and a team launch's
/// lead tree left no `worktrees` record whatever. So the chain is, in order:
///
/// 1. the caller's own path, when it has one;
/// 2. the desktop's `worktrees` record for this session id — exact when the
///    create path recorded one, which from ledger 188 a team launch does too;
/// 3. the **provider's** own snapshot of the execution, `sessions[<id>].cwd`
///    — the directory the agent is literally running in, written by the
///    process that spawned it;
/// 4. the desktop's one-shot create hint, `pending[<createCommandId>]`, with
///    the command id read from that same provider record.
///
/// Rungs 2–4 are records, not assertions, so each candidate is checked to be
/// a directory and a git work tree before it is returned; a record naming
/// something else is refused by name. The caller's own path is taken as
/// given: a create passes the tree it has just cut, and adding a git probe
/// there would invent a new way for a hire to fail.
///
/// Nothing here writes, and nothing takes the workdir store's lock.
pub(crate) fn resolve_seat_agents_worktree(
    app: &AppHandle,
    state: &AppState,
    caller: Option<&Path>,
    session_id: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(caller) = caller {
        return Ok(caller.to_path_buf());
    }
    let Some(session_id) = session_id else {
        return Err(
            "the caller named no worktree and this execution carries no provider session id, so \
             this computer has nothing to look one up by"
                .to_string(),
        );
    };
    let store = load_workdir_store_readonly(app).map_err(|error| {
        format!(
            "the caller named no worktree and this computer could not read its own record of the \
             trees it cut: {error}"
        )
    })?;
    let provider = provider_session_record(app, state, session_id)?;
    seat_worktree_from_records(session_id, &store, &provider)
}

/// Rungs 2 to 4 of [`resolve_seat_agents_worktree`], over records already
/// read — so the whole chain, including its refusals, is exercisable without
/// an [`AppHandle`], a provider on disk, or a desktop store file.
fn seat_worktree_from_records(
    session_id: &str,
    store: &CodingSessionWorkdirStore,
    provider: &ProviderSessionFacts,
) -> Result<PathBuf, String> {
    // Rung 2. A `worktrees` record that names two different directories for
    // one session is a contradiction, not a fallback, so it refuses here
    // rather than quietly trying the next record.
    if let Some(recorded) = seat_worktree_from_record(session_id, store)? {
        return usable_seat_worktree(recorded, "this computer's worktree record");
    }
    // Rung 3, then rung 4, both off the provider's record of this execution.
    if let Some(cwd) = provider.cwd.clone() {
        return usable_seat_worktree(cwd, "the provider's record of this execution");
    }
    if let Some(command_id) = provider.command_id.as_deref() {
        if let Some(hint) = store.pending.get(command_id) {
            return usable_seat_worktree(
                hint.clone(),
                &format!("this computer's create hint for command {command_id}"),
            );
        }
    }
    Err(format!(
        "the caller named no worktree and nothing on this computer records one for provider \
         session {session_id} (looked in its own coding-session worktree records, in the \
         provider's record of that execution, and in its create hint for {})",
        provider
            .command_id
            .as_deref()
            .map(|id| format!("command {id}"))
            .unwrap_or_else(|| "that execution, which names no create command".to_string())
    ))
}

/// A candidate path from a record, admitted only if it is still a git work
/// tree on disk.
///
/// `source` names the record it came from, so a refusal says which of the
/// three answered and what was wrong with its answer — the difference
/// between "nobody wrote it down" and "what was written down is gone".
fn usable_seat_worktree(path: PathBuf, source: &str) -> Result<PathBuf, String> {
    if !path.is_dir() {
        return Err(format!(
            "{source} names {} for this seat, and no such directory exists on this computer",
            path.display()
        ));
    }
    // `.git` is a **file** in a linked worktree and a directory in an ordinary
    // clone; both are work trees, and both are legitimate homes for a seat.
    if !path.join(".git").exists() {
        return Err(format!(
            "{source} names {} for this seat, and that directory is not a git work tree",
            path.display()
        ));
    }
    Ok(path)
}

/// The desktop's own worktree record for the execution running as
/// `session_id`: `Some` when exactly one directory is recorded, `None` when
/// none is, and a refusal when two disagree.
///
/// `None` is deliberately not an error. Ledger 188: this record exists only
/// where a create path wrote one, and for most of the trees on a real machine
/// no create path ever did — so an absent record is an ordinary state that
/// the next rung answers, while two contradictory records are a fault that
/// must not be papered over by falling through.
fn seat_worktree_from_record(
    session_id: &str,
    store: &CodingSessionWorkdirStore,
) -> Result<Option<PathBuf>, String> {
    let mut matches: Vec<&Path> = store
        .worktrees
        .values()
        .filter(|record| record.session_id.as_deref() == Some(session_id))
        .map(|record| record.path.as_path())
        .collect();
    matches.sort_unstable();
    matches.dedup();
    match matches.as_slice() {
        [] => Ok(None),
        [only] => Ok(Some(only.to_path_buf())),
        many => Err(format!(
            "this computer records {} different worktrees for provider session {session_id} \
             ({}), so it cannot tell which one an agents clone belongs beside",
            many.len(),
            many.iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The two host-local facts the provider's own snapshot holds about one
/// execution: where it runs, and the create command that minted it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ProviderSessionFacts {
    /// `sessions[<id>].cwd` — the directory the agent process runs in.
    cwd: Option<PathBuf>,
    /// `sessions[<id>].commandId` — the key the desktop's create hint is
    /// filed under.
    command_id: Option<String>,
}

/// Longest `state.json` this reader will parse. The provider's snapshot on a
/// busy machine is tens of kilobytes; a file orders of magnitude past that is
/// refused rather than parsed, the same bound the workdir store uses.
const MAX_PROVIDER_STATE_BYTES: u64 = 8 * 1024 * 1024;

/// Read what the provider recorded about one execution.
///
/// **Why the desktop reads another process's file.** `cwd` is deliberately
/// host-local — it is never in a signed event — so the only parties that can
/// know it are the two processes on this machine, and the provider is the one
/// that spawned the agent *in* it. The desktop already reads this directory
/// (the redaction vault) and writes into it (the seat custody file and the
/// projects view), so this is the same seam, not a new one.
///
/// Read as JSON rather than through [`buzz_session_provider_pkg::state`]'s
/// own store, which creates the directory, re-restricts every file in it and
/// replays the command ledger — all of it write-shaped work this lookup has
/// no business doing behind a running provider's back. The two keys it reads
/// are pinned against the real `SessionRecord` by a test below, so a rename
/// upstream fails here rather than silently answering `None` forever.
///
/// A missing file, an unprovisioned provider, or an execution this provider
/// never ran are all "nothing recorded", not errors: the caller's final
/// refusal names every place it looked.
fn provider_session_record(
    app: &AppHandle,
    state: &AppState,
    session_id: &str,
) -> Result<ProviderSessionFacts, String> {
    let relay_url = crate::relay::relay_ws_url_with_override(state);
    let store = crate::session_provider::store::load_provider_store(app)?;
    let Some(record) = store.get(&relay_url) else {
        return Ok(ProviderSessionFacts::default());
    };
    let path = crate::session_provider::provider_state_dir(app, &record.provider_pubkey)?
        .join("state.json");
    let Some(text) = read_bounded(&path)? else {
        return Ok(ProviderSessionFacts::default());
    };
    let snapshot: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("could not read the provider's own session record: {error}"))?;
    Ok(provider_session_facts(&snapshot, session_id))
}

/// Read `path` when it is there and not absurd, as text.
fn read_bounded(path: &Path) -> Result<Option<String>, String> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.len() > MAX_PROVIDER_STATE_BYTES => {
            return Err(format!(
                "the provider's session record at {} is too large to read",
                path.display()
            ))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "could not read the provider's own session record: {error}"
            ))
        }
    }
    std::fs::read_to_string(path)
        .map(Some)
        .map_err(|error| format!("could not read the provider's own session record: {error}"))
}

/// Pull one execution's two facts out of a parsed provider snapshot.
///
/// Split from the file read so the shape can be exercised without a provider
/// on disk. Anything absent or of the wrong type reads as `None` — this is a
/// lookup in someone else's file, and the caller's refusal already names it
/// as one of the places that had no answer.
fn provider_session_facts(snapshot: &serde_json::Value, session_id: &str) -> ProviderSessionFacts {
    let Some(record) = snapshot.get("sessions").and_then(|s| s.get(session_id)) else {
        return ProviderSessionFacts::default();
    };
    ProviderSessionFacts {
        cwd: record
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .filter(|cwd| !cwd.trim().is_empty())
            .map(PathBuf::from),
        command_id: record
            .get("commandId")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(str::to_owned),
    }
}

/// The tail of the relay clone URL a staged agents clone's `origin` carries,
/// `/git/<owner>/<id>` — the part [`packs_cache::packs_clone_url`] writes
/// after the relay's own base, which is the only part that identifies the
/// repository rather than the community it was reached through.
///
/// Compared as a suffix on purpose: a re-stage after a port change, a
/// protocol change or a community switch must still recognise the clone it
/// cut, and the repository coordinate is what makes it this project's.
fn relay_clone_url_suffix(owner: &str, id: &str) -> String {
    format!("/git/{owner}/{id}")
}

/// What is already sitting where the seat's agents clone belongs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingSeatAgentsClone {
    /// Nothing is there; the clone has to be cut.
    None,
    /// This project's agents clone from an earlier stage; advance it.
    Reusable,
}

/// Decide whether `dest` may be advanced as this seat's agents clone.
///
/// Reuse is admitted only by evidence: `dest` is a git work tree whose
/// `origin` is either the packs cache it was cut from (a clone that was never
/// re-pointed) or a relay URL naming the same repository
/// ([`relay_clone_url_suffix`], which is what [`cut_seat_agents_clone`] sets
/// immediately after cutting). Anything else — a person's folder, an
/// unrelated checkout, a half-written directory — is refused **by name**,
/// because fetching a staged commit into a repository the host did not cut
/// would quietly hand the seat someone else's history under the project's
/// name (ledger 187).
fn existing_seat_agents_clone(
    dest: &Path,
    cache: &Path,
    origin_suffix: &str,
    auth: &GitAuthConfig,
) -> Result<ExistingSeatAgentsClone, String> {
    if !dest.exists() {
        return Ok(ExistingSeatAgentsClone::None);
    }
    if !dest.is_dir() {
        return Err(format!(
            "{} already exists and is not a directory, so this seat's agents clone cannot go there",
            dest.display()
        ));
    }
    if !dest.join(".git").is_dir() {
        return Err(format!(
            "{} already exists and is not a git clone of the project's agents repository, so this              seat's agents clone cannot go there",
            dest.display()
        ));
    }
    let origin = run_git(&["remote", "get-url", "origin"], Some(dest), auth).map_err(|error| {
        format!(
            "{} already exists and has no git remote named origin to identify it as the project's              agents clone: {error}",
            dest.display()
        )
    })?;
    let origin = origin.trim();
    if origin == cache.to_string_lossy() || origin.ends_with(origin_suffix) {
        return Ok(ExistingSeatAgentsClone::Reusable);
    }
    Err(format!(
        "{} already exists and its origin is {origin}, which is neither this computer's packs          cache at {} nor a relay URL for the project's agents repository (…{origin_suffix}), so          this seat's agents clone cannot go there",
        dest.display(),
        cache.display()
    ))
}

/// Cut (or advance) `dest` from `cache`, landed on `sha` and checked out to
/// `branch` — the part of [`cut_seat_agents_clone`] that touches only the
/// cache and the destination, so it can be exercised against throwaway
/// repositories without an [`AppHandle`]/[`AppState`].
///
/// No `--branch` on the clone: that resolves the cache's *local*
/// `refs/heads/*`, which [`packs_cache`]'s sync never advances — it fetches
/// into `refs/remotes/origin/*` and checks out detached. A cache synced
/// after its first clone therefore has a stale local branch, and `--branch`
/// would seat the seat on the seed commit while the staged pack and the
/// grant both name the synced tip (ledger 172). A plain clone pulls every
/// ref the cache advertises, including the synced remote-tracking ones, so
/// `sha` is reachable to check out below even when the cache's local branch
/// never moved. When `dest` already exists (a re-staged seat), `origin`
/// there already points at the relay from a prior stage, so the update
/// fetches the exact commit from `cache` directly rather than through it.
/// Either way, the final `git rev-parse HEAD` is checked against `sha`
/// before returning, so a mismatch is caught here rather than trusted.
///
/// `origin_suffix` is how an existing `dest` proves it is this project's
/// clone before anything is fetched into it — see
/// [`existing_seat_agents_clone`].
fn land_seat_agents_clone_on_sha(
    cache: &Path,
    dest: &Path,
    branch: &str,
    sha: &str,
    origin_suffix: &str,
    auth: &GitAuthConfig,
) -> Result<(), String> {
    let cache_str = cache.to_string_lossy().into_owned();
    let dest_str = dest.to_string_lossy().into_owned();
    // Nothing is checked out on the host: a fresh clone is taken with
    // nothing checked out, and an existing one only fetches. The files are
    // written inside the clone's project boundary, because a seat can write
    // its own clone's configuration (filters, fsmonitor) and the host must
    // not run what it planted.
    let fresh = existing_seat_agents_clone(dest, cache, origin_suffix, auth)?
        == ExistingSeatAgentsClone::None;
    if fresh {
        run_git(
            &[
                "clone",
                "--quiet",
                "--no-checkout",
                "--",
                &cache_str,
                &dest_str,
            ],
            None,
            auth,
        )
        .map_err(|error| format!("could not clone the agents repository for this seat: {error}"))?;
    } else {
        run_git(
            &["fetch", "--quiet", "--", &cache_str, sha],
            Some(dest),
            auth,
        )
        .map_err(|error| {
            format!("could not update the seat's agents clone to the staged commit {sha}: {error}")
        })?;
    }
    let name = format!("agents-clone\n{}", dest.display());
    crate::coding_sessions::host_git::materialize(
        &crate::coding_sessions::host_git::Workspace {
            tree: dest,
            repo_root: None,
            name: &name,
            host_branch: Some(branch),
            host_read: &[],
        },
        &["checkout", "--quiet", "-B", branch, sha],
        sha,
        fresh,
    )
    .map_err(|error| {
        format!(
            "the seat's agents clone could not be put on the staged commit {sha} from the packs \
             cache at {}: {error}",
            cache.display()
        )
    })?;
    let head = run_git(&["rev-parse", "HEAD"], Some(dest), auth)?;
    if head.trim() != sha {
        return Err(format!(
            "the seat's agents clone landed on {} instead of the staged commit {sha}",
            head.trim()
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "seat_agents_clone_tests.rs"]
mod tests;
