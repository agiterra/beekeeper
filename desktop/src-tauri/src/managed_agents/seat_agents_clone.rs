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
/// from the caller when it named one and from this host's own record of the
/// execution when it did not.
///
/// Reads the desktop's worktree store without taking its lock or writing
/// anything: this is a lookup, and the record it reads is written only by the
/// path that cut the tree.
pub(crate) fn resolve_seat_agents_worktree(
    app: &AppHandle,
    caller: Option<&Path>,
    session_id: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(caller) = caller {
        return Ok(caller.to_path_buf());
    }
    let store = load_workdir_store_readonly(app).map_err(|error| {
        format!(
            "the caller named no worktree and this computer could not read its own record of the \
             trees it cut: {error}"
        )
    })?;
    seat_worktree_from_record(session_id, &store)
}

/// The recorded worktree of the execution running as `session_id`.
///
/// # Why this exists (ledger 187)
///
/// A seat is staged twice in its life: once by the create that hired it, and
/// again by every reconnect, restart and post-relaunch generation. Only the
/// first caller holds the directory — the create cut it. The composer's
/// Reconnect holds the execution's provider session id and nothing else, so a
/// lead with an `agents_repo` grant could not be reconnected at all: staging
/// refused for want of a path this host had itself written into
/// `coding-session-workdirs.json` the moment it cut the tree.
///
/// The provider resolves a live session's cwd from exactly this field
/// (`CodingSessionSeatWorktree::session_id`, republished as the projects
/// view's `sessions` map), so reading it here gives the clone the same
/// directory the seat is actually running in rather than a second opinion.
///
/// Every failure names what was looked for. A guess would put a clone — and,
/// on disposal, a removal — beside a directory nobody asked for.
fn seat_worktree_from_record(
    session_id: Option<&str>,
    store: &CodingSessionWorkdirStore,
) -> Result<PathBuf, String> {
    let Some(session_id) = session_id else {
        return Err(
            "the caller named no worktree and this execution carries no provider session id, so \
             this computer has nothing to look one up by"
                .to_string(),
        );
    };
    let mut matches: Vec<&Path> = store
        .worktrees
        .values()
        .filter(|record| record.session_id.as_deref() == Some(session_id))
        .map(|record| record.path.as_path())
        .collect();
    matches.sort_unstable();
    matches.dedup();
    match matches.as_slice() {
        [] => Err(format!(
            "the caller named no worktree and this computer records none for provider session \
             {session_id} (looked in its own coding-session worktree records)"
        )),
        [only] => Ok(only.to_path_buf()),
        many => Err(format!(
            "the caller named no worktree and this computer records {} different worktrees for \
             provider session {session_id} ({}), so it cannot tell which one an agents clone \
             belongs beside",
            many.len(),
            many.iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
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
    if existing_seat_agents_clone(dest, cache, origin_suffix, auth)?
        == ExistingSeatAgentsClone::None
    {
        run_git(
            &["clone", "--quiet", "--", &cache_str, &dest_str],
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
    run_git(
        &["checkout", "--quiet", "-B", branch, sha],
        Some(dest),
        auth,
    )
    .map_err(|error| {
        format!(
            "the packs cache at {} has no commit {sha} to check the seat's agents clone out to: \
             {error}",
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
mod tests {
    use super::*;

    /// The relay-URL tail the tests' `origin` carries, so a reuse is admitted
    /// the same way [`cut_seat_agents_clone`] admits one in production.
    const SEAT_ORIGIN_SUFFIX: &str = "/git/deadbeef/seat-slug-beekeeper-agents";

    /// The clone's own transport, in a throwaway repository.
    ///
    /// This is ledger 169's first bug as a test: with the configuration every
    /// *remote* operation uses, git refuses a clone whose remote is a path
    /// (`protocol.file.allow=never`), and the hire that was staging this seat
    /// dies with it. With the local clone configuration the same command
    /// succeeds. Nothing here touches the real repository or a real cache.
    #[test]
    fn a_clone_from_the_cache_needs_the_file_transport_only_the_local_config_allows() {
        use crate::commands::project_git_exec::{
            build_git_auth_config_for_keys, build_local_clone_git_auth_config,
            build_test_git_auth_config,
        };
        let temp = tempfile::tempdir().expect("temp");
        let source = temp.path().join("cache");
        std::fs::create_dir(&source).expect("source dir");
        let seed = build_test_git_auth_config().expect("seed auth");
        run_git(
            &["init", "--quiet", "--initial-branch", "main"],
            Some(&source),
            &seed,
        )
        .expect("git init");
        std::fs::write(source.join("team.yml"), "schema: beekeeper-team/v1\n").expect("team.yml");
        run_git(&["add", "team.yml"], Some(&source), &seed).expect("git add");
        run_git(&["commit", "--quiet", "-m", "seed"], Some(&source), &seed).expect("git commit");

        let source_str = source.to_string_lossy().into_owned();
        let remote_auth =
            build_git_auth_config_for_keys(&nostr::Keys::generate()).expect("remote auth");
        let refused_dest = temp.path().join("refused").to_string_lossy().into_owned();
        let refusal = run_git(
            &[
                "clone",
                "--quiet",
                "--branch",
                "main",
                "--",
                &source_str,
                &refused_dest,
            ],
            None,
            &remote_auth,
        )
        .expect_err("the remote configuration must refuse a clone from a path");
        assert!(
            refusal.contains("transport 'file' not allowed"),
            "unexpected refusal: {refusal}"
        );

        let dest = temp.path().join("seat-agents");
        let dest_str = dest.to_string_lossy().into_owned();
        run_git(
            &[
                "clone",
                "--quiet",
                "--branch",
                "main",
                "--",
                &source_str,
                &dest_str,
            ],
            None,
            &build_local_clone_git_auth_config().expect("local clone auth"),
        )
        .expect("the local clone configuration clones from a path");
        assert!(dest.join(".git").is_dir(), "no clone at {}", dest.display());
        assert!(dest.join("team.yml").is_file());
    }

    /// Build a packs-cache-shaped repo the way `sync_packs_checkout` leaves
    /// one: a first clone of a throwaway `seed` (which gives the cache the
    /// local `refs/heads/<branch>` the sync never advances), then `seed`
    /// gains a second commit and the cache is synced the way
    /// `sync_packs_checkout` does it — `git fetch origin` into
    /// `refs/remotes/origin/*`, then a detached checkout — never touching
    /// the cache's own local branch. Returns the cache path and both commits.
    fn build_lagging_cache(
        temp: &tempfile::TempDir,
        branch: &str,
        auth: &GitAuthConfig,
    ) -> (PathBuf, String, String) {
        let seed = temp.path().join("seed");
        std::fs::create_dir(&seed).expect("seed dir");
        run_git(
            &["init", "--quiet", "--initial-branch", branch],
            Some(&seed),
            auth,
        )
        .expect("git init seed");
        std::fs::write(
            seed.join("team.yml"),
            "schema: beekeeper-team/v1\nroles:\n  runner: {}\n",
        )
        .expect("write team.yml");
        run_git(&["add", "team.yml"], Some(&seed), auth).expect("git add");
        run_git(&["commit", "--quiet", "-m", "seed"], Some(&seed), auth).expect("git commit seed");
        let seed_str = seed.to_string_lossy().into_owned();

        let cache = temp.path().join("cache");
        let cache_str = cache.to_string_lossy().into_owned();
        run_git(
            &["clone", "--quiet", "--", &seed_str, &cache_str],
            None,
            auth,
        )
        .expect("clone the cache from the seed");
        let seed_sha = run_git(&["rev-parse", "HEAD"], Some(&seed), auth)
            .expect("seed sha")
            .trim()
            .to_string();

        // Advance the source (what the relay's tip does between syncs), then
        // sync the cache exactly as `sync_packs_checkout` does: fetch into
        // `refs/remotes/origin/*` and check out detached. The cache's local
        // `refs/heads/<branch>` is never touched, so it stays at `seed_sha`.
        std::fs::write(
            seed.join("team.yml"),
            "schema: beekeeper-team/v1\nroles:\n  runner:\n    workspace:\n      agents_repo: read\n",
        )
        .expect("write advanced team.yml");
        run_git(&["add", "team.yml"], Some(&seed), auth).expect("git add advance");
        run_git(
            &["commit", "--quiet", "-m", "grant runner agents_repo: read"],
            Some(&seed),
            auth,
        )
        .expect("git commit advance");
        let synced_sha = run_git(&["rev-parse", "HEAD"], Some(&seed), auth)
            .expect("synced sha")
            .trim()
            .to_string();
        run_git(&["fetch", "--quiet", "origin"], Some(&cache), auth).expect("sync fetch");
        run_git(
            &["checkout", "--quiet", "--detach", "origin/main"],
            Some(&cache),
            auth,
        )
        .expect("sync detached checkout");

        assert_ne!(
            seed_sha, synced_sha,
            "the test needs two distinct commits to prove a lag"
        );
        assert_eq!(
            run_git(&["rev-parse", "refs/heads/main"], Some(&cache), auth)
                .expect("cache local main")
                .trim(),
            seed_sha,
            "the cache's local branch must still be the seed commit, or this test proves nothing"
        );
        (cache, seed_sha, synced_sha)
    }

    /// Ledger 172, as a red/green test: `--branch main` reads the cache's
    /// *local* `main`, which stays on the seed commit forever once the cache
    /// is synced by fetch-and-detach. Landing the clone on the resolved
    /// `sha` instead reaches the commit the seat's pack was actually staged
    /// from, checks it out as a real branch (so a `write` seat has one to
    /// push), and the `remote set-url` [`cut_seat_agents_clone`] runs right
    /// after lands cleanly on top.
    #[test]
    fn a_lagging_local_branch_does_not_strand_the_seat_on_the_seed_commit() {
        let temp = tempfile::tempdir().expect("temp");
        let auth =
            crate::commands::project_git_exec::build_test_git_auth_config().expect("test auth");
        let (cache, seed_sha, synced_sha) = build_lagging_cache(&temp, "main", &auth);

        // What `--branch main` would have done: it names the ref, not the
        // commit, so it resolves the cache's stale local branch.
        let stale_dest = temp.path().join("stale-branch-name");
        run_git(
            &[
                "clone",
                "--quiet",
                "--branch",
                "main",
                "--",
                &cache.to_string_lossy(),
                &stale_dest.to_string_lossy(),
            ],
            None,
            &auth,
        )
        .expect("clone by branch name");
        assert_eq!(
            run_git(&["rev-parse", "HEAD"], Some(&stale_dest), &auth)
                .expect("stale HEAD")
                .trim(),
            seed_sha,
            "the bug: --branch main lands on the seed commit, not the synced tip"
        );

        // The fix: land on the resolved sha.
        let dest = temp.path().join("seat-agents");
        land_seat_agents_clone_on_sha(
            &cache,
            &dest,
            "main",
            &synced_sha,
            SEAT_ORIGIN_SUFFIX,
            &auth,
        )
        .expect("landing on the synced sha must succeed");
        assert_eq!(
            run_git(&["rev-parse", "HEAD"], Some(&dest), &auth)
                .expect("dest HEAD")
                .trim(),
            synced_sha,
            "the clone must land on the synced commit, not the seed"
        );
        assert_eq!(
            run_git(&["symbolic-ref", "--short", "HEAD"], Some(&dest), &auth)
                .expect("branch name")
                .trim(),
            "main",
            "the checkout must be a real branch named after the pinned ref, not detached"
        );
        assert!(
            dest.join("team.yml")
                .to_str()
                .map(|_| std::fs::read_to_string(dest.join("team.yml")).unwrap_or_default())
                .unwrap_or_default()
                .contains("agents_repo: read"),
            "the clone's working tree must hold the synced content, not the seed's"
        );

        // The step `cut_seat_agents_clone` runs right after landing: pointing
        // `origin` at the relay. Proven here against the same `run_git` call
        // it uses, so a break in that composition shows up beside the clone.
        let relay_like = "http://127.0.0.1:9/git/deadbeef/seat-slug-beekeeper-agents";
        run_git(
            &["remote", "set-url", "origin", "--", relay_like],
            Some(&dest),
            &auth,
        )
        .expect("point origin at the relay");
        assert_eq!(
            run_git(&["remote", "get-url", "origin"], Some(&dest), &auth)
                .expect("origin url")
                .trim(),
            relay_like
        );
    }

    /// A seat re-staged after the cache advanced again must not be left on
    /// its first stage's commit: [`land_seat_agents_clone_on_sha`]'s reuse
    /// branch fetches the new sha straight from the cache path, not through
    /// `origin` (which, on a real clone, already points at the relay by the
    /// time a reuse happens).
    #[test]
    fn the_reuse_path_advances_an_existing_clone() {
        let temp = tempfile::tempdir().expect("temp");
        let auth =
            crate::commands::project_git_exec::build_test_git_auth_config().expect("test auth");
        let (cache, seed_sha, synced_sha) = build_lagging_cache(&temp, "main", &auth);

        let dest = temp.path().join("seat-agents");
        land_seat_agents_clone_on_sha(&cache, &dest, "main", &seed_sha, SEAT_ORIGIN_SUFFIX, &auth)
            .expect("first stage lands on the seed commit");
        assert_eq!(
            run_git(&["rev-parse", "HEAD"], Some(&dest), &auth)
                .expect("HEAD after first stage")
                .trim(),
            seed_sha
        );

        // As `cut_seat_agents_clone` does after a real clone: point `origin`
        // at the relay, so the reuse fetch below must not depend on it.
        run_git(
            &[
                "remote",
                "set-url",
                "origin",
                "--",
                "http://127.0.0.1:9/git/deadbeef/seat-slug-beekeeper-agents",
            ],
            Some(&dest),
            &auth,
        )
        .expect("point origin at the relay");

        land_seat_agents_clone_on_sha(
            &cache,
            &dest,
            "main",
            &synced_sha,
            SEAT_ORIGIN_SUFFIX,
            &auth,
        )
        .expect("the reuse path must advance the clone");
        assert_eq!(
            run_git(&["rev-parse", "HEAD"], Some(&dest), &auth)
                .expect("HEAD after reuse")
                .trim(),
            synced_sha,
            "a re-staged seat must land on the newly staged commit, not its first one"
        );
    }

    /// A sha the cache never held — never fetched, never synced, or simply
    /// wrong — must refuse by name rather than silently landing somewhere
    /// else or hanging on a network round trip that never happens locally.
    #[test]
    fn a_sha_absent_from_the_cache_is_refused_by_name() {
        let temp = tempfile::tempdir().expect("temp");
        let auth =
            crate::commands::project_git_exec::build_test_git_auth_config().expect("test auth");
        let (cache, _seed_sha, _synced_sha) = build_lagging_cache(&temp, "main", &auth);

        let missing_sha = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        let dest = temp.path().join("seat-agents");
        let error = land_seat_agents_clone_on_sha(
            &cache,
            &dest,
            "main",
            missing_sha,
            SEAT_ORIGIN_SUFFIX,
            &auth,
        )
        .expect_err("a sha the cache never held must be refused");
        assert!(
            error.contains(missing_sha),
            "the refusal must name the missing sha, got: {error}"
        );
    }

    /// Ledger 187's second weakness: a re-stage must **advance** the clone
    /// generation 1 left beside the worktree, not try to cut a second one.
    ///
    /// Proven by evidence the reuse path cannot fake: an untracked file and a
    /// second branch written into the first clone are still there after the
    /// second stage. A fresh clone would have neither (and, in fact, git
    /// refuses to clone into a non-empty directory at all, which is how this
    /// showed up as a refusal rather than as a duplicate).
    #[test]
    fn an_existing_clone_is_reused_rather_than_recut() {
        let temp = tempfile::tempdir().expect("temp");
        let auth =
            crate::commands::project_git_exec::build_test_git_auth_config().expect("test auth");
        let (cache, seed_sha, synced_sha) = build_lagging_cache(&temp, "main", &auth);

        let dest = temp.path().join("seat-agents");
        land_seat_agents_clone_on_sha(&cache, &dest, "main", &seed_sha, SEAT_ORIGIN_SUFFIX, &auth)
            .expect("first stage cuts the clone");
        std::fs::write(dest.join("seat-notes.md"), "generation 1 was here\n").expect("seat note");
        run_git(&["branch", "generation-1"], Some(&dest), &auth).expect("mark generation 1");
        run_git(
            &[
                "remote",
                "set-url",
                "origin",
                "--",
                "http://127.0.0.1:9/git/deadbeef/seat-slug-beekeeper-agents",
            ],
            Some(&dest),
            &auth,
        )
        .expect("point origin at the relay");

        land_seat_agents_clone_on_sha(
            &cache,
            &dest,
            "main",
            &synced_sha,
            SEAT_ORIGIN_SUFFIX,
            &auth,
        )
        .expect("a re-stage must reuse the clone it finds");
        assert_eq!(
            run_git(&["rev-parse", "HEAD"], Some(&dest), &auth)
                .expect("HEAD after re-stage")
                .trim(),
            synced_sha,
            "the reused clone must land on the newly staged commit"
        );
        assert!(
            dest.join("seat-notes.md").is_file(),
            "a reused clone keeps what generation 1 left in it; this one was recut"
        );
        assert!(
            run_git(
                &["rev-parse", "--verify", "generation-1"],
                Some(&dest),
                &auth
            )
            .is_ok(),
            "a reused clone keeps generation 1's refs; this one was recut"
        );
    }

    /// A directory that merely carries the clone's name is refused by name.
    ///
    /// Two shapes, both of which a person can create by hand beside a seat's
    /// worktree: a plain folder, and a real git repository of something else.
    /// The second is the dangerous one — a fetch into it would succeed, and
    /// the seat would be handed an unrelated history under the project's
    /// name — so the refusal rests on `origin`, not on "is it a repo".
    #[test]
    fn a_foreign_directory_beside_the_worktree_is_refused_by_name() {
        let temp = tempfile::tempdir().expect("temp");
        let auth =
            crate::commands::project_git_exec::build_test_git_auth_config().expect("test auth");
        let (cache, _seed_sha, synced_sha) = build_lagging_cache(&temp, "main", &auth);

        let plain = temp.path().join("plain-folder-agents");
        std::fs::create_dir(&plain).expect("plain dir");
        std::fs::write(plain.join("notes.txt"), "mine\n").expect("note");
        let error = land_seat_agents_clone_on_sha(
            &cache,
            &plain,
            "main",
            &synced_sha,
            SEAT_ORIGIN_SUFFIX,
            &auth,
        )
        .expect_err("a plain folder must be refused, not cloned into");
        assert!(
            error.contains(&plain.display().to_string())
                && error.contains("not a git clone of the project's agents repository"),
            "the refusal must name the directory: {error}"
        );

        let foreign = temp.path().join("foreign-agents");
        std::fs::create_dir(&foreign).expect("foreign dir");
        run_git(
            &["init", "--quiet", "--initial-branch", "main"],
            Some(&foreign),
            &auth,
        )
        .expect("git init foreign");
        run_git(
            &[
                "remote",
                "add",
                "origin",
                "--",
                "http://127.0.0.1:9/git/deadbeef/somebody-elses-repo",
            ],
            Some(&foreign),
            &auth,
        )
        .expect("foreign origin");
        let error = land_seat_agents_clone_on_sha(
            &cache,
            &foreign,
            "main",
            &synced_sha,
            SEAT_ORIGIN_SUFFIX,
            &auth,
        )
        .expect_err("a git repository of something else must be refused, not fetched into");
        assert!(
            error.contains("somebody-elses-repo") && error.contains(SEAT_ORIGIN_SUFFIX),
            "the refusal must name what it found and what it expected: {error}"
        );
        // `rev-parse --verify` takes a well-formed 40-hex at face value, so
        // the object itself is what gets asked about.
        assert!(
            run_git(&["cat-file", "-e", &synced_sha], Some(&foreign), &auth).is_err(),
            "the refused directory must not have been fetched into"
        );
    }

    /// One recorded worktree, shaped as the create path files it.
    fn seat_worktree_record(
        path: &str,
        session_id: Option<&str>,
    ) -> crate::coding_sessions::workdir_store::CodingSessionSeatWorktree {
        crate::coding_sessions::workdir_store::CodingSessionSeatWorktree {
            path: PathBuf::from(path),
            branch: "coding-session-lead".into(),
            repo_root: PathBuf::from("/Users/someone/Projects/pivot-test"),
            created_at: "2026-09-20T13:00:00Z".into(),
            session_id: session_id.map(str::to_owned),
            agents_clone: None,
        }
    }

    fn store_with(
        records: &[(&str, &str, Option<&str>)],
    ) -> crate::coding_sessions::workdir_store::CodingSessionWorkdirStore {
        let mut store = crate::coding_sessions::workdir_store::CodingSessionWorkdirStore::default();
        for (key, path, session_id) in records {
            store
                .worktrees
                .insert((*key).to_string(), seat_worktree_record(path, *session_id));
        }
        store
    }

    /// Ledger 187: the reconnect names the execution, not the directory, and the
    /// host answers from the record it wrote when it cut the tree.
    #[test]
    fn an_absent_worktree_is_resolved_from_this_hosts_own_record() {
        let store = store_with(&[
            (
                "44220:aa:one/lead",
                "/Users/brian/Projects/pivot-test-wt-build-kettle-cli-lead",
                Some("session-74495ca8"),
            ),
            (
                "44220:aa:one/builder",
                "/Users/brian/Projects/pivot-test-wt-build-kettle-cli-builder",
                Some("session-6c628bef"),
            ),
            (
                "44220:aa:old/lead",
                "/Users/brian/Projects/older-tree",
                None,
            ),
        ]);
        assert_eq!(
            seat_worktree_from_record(Some("session-74495ca8"), &store).expect("resolved"),
            PathBuf::from("/Users/brian/Projects/pivot-test-wt-build-kettle-cli-lead")
        );
    }

    /// The same record filed under two keys (a seat relabelled between
    /// generations) is one directory, not an ambiguity.
    #[test]
    fn two_records_naming_one_directory_resolve_to_it() {
        let store = store_with(&[
            (
                "44220:aa:one/lead",
                "/Users/brian/Projects/tree",
                Some("s1"),
            ),
            (
                "44220:aa:one/Levain",
                "/Users/brian/Projects/tree",
                Some("s1"),
            ),
        ]);
        assert_eq!(
            seat_worktree_from_record(Some("s1"), &store).expect("resolved"),
            PathBuf::from("/Users/brian/Projects/tree")
        );
    }

    /// Every failure names what was looked for. A guess here would put a clone —
    /// and, on disposal, a removal — beside a directory nobody asked for.
    #[test]
    fn an_unresolvable_worktree_is_refused_with_what_was_looked_for() {
        let store = store_with(&[(
            "44220:aa:one/lead",
            "/Users/brian/Projects/tree",
            Some("s1"),
        )]);

        let no_id = seat_worktree_from_record(None, &store).expect_err("no id, no answer");
        assert!(
            no_id.contains("no provider session id"),
            "unexpected refusal: {no_id}"
        );

        let unknown = seat_worktree_from_record(Some("s-missing"), &store)
            .expect_err("an unrecorded session must refuse");
        assert!(
            unknown.contains("s-missing") && unknown.contains("records none"),
            "the refusal must name the session it looked for: {unknown}"
        );

        let ambiguous = store_with(&[
            ("44220:aa:one/a", "/Users/brian/Projects/tree-a", Some("s1")),
            ("44220:aa:one/b", "/Users/brian/Projects/tree-b", Some("s1")),
        ]);
        let error = seat_worktree_from_record(Some("s1"), &ambiguous)
            .expect_err("two directories for one session must refuse");
        assert!(
            error.contains("tree-a") && error.contains("tree-b"),
            "the refusal must name both candidates: {error}"
        );
    }

    #[test]
    fn the_clone_sits_beside_the_worktree() {
        assert_eq!(
            seat_agents_clone_path(Path::new("/src/proj.worktrees/lane")),
            Some(PathBuf::from("/src/proj.worktrees/lane-agents"))
        );
        assert_eq!(seat_agents_clone_path(Path::new("/")), None);
    }
}
