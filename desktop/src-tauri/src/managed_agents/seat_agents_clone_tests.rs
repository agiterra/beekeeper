//! Tests for the seat's agents-repository clone and the records that place it.
//!
//! Split out of `seat_agents_clone.rs` to keep both files under the
//! repository's 1000-line ceiling; `use super::*` keeps every claim against
//! the same module it was written for. Everything here runs against throwaway
//! git repositories and in-memory records — never the real packs cache, the
//! real workdir store, or a running provider.

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
    let auth = crate::commands::project_git_exec::build_test_git_auth_config().expect("test auth");
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
    let auth = crate::commands::project_git_exec::build_test_git_auth_config().expect("test auth");
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
    let auth = crate::commands::project_git_exec::build_test_git_auth_config().expect("test auth");
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
    let auth = crate::commands::project_git_exec::build_test_git_auth_config().expect("test auth");
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
    let auth = crate::commands::project_git_exec::build_test_git_auth_config().expect("test auth");
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
        commit_identity: None,
        actor_pubkey: None,
        seeding: None,
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

/// A real git work tree in a throwaway directory, since rungs 2–4 admit a
/// record only if what it names is still one.
fn throwaway_worktree(temp: &tempfile::TempDir, name: &str) -> PathBuf {
    let auth = crate::commands::project_git_exec::build_test_git_auth_config().expect("test auth");
    let path = temp.path().join(name);
    std::fs::create_dir_all(&path).expect("tree dir");
    run_git(
        &["init", "--quiet", "--initial-branch", "main"],
        Some(&path),
        &auth,
    )
    .expect("git init");
    path
}

/// Ledger 187's rung: the reconnect names the execution, not the
/// directory, and the desktop's own worktree record answers.
#[test]
fn a_recorded_worktree_answers_for_its_session() {
    let temp = tempfile::tempdir().expect("temp");
    let lead = throwaway_worktree(&temp, "kettle-lead");
    let other = throwaway_worktree(&temp, "kettle-builder");
    let store = store_with(&[
        (
            "44220:aa:one/lead",
            lead.to_str().expect("utf8"),
            Some("session-74495ca8"),
        ),
        (
            "44220:aa:one/builder",
            other.to_str().expect("utf8"),
            Some("session-6c628bef"),
        ),
        ("44220:aa:old/lead", "/Users/brian/Projects/older", None),
    ]);
    assert_eq!(
        seat_worktree_from_records("session-74495ca8", &store, &ProviderSessionFacts::default())
            .expect("resolved"),
        lead
    );
}

/// The same directory filed under two keys is one answer, not a
/// contradiction; two different directories for one session refuse rather
/// than falling through to a less exact record.
#[test]
fn one_session_with_two_directories_refuses_instead_of_guessing() {
    let temp = tempfile::tempdir().expect("temp");
    let tree = throwaway_worktree(&temp, "one-tree");
    let twice = store_with(&[
        (
            "44220:aa:one/lead",
            tree.to_str().expect("utf8"),
            Some("s1"),
        ),
        (
            "44220:aa:one/Levain",
            tree.to_str().expect("utf8"),
            Some("s1"),
        ),
    ]);
    assert_eq!(
        seat_worktree_from_records("s1", &twice, &ProviderSessionFacts::default())
            .expect("one directory, twice"),
        tree
    );

    let ambiguous = store_with(&[
        ("44220:aa:one/a", "/Users/brian/Projects/tree-a", Some("s1")),
        ("44220:aa:one/b", "/Users/brian/Projects/tree-b", Some("s1")),
    ]);
    let error = seat_worktree_from_records(
        "s1",
        &ambiguous,
        // A provider record that *could* answer, to prove the
        // contradiction is not papered over by the next rung.
        &ProviderSessionFacts {
            cwd: Some(PathBuf::from("/Users/brian/Projects/tree-a")),
            command_id: None,
        },
    )
    .expect_err("two directories for one session must refuse");
    assert!(
        error.contains("tree-a") && error.contains("tree-b"),
        "the refusal must name both candidates: {error}"
    );
}

