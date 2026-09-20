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

use std::path::{Path, PathBuf};

use tauri::AppHandle;

use crate::app_state::AppState;
use crate::commands::project_git_exec::{
    build_local_clone_git_auth_config, run_git, GitAuthConfig,
};
use crate::managed_agents::packs_cache;

/// The suffix the clone's directory carries beside the seat's worktree.
///
/// Defined in `buzz-core` and re-exported here: `bee`, running *inside* a
/// seat, composes the same sibling to find the project's model registry
/// (`buzz_core::model_registry_source`, ledger 178(a)), and a host that cut
/// `-agents` while the CLI looked for something else would leave a seat
/// unable to find the repository it was given.
pub use buzz_core_pkg::model_registry_source::{seat_agents_clone_path, SEAT_AGENTS_CLONE_SUFFIX};

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
    land_seat_agents_clone_on_sha(&cache, &dest, branch, sha, &auth)?;
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
fn land_seat_agents_clone_on_sha(
    cache: &Path,
    dest: &Path,
    branch: &str,
    sha: &str,
    auth: &GitAuthConfig,
) -> Result<(), String> {
    let cache_str = cache.to_string_lossy().into_owned();
    let dest_str = dest.to_string_lossy().into_owned();
    if !dest.join(".git").is_dir() {
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
        land_seat_agents_clone_on_sha(&cache, &dest, "main", &synced_sha, &auth)
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
        land_seat_agents_clone_on_sha(&cache, &dest, "main", &seed_sha, &auth)
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

        land_seat_agents_clone_on_sha(&cache, &dest, "main", &synced_sha, &auth)
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
        let error = land_seat_agents_clone_on_sha(&cache, &dest, "main", missing_sha, &auth)
            .expect_err("a sha the cache never held must be refused");
        assert!(
            error.contains(missing_sha),
            "the refusal must name the missing sha, got: {error}"
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