/// Ledger 188, the live failure: the kettle lead's tree was cut by the
/// **launch** path, which wrote no `worktrees` record at all — only the
/// create hint keyed by its `commandId`. The provider's own snapshot is
/// what still knows where that execution runs, so it answers.
#[test]
fn the_providers_own_record_answers_when_the_desktop_recorded_nothing() {
    let temp = tempfile::tempdir().expect("temp");
    let lead = throwaway_worktree(&temp, "pivot-test-wt-build-kettle-cli-lead");
    let store = crate::coding_sessions::workdir_store::CodingSessionWorkdirStore::default();
    let facts = ProviderSessionFacts {
        cwd: Some(lead.clone()),
        command_id: Some("csl-86e283cf-dd2c-4d6c-a0ee-b23cd1f9a8c1".into()),
    };
    assert_eq!(
        seat_worktree_from_records("74495ca8-2aeb-4f25-9f40-3124be1c476f", &store, &facts)
            .expect("the provider's record answers"),
        lead
    );
}

/// Rung 4: a provider record with no `cwd` still names the create
/// command, and the desktop's one-shot hint is filed under exactly that.
#[test]
fn the_create_hint_answers_by_the_command_the_provider_names() {
    let temp = tempfile::tempdir().expect("temp");
    let lead = throwaway_worktree(&temp, "hinted-lead");
    let mut store = crate::coding_sessions::workdir_store::CodingSessionWorkdirStore::default();
    store.pending.insert("csl-86e283cf".into(), lead.clone());
    let facts = ProviderSessionFacts {
        cwd: None,
        command_id: Some("csl-86e283cf".into()),
    };
    assert_eq!(
        seat_worktree_from_records("74495ca8", &store, &facts).expect("the hint answers"),
        lead
    );
}

/// With nothing anywhere, the refusal names all three places it looked —
/// which is the difference between a fact and a shrug.
#[test]
fn a_session_no_record_knows_is_refused_naming_every_place_looked() {
    let store = crate::coding_sessions::workdir_store::CodingSessionWorkdirStore::default();
    let error = seat_worktree_from_records(
        "74495ca8",
        &store,
        &ProviderSessionFacts {
            cwd: None,
            command_id: Some("csl-86e283cf".into()),
        },
    )
    .expect_err("nothing recorded must refuse");
    for expected in [
        "74495ca8",
        "coding-session worktree records",
        "provider's record",
        "create hint",
        "csl-86e283cf",
    ] {
        assert!(
            error.contains(expected),
            "{expected:?} missing from: {error}"
        );
    }

    let unknown = seat_worktree_from_records("74495ca8", &store, &ProviderSessionFacts::default())
        .expect_err("an execution the provider never ran must refuse");
    assert!(
        unknown.contains("names no create command"),
        "the refusal must say the provider knows no command either: {unknown}"
    );
}

/// A record is a record, not a promise: the directory it names may have
/// been moved, deleted, or never have been a work tree. Each case is
/// refused naming the record that answered and the path it gave.
#[test]
fn a_record_naming_something_that_is_not_a_work_tree_is_refused_by_name() {
    let temp = tempfile::tempdir().expect("temp");
    let gone = temp.path().join("deleted-lead");
    let error = seat_worktree_from_records(
        "s1",
        &crate::coding_sessions::workdir_store::CodingSessionWorkdirStore::default(),
        &ProviderSessionFacts {
            cwd: Some(gone.clone()),
            command_id: None,
        },
    )
    .expect_err("a vanished directory must refuse");
    assert!(
        error.contains(&gone.display().to_string()) && error.contains("no such directory"),
        "unexpected refusal: {error}"
    );

    let plain = temp.path().join("not-a-repo");
    std::fs::create_dir(&plain).expect("plain dir");
    let error = seat_worktree_from_records(
        "s1",
        &crate::coding_sessions::workdir_store::CodingSessionWorkdirStore::default(),
        &ProviderSessionFacts {
            cwd: Some(plain.clone()),
            command_id: None,
        },
    )
    .expect_err("a directory that is not a work tree must refuse");
    assert!(
        error.contains(&plain.display().to_string()) && error.contains("not a git work tree"),
        "unexpected refusal: {error}"
    );
}

/// The two keys this reader pulls out of the provider's snapshot, read
/// off a real record — the exact shape observed on Brian's machine for
/// the kettle lead.
#[test]
fn the_providers_snapshot_yields_the_cwd_and_the_create_command() {
    let snapshot = serde_json::json!({
        "version": 1,
        "sessions": {
            "74495ca8-2aeb-4f25-9f40-3124be1c476f": {
                "sessionId": "74495ca8-2aeb-4f25-9f40-3124be1c476f",
                "commandId": "csl-86e283cf-dd2c-4d6c-a0ee-b23cd1f9a8c1",
                "cwd": "/Users/brian/Projects/pivot-test-wt-build-kettle-cli-lead"
            }
        }
    });
    assert_eq!(
        provider_session_facts(&snapshot, "74495ca8-2aeb-4f25-9f40-3124be1c476f"),
        ProviderSessionFacts {
            cwd: Some(PathBuf::from(
                "/Users/brian/Projects/pivot-test-wt-build-kettle-cli-lead"
            )),
            command_id: Some("csl-86e283cf-dd2c-4d6c-a0ee-b23cd1f9a8c1".into()),
        }
    );
    // An execution this provider never ran, and a snapshot with no
    // sessions at all, are both "nothing recorded" rather than errors.
    assert_eq!(
        provider_session_facts(&snapshot, "no-such-session"),
        ProviderSessionFacts::default()
    );
    assert_eq!(
        provider_session_facts(&serde_json::json!({"version": 1}), "74495ca8"),
        ProviderSessionFacts::default()
    );
}

/// The coupling, pinned: this module reads two keys out of a file another
/// crate writes, so the keys are asserted against that crate's own type.
///
/// A rename in `beekeeper_session_provider::state::SessionRecord` fails here —
/// at the deserialize, or at the key assertion — instead of silently
/// turning rungs 3 and 4 into a permanent "nothing recorded".
#[test]
fn the_provider_session_keys_match_the_providers_own_type() {
    let fixture = serde_json::json!({
        "sessionId": "74495ca8-2aeb-4f25-9f40-3124be1c476f",
        "generation": 1,
        "channelId": "85b8db75-4b60-4741-bcfa-7f75cc238ff0",
        "commandId": "csl-86e283cf-dd2c-4d6c-a0ee-b23cd1f9a8c1",
        "cwd": "/Users/brian/Projects/pivot-test-wt-build-kettle-cli-lead",
        "projectRef": null,
        "repoRef": null,
        "createdBy": "3d3b7169a13a8311b480bdfce85b4a0c7ff9b185832cbc6e547db7bbcf96c05e",
        "authoritySeq": 7,
        "createdAtMs": 1789903633887u64,
        "nextSeq": 291,
        "closed": false
    });
    let record: beekeeper_session_provider_pkg::state::SessionRecord =
        serde_json::from_value(fixture.clone())
            .expect("the provider's own type must accept this shape");
    let round_tripped = serde_json::to_value(&record).expect("serialize");
    assert_eq!(
        round_tripped.get("cwd").and_then(|v| v.as_str()),
        Some("/Users/brian/Projects/pivot-test-wt-build-kettle-cli-lead"),
        "the provider writes the working directory under some other key now"
    );
    assert_eq!(
        round_tripped.get("commandId").and_then(|v| v.as_str()),
        Some("csl-86e283cf-dd2c-4d6c-a0ee-b23cd1f9a8c1"),
        "the provider writes the create command under some other key now"
    );
    // And the reader agrees with the type, on the type's own output.
    let snapshot = serde_json::json!({ "sessions": { "s": round_tripped } });
    let facts = provider_session_facts(&snapshot, "s");
    assert_eq!(facts.cwd, Some(record.cwd.clone()));
    assert_eq!(
        facts.command_id.as_deref(),
        Some(record.command_id.as_str())
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

/// A write seat can edit its own clone's configuration. Re-staging the clone
/// materializes it inside the clone's boundary: a filter the seat planted
/// there runs with the clone's rights only — it can write the clone, never
/// outside it. The planted filter is proved live, and able to write outside,
/// by an ordinary host checkout first.
// The boundary backend is macOS's; elsewhere the plan is disclosed as
// unenforced.
#[cfg(target_os = "macos")]
#[test]
fn restaging_runs_a_planted_filter_only_inside_the_clones_boundary() {
    let temp = tempfile::tempdir().expect("temp");
    let auth = crate::commands::project_git_exec::build_test_git_auth_config().expect("test auth");
    let (cache, seed_sha, synced_sha) = build_lagging_cache(&temp, "main", &auth);
    let dest = temp.path().join("seat-agents");
    land_seat_agents_clone_on_sha(&cache, &dest, "main", &seed_sha, SEAT_ORIGIN_SUFFIX, &auth)
        .expect("first stage");
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
    .expect("origin at the relay");

    // What the seat plants: attributes and a driver in its clone's `.git`.
    let canary = temp.path().join("FILTER_RAN");
    let inside = dest.join(".git/PLANTED_FILTER_RAN");
    std::fs::write(dest.join(".git/info/attributes"), "* filter=planted\n").expect("attributes");
    let smudge = format!(
        "touch '{}' 2>/dev/null; touch '{}' 2>/dev/null; cat",
        inside.display(),
        canary.display()
    );
    run_git(
        &["config", "filter.planted.smudge", &smudge],
        Some(&dest),
        &auth,
    )
    .expect("smudge");
    run_git(
        &["config", "filter.planted.clean", "cat"],
        Some(&dest),
        &auth,
    )
    .expect("clean");
    run_git(
        &["config", "filter.planted.required", "true"],
        Some(&dest),
        &auth,
    )
    .expect("required");

    // Positive control: an ordinary checkout in the clone runs it.
    run_git(
        &[
            "fetch",
            "--quiet",
            "--",
            &cache.to_string_lossy(),
            &synced_sha,
        ],
        Some(&dest),
        &auth,
    )
    .expect("control fetch");
    run_git(
        &["checkout", "--quiet", "--detach", &synced_sha],
        Some(&dest),
        &auth,
    )
    .expect("control checkout");
    assert!(
        canary.exists(),
        "the planted filter must be live, or this test proves nothing"
    );
    run_git(&["checkout", "--quiet", "main"], Some(&dest), &auth).expect("back to the seed");
    std::fs::remove_file(&canary).expect("reset canary");
    std::fs::remove_file(&inside).expect("reset marker");

    land_seat_agents_clone_on_sha(
        &cache,
        &dest,
        "main",
        &synced_sha,
        SEAT_ORIGIN_SUFFIX,
        &auth,
    )
    .expect("re-stage");
    assert!(
        inside.exists(),
        "the re-stage materialized without the clone's own filter"
    );
    assert!(
        !canary.exists(),
        "the seat's planted filter wrote outside its clone during the host's re-stage"
    );
    assert_eq!(
        run_git(&["rev-parse", "HEAD"], Some(&dest), &auth)
            .expect("HEAD")
            .trim(),
        synced_sha
    );
    assert!(
        std::fs::read_to_string(dest.join("team.yml"))
            .expect("team.yml")
            .contains("agents_repo: read"),
        "the re-stage still materializes the staged commit"
    );
}
