//! Project execution scope, exercised through the provider's own launch path.
//!
//! Every test here that asserts isolation starts a real adapter process via
//! [`crate::session::SessionManager`] with a [`ExecutionPlan`] that
//! [`prepare`] produced, and reads back what that process — and the shells
//! and interpreters it started — could actually reach. Nothing is asserted
//! from the policy text alone.

use super::*;
use crate::session::testing::fake_agent;
use crate::session::{CreateRequest, SessionEvent, SessionManager};
use buzz_core::coding_session_command::CodingSessionTarget;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

const PROVIDER_PUBKEY: &str = "abababababababababababababababababababababababababababababababab";

/// Disposable fixture: an app-data layout holding another project's cached
/// plan, project A as a git repository with a linked seat worktree, a sibling
/// project B, and a project C under a different root.
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    state_dir: PathBuf,
    repo_a: PathBuf,
    seat_a: PathBuf,
    b_plan: PathBuf,
    c_plan: PathBuf,
    cache_plan: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    let mut command = std::process::Command::new("git");
    command.args(args).current_dir(dir);
    for name in crate::git_probe::GIT_REPO_SELECTION_VARS {
        command.env_remove(name);
    }
    command
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_TEMPLATE_DIR");
    let status = command
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?} failed");
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical");
    let app = root.join("app");
    let state_dir = app.join("session-provider").join(PROVIDER_PUBKEY);
    std::fs::create_dir_all(&state_dir).expect("state dir");
    // The regression run10 hit: another project's plan in the host cache.
    let cache_plan = app.join("packs/other-project-agents/plans/kettle.md");
    std::fs::create_dir_all(cache_plan.parent().expect("parent")).expect("cache");
    std::fs::write(&cache_plan, "CACHED_FOREIGN_PLAN\n").expect("cache plan");

    let repo_a = root.join("repos/a");
    std::fs::create_dir_all(&repo_a).expect("a");
    git(&repo_a, &["init", "-q", "-b", "main"]);
    std::fs::write(repo_a.join("README.md"), "A_OWN\n").expect("readme");
    git(&repo_a, &["add", "."]);
    git(&repo_a, &["commit", "-q", "-m", "init"]);
    let seat_a = root.join("repos/a-wt-seat");
    git(
        &repo_a,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "seat-branch",
            seat_a.to_str().expect("utf8"),
            "main",
        ],
    );

    std::fs::create_dir_all(seat_a.join(".probe")).expect("probe dir");
    std::fs::write(seat_a.join(".probe/sentinels.py"), SENTINEL_PRINTER).expect("printer");

    let b_plan = root.join("repos/b/plans/plan.md");
    std::fs::create_dir_all(b_plan.parent().expect("parent")).expect("b");
    std::fs::write(&b_plan, "B_PLAN_CANARY\n").expect("b plan");
    let c_plan = root.join("elsewhere/c/plan.md");
    std::fs::create_dir_all(c_plan.parent().expect("parent")).expect("c");
    std::fs::write(&c_plan, "C_PLAN_CANARY\n").expect("c plan");
    Fixture {
        _dir: dir,
        root,
        state_dir,
        repo_a,
        seat_a,
        b_plan,
        c_plan,
        cache_plan,
    }
}

fn inputs<'a>(
    fx: &'a Fixture,
    session_id: &'a str,
    cwd: &'a Path,
    agent_env: &'a [(String, String)],
    identity_env: &'a [(String, String)],
) -> ScopeInputs<'a> {
    let mut inputs = ScopeInputs::new(ScopePurpose::Session, &fx.state_dir, session_id, cwd);
    inputs.project_ref = Some("30621:aa:project-a");
    // The seat worktree is a linked worktree of the recorded checkout: the
    // association is proved from the repository, not asserted.
    inputs.project_checkout = Some(&fx.repo_a);
    inputs.actor = Some("cd");
    inputs.driver = "claude-agent-acp";
    inputs.runtime = RuntimeProfile::TestDouble;
    inputs.agent_command = "bash";
    inputs.agent_env = agent_env;
    inputs.identity_env = identity_env;
    inputs
}

/// An ACP agent that, on `session/new`, probes what it can reach from
/// itself, a nested shell and a python child, writes one `name=OK|DENIED`
/// line per probe to `report` inside its working tree, then waits for the
/// test to name a path created after it started and probes that too.
fn probe_agent(fx: &Fixture, mutating: bool) -> String {
    let b = fx.b_plan.display();
    let c = fx.c_plan.display();
    let cache = fx.cache_plan.display();
    let seat = fx.seat_a.display();
    let fixture_root = fx.root.display();
    format!(
        r#"
# The legacy ACP spawn does not set its process cwd. Never run a mutating
# probe until both the checkout and Git administration are inside this fixture.
cd -- "{seat}" || exit 90
[ "$(pwd -P)" = "{seat}" ] || exit 91
COMMON=$(git rev-parse --path-format=absolute --git-common-dir) || exit 92
case "$COMMON" in "{fixture_root}/"*) ;; *) exit 93 ;; esac
[ "$(git rev-parse --show-toplevel)" = "{seat}" ] || exit 94
REPORT="$PWD/report"
probe() {{ if eval "$2" >/dev/null 2>&1; then echo "$1=OK" >> "$REPORT"; else echo "$1=DENIED" >> "$REPORT"; fi; }}
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":2}}}}\n' "$id" ;;
    *'"method":"session/new"'*)
      probe own_read 'cat README.md'
      probe own_write 'echo x > own-new.txt'
      probe sibling_abs 'cat "{b}"'
      probe sibling_rel 'cat ../b/plans/plan.md'
      ln -s "{b}" link-to-b 2>/dev/null
      probe sibling_symlink 'cat link-to-b'
      probe sibling_list 'ls ../b'
      probe other_root 'cat "{c}"'
      probe host_cache 'cat "{cache}"'
      probe python_child '/usr/bin/python3 -c "open(\"{b}\").read()"'
      probe shell_child '/bin/sh -c "cat \"{b}\""'
      if [ "{mutating}" = true ]; then
        probe sibling_write 'echo pwn >> "{b}"'
        probe git_commit 'git -c user.name=t -c user.email=t@x commit -q --allow-empty -m probe'
        probe git_other_branch 'git update-ref refs/heads/main HEAD'
        probe git_new_branch 'git branch probe-new'
        probe git_shared_config 'git config --local probe.x 1'
        probe git_hook 'echo x > "$COMMON/hooks/post-commit"'
      fi
      /bin/sh -c '/usr/bin/python3 .probe/sentinels.py' > "$PWD/child-env" 2>/dev/null
      echo done > "$PWD/phase1"
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"sessionId":"acp-probe"}}}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      LATE=$(cat "$PWD/late-path" 2>/dev/null)
      probe late_sibling "cat \"$LATE\""
      probe own_after 'cat README.md'
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$id" ;;
  esac
done
"#
    )
}

/// Python that prints only the named sentinels (presence for secrets), never
/// the ambient environment.
const SENTINEL_PRINTER: &str = r#"import os
for k in ['SEAT_SENTINEL','RUNTIME_SENTINEL','PROJECT_A_ONLY','TMPDIR','GIT_CONFIG_GLOBAL','CARGO_HOME']:
    print(k+'='+os.environ.get(k,'<absent>'))
for k in ['BUZZ_PRIVATE_KEY','CARGO_MANIFEST_DIR','CARGO_PKG_NAME']:
    print(k+'='+('<present>' if k in os.environ else '<absent>'))"#;

fn create_request(cwd: &Path, agent: String, execution: ExecutionPlan) -> CreateRequest {
    CreateRequest {
        media: None,
        seat: None,
        post_fence_env: Vec::new(),
        seat_skills: None,
        target: CodingSessionTarget {
            driver: "claude-agent-acp".into(),
            instance_id: "instance-1".into(),
            session_id: "s1".into(),
            generation: 1,
        },
        channel_id: Uuid::nil(),
        cwd: cwd.to_path_buf(),
        title: None,
        model: None,
        resume_cursor: None,
        strict_native: false,
        rehydration_mcp: None,
        agent_command: "bash".into(),
        agent_args: vec![agent],
        agent_env: Vec::new(),
        idle_timeout: Duration::from_secs(10),
        answer_stall_timeout: None,
        emit_raw_sdk_frames: false,
        max_turn_duration: Duration::from_secs(20),
        idle_shutdown: Duration::from_secs(30),
        include_thoughts: false,
        execution,
    }
}

fn report(dir: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(dir.join("report"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

#[test]
fn a_project_root_the_boundary_cannot_hold_is_refused() {
    let fx = fixture();
    let home = home_dir().expect("home");
    let unbound = WorkspaceAssociation::Unbound;
    for (root, why) in [
        (PathBuf::from("/opt/homebrew/src/x"), "system root"),
        (home.clone(), "the home itself"),
        (fx.state_dir.clone(), "the app's own data"),
        (
            fx.root.join("app/packs/other-project-agents"),
            "the host cache",
        ),
    ] {
        let error =
            validate_workspace_placement(&root, unbound, &home, &fx.state_dir).expect_err(why);
        assert_eq!(error.code, EXECUTION_SCOPE_INVALID, "{why}");
    }
    assert!(validate_workspace_placement(&fx.seat_a, unbound, &home, &fx.state_dir).is_ok());
    // Host-owned workspaces inside the app's data: admitted only when a host
    // record binds them, never as a directory someone merely named.
    let bound = WorkspaceAssociation::HostBound;
    for root in [
        fx.root.join("app/project-team-setup/s1/draft"),
        fx.state_dir.join("actions/run/step/worktree"),
        fx.state_dir.join("agents-clones/s1"),
    ] {
        assert!(
            validate_workspace_placement(&root, bound, &home, &fx.state_dir).is_ok(),
            "{}",
            root.display()
        );
        assert!(
            validate_workspace_placement(&root, unbound, &home, &fx.state_dir).is_err(),
            "{}",
            root.display()
        );
    }
    assert!(
        validate_workspace_placement(&fx.root.join("app/packs/x"), bound, &home, &fx.state_dir)
            .is_err(),
        "a host record cannot make the host cache a workspace"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_project_launch_runs_only_in_a_workspace_the_host_ties_to_that_project() {
    let fx = fixture();
    // An ordinary directory elsewhere — a channel default, an unbound hint.
    let elsewhere = fx.root.join("elsewhere/plain");
    std::fs::create_dir_all(&elsewhere).expect("dir");
    let mut facts = inputs(&fx, "s1", &elsewhere, &[], &[]);
    let error = prepare(&facts).expect_err("not the project's workspace");
    assert_eq!(error.code, EXECUTION_SCOPE_INVALID);
    assert!(
        error.message.contains("no host record binds it"),
        "{}",
        error.message
    );
    // The same directory, bound to the project by a host record.
    facts.association = WorkspaceAssociation::HostBound;
    assert!(matches!(prepare(&facts), Ok(ExecutionPlan::Prepared(_))));
    // No project claimed: nothing to associate.
    let mut standalone = inputs(&fx, "s2", &elsewhere, &[], &[]);
    standalone.project_ref = None;
    assert!(matches!(
        prepare(&standalone),
        Ok(ExecutionPlan::Prepared(_))
    ));
    // The recorded checkout itself, and its linked worktree.
    for cwd in [&fx.repo_a, &fx.seat_a] {
        assert!(matches!(
            prepare(&inputs(&fx, "s3", cwd, &[], &[])),
            Ok(ExecutionPlan::Prepared(_))
        ));
    }
    // A worktree of another repository, claimed for this project.
    let other = fixture();
    let error =
        prepare(&inputs(&fx, "s4", &other.seat_a, &[], &[])).expect_err("foreign repository");
    assert_eq!(error.code, EXECUTION_SCOPE_INVALID);
}

#[test]
fn a_worktree_of_another_repository_or_with_borrowed_objects_is_refused() {
    let fx = fixture();
    let other = fx.root.join("repos/b");
    let error = crate::execution_scope_git::linked_worktree_admin(&fx.seat_a, Some(&other), None)
        .expect_err("foreign repo");
    assert_eq!(error.code, EXECUTION_SCOPE_INVALID);
    let grants =
        crate::execution_scope_git::linked_worktree_admin(&fx.seat_a, Some(&fx.repo_a), None)
            .expect("own repo")
            .expect("linked worktree");
    let common = fx.repo_a.join(".git").canonicalize().expect("common");
    assert!(grants.iter().any(|g| g.target
        == exec_boundary::GrantTarget::Tree(common.join("objects"))
        && g.access == Access::ReadWrite));
    assert!(grants.iter().any(|g| g.target
        == exec_boundary::GrantTarget::File(common.join("objects/info/alternates"))
        && g.access == Access::NoWrite));
    assert!(
        !grants
            .iter()
            .any(|g| g.target.path() == common.join("refs/heads/main")),
        "another branch must not be writable"
    );
    // A branch the host itself moves the tree to is granted beside its own.
    let grants = crate::execution_scope_git::linked_worktree_admin(
        &fx.seat_a,
        Some(&fx.repo_a),
        Some("wip/next"),
    )
    .expect("host branch")
    .expect("linked");
    assert!(grants
        .iter()
        .any(|g| g.target.path() == common.join("refs/heads/wip/next")));
    std::fs::create_dir_all(common.join("objects/info")).expect("info");
    std::fs::write(
        common.join("objects/info/alternates"),
        "/elsewhere/objects\n",
    )
    .expect("alt");
    let error =
        crate::execution_scope_git::linked_worktree_admin(&fx.seat_a, Some(&fx.repo_a), None)
            .expect_err("alternates");
    assert_eq!(error.code, EXECUTION_SCOPE_INVALID);
}

#[test]
fn the_scope_digest_binds_project_seat_and_tree_but_not_wake_data() {
    let fx = fixture();
    let base = inputs(&fx, "s1", &fx.seat_a, &[], &[]);
    let tree = fx.seat_a.clone();
    let digest = scope_digest(&base, &tree, "");
    let mut other_session = base.clone();
    other_session.session_id = "s2";
    assert_eq!(
        scope_digest(&other_session, &tree, ""),
        digest,
        "a new generation or session id is not a new scope"
    );
    let mut rebound = base.clone();
    rebound.project_ref = Some("30621:aa:project-b");
    assert_ne!(scope_digest(&rebound, &tree, ""), digest);
    let mut reseated = base.clone();
    reseated.actor = Some("ef");
    assert_ne!(scope_digest(&reseated, &tree, ""), digest);
    assert_ne!(
        scope_digest(&base, &tree, "/x|read-write"),
        digest,
        "a change of agents rights is a new scope"
    );
}

#[test]
fn runtimes_are_decided_by_driver_and_unverified_ones_are_refused() {
    assert_eq!(
        RuntimeProfile::for_driver("claude-agent-acp"),
        RuntimeProfile::Claude
    );
    assert_eq!(
        RuntimeProfile::for_driver("codex-acp"),
        RuntimeProfile::Codex
    );
    assert_eq!(
        RuntimeProfile::for_driver("goose-acp"),
        RuntimeProfile::Unsupported
    );
    let fx = fixture();
    let mut facts = inputs(&fx, "s1", &fx.seat_a, &[], &[]);
    facts.runtime = RuntimeProfile::Unsupported;
    if cfg!(target_os = "macos") {
        let error = prepare(&facts).expect_err("unsupported runtime");
        assert_eq!(error.code, EXECUTION_BOUNDARY_UNAVAILABLE);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn native_history_is_reattached_only_to_its_own_scope() {
    let fx = fixture();
    let cursor = "acp-1";
    let mut facts = inputs(&fx, "s1", &fx.seat_a, &[], &[]);
    facts.prior = Some((None, Some(cursor)));
    let ExecutionPlan::Prepared(legacy) = prepare(&facts).expect("prepared") else {
        panic!("macOS prepares a scope");
    };
    assert_eq!(legacy.native_refusal, Some(NATIVE_HISTORY_UNBOUND));
    let binding = legacy.binding.clone();
    facts.prior = Some((Some(&binding), Some(cursor)));
    let Ok(ExecutionPlan::Prepared(same)) = prepare(&facts) else {
        panic!("prepared twice");
    };
    assert_eq!(same.native_refusal, None);
    let mut foreign = binding.clone();
    foreign.scope_digest = "0".repeat(64);
    facts.prior = Some((Some(&foreign), Some(cursor)));
    let Ok(ExecutionPlan::Prepared(other)) = prepare(&facts) else {
        panic!("prepared thrice");
    };
    assert_eq!(other.native_refusal, Some(NATIVE_HISTORY_FOREIGN));
}

#[test]
fn each_session_and_scope_gets_its_own_verified_private_directory() {
    let fx = fixture();
    let a = claim_owner_dir(&fx.state_dir, "s1", &"a".repeat(64)).expect("a");
    let a_again = claim_owner_dir(&fx.state_dir, "s1", &"a".repeat(64)).expect("reuse");
    assert_eq!(
        a.state, a_again.state,
        "a resume of the same scope reuses its history"
    );
    let rebound = claim_owner_dir(&fx.state_dir, "s1", &"b".repeat(64)).expect("b");
    assert_ne!(
        a.state, rebound.state,
        "a changed scope never inherits the old history"
    );
    let other = claim_owner_dir(&fx.state_dir, "s2", &"a".repeat(64)).expect("s2");
    assert_ne!(a.state, other.state);
    assert_ne!(
        a.control, a.state,
        "host control records are not runtime state"
    );
    std::fs::write(a.control.join("owner.json"), "{\"sessionId\":\"sX\"}").expect("tamper");
    let error = claim_owner_dir(&fx.state_dir, "s1", &"a".repeat(64)).expect_err("tampered");
    assert_eq!(error.code, EXECUTION_BOUNDARY_UNAVAILABLE);
    assert!(claim_owner_dir(&fx.state_dir, "../escape", &"a".repeat(64)).is_err());
}

#[test]
fn a_changed_role_contract_is_a_new_scope() {
    let fx = fixture();
    let mut base = inputs(&fx, "s1", &fx.seat_a, &[], &[]);
    base.role_contract = Some("role=builder|persona=builder|pack=r@aaaa:p|compose=d1");
    let tree = fx.seat_a.clone();
    let digest = scope_digest(&base, &tree, "");
    let mut restaged = base.clone();
    restaged.role_contract = Some("role=builder|persona=builder|pack=r@bbbb:p|compose=d1");
    assert_ne!(scope_digest(&restaged, &tree, ""), digest);
}

#[test]
fn prepared_diagnostics_carry_no_values_or_paths() {
    let fx = fixture();
    let identity = vec![(
        "BUZZ_PRIVATE_KEY".to_owned(),
        "nsec-secret-value".to_owned(),
    )];
    let Ok(plan) = prepare(&inputs(&fx, "s1", &fx.seat_a, &[], &identity)) else {
        panic!("prepared");
    };
    let rendered = format!("{plan:?}");
    assert!(!rendered.contains("nsec-secret-value"), "{rendered}");
    assert!(
        !rendered.contains(&*fx.root.to_string_lossy()),
        "{rendered}"
    );
}

#[cfg(target_os = "macos")]
mod launched {
    use super::*;

    fn run_in(plan: &PreparedExecution, cwd: &Path, script: &str) -> std::process::Output {
        let (program, args) = plan
            .launch
            .boundary()
            .wrap("/bin/sh", &["-c".to_owned(), script.to_owned()]);
        let mut command = std::process::Command::new(program);
        command
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .envs(plan.launch.env().vars());
        crate::session::testing::output_within(command, Duration::from_secs(120))
    }

    /// A seat's first launch: its bundle does not exist when the scope is
    /// prepared (the skills are materialized just before the spawn). The
    /// grant must still hold, and the child must read the staged skill.
    #[test]
    fn a_first_seat_reads_skills_staged_after_its_scope_was_prepared() {
        let fx = fixture();
        let bundle = fx.root.join("app/agents/seats/s1");
        assert!(!bundle.exists());
        let mut facts = inputs(&fx, "s1", &fx.seat_a, &[], &[]);
        facts.seat_bundle = Some(&bundle);
        let plan = match prepare(&facts) {
            Ok(ExecutionPlan::Prepared(plan)) => plan,
            other => panic!("prepared: {other:?}"),
        };
        // What `materialize_seat_skills` does, after preparation.
        let skill = bundle.join("skills/run-and-report/SKILL.md");
        std::fs::create_dir_all(skill.parent().expect("parent")).expect("skills");
        std::fs::write(&skill, "STAGED_SKILL\n").expect("skill");
        let output = run_in(&plan, &fx.seat_a, &format!("cat '{}'", skill.display()));
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "STAGED_SKILL\n",
            "{output:?}"
        );
        let output = run_in(
            &plan,
            &fx.seat_a,
            &format!("echo x > '{}/injected'", bundle.display()),
        );
        assert!(
            !output.status.success(),
            "the bundle is read-only to the seat"
        );
    }

    #[test]
    fn a_seats_agents_clone_must_exist_and_a_read_role_cannot_write_it() {
        let fx = fixture();
        let missing = fx.root.join("repos/a-wt-seat-agents");
        let mut facts = inputs(&fx, "s1", &fx.seat_a, &[], &[]);
        facts.agents_checkout = Some((&missing, false));
        let error = prepare(&facts).expect_err("missing clone");
        assert_eq!(error.code, EXECUTION_SCOPE_INVALID);
        assert!(
            error.message.contains("must be prepared"),
            "{}",
            error.message
        );

        std::fs::create_dir_all(missing.join("plans")).expect("clone");
        git(&missing, &["init", "-q", "-b", "main"]);
        std::fs::write(missing.join("plans/own.md"), "OWN_PLAN\n").expect("plan");
        let plan = match prepare(&facts) {
            Ok(ExecutionPlan::Prepared(plan)) => plan,
            other => panic!("prepared: {other:?}"),
        };
        let output = run_in(
            &plan,
            &fx.seat_a,
            &format!("cat '{}/plans/own.md'", missing.display()),
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "OWN_PLAN\n",
            "{output:?}"
        );
        let output = run_in(
            &plan,
            &fx.seat_a,
            &format!("echo x >> '{}/plans/own.md'", missing.display()),
        );
        assert!(
            !output.status.success(),
            "a read role cannot write its agents clone"
        );
        let output = run_in(
            &plan,
            &fx.seat_a,
            &format!("cat '{}'", fx.cache_plan.display()),
        );
        assert!(!output.status.success(), "the host cache stays out");
    }

    /// A Hermit project: a build child resolves the declared toolchain
    /// through the project's own `bin/` shim under the produced policy, from
    /// a fixture-local copy of the host's Hermit state; an undeclared package
    /// in that same state stays out.
    #[test]
    #[ignore = "integration: set BEEKEEPER_TEST_HERMIT_STATE to a Hermit state dir holding just-1.46.0"]
    fn a_declared_hermit_toolchain_runs_from_a_build_child_and_nothing_else() {
        // Only read, to copy the packages into the fixture's own state.
        let host_state = std::env::var_os("BEEKEEPER_TEST_HERMIT_STATE")
            .map(PathBuf::from)
            .expect("BEEKEEPER_TEST_HERMIT_STATE must name the Hermit state this integration test copies");
        assert!(
            host_state.join("pkg/just-1.46.0").is_dir()
                && host_state.join("pkg/hermit@stable").is_dir(),
            "{} holds no just-1.46.0 and hermit@stable",
            host_state.display()
        );
        let fx = fixture();
        let state = fx.root.join("hermit-state");
        for part in [
            "pkg/just-1.46.0",
            "pkg/hermit@stable",
            "pkg/node-24.15.0",
            "sources",
        ] {
            let from = host_state.join(part);
            if !from.exists() {
                continue;
            }
            let to = state.join(part);
            std::fs::create_dir_all(to.parent().expect("parent")).expect("dir");
            let status = std::process::Command::new("/bin/cp")
                .arg("-R")
                .arg(&from)
                .arg(&to)
                .status()
                .expect("cp");
            assert!(status.success());
        }
        // A foreign manifest source beside the declared one: never granted.
        let foreign = state.join("sources/0000foreign");
        std::fs::create_dir_all(&foreign).expect("foreign source");
        git(&foreign, &["init", "-q"]);
        git(
            &foreign,
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/private-manifests.git",
            ],
        );
        std::fs::write(foreign.join("secret.hcl"), "FOREIGN_MANIFEST_CANARY").expect("canary");
        // Hermit's `binaries/<pkg>/<bin>` links name the absolute `pkg/`
        // path; point the fixture's at the fixture's own copy.
        std::fs::create_dir_all(state.join("binaries/just-1.46.0")).expect("binaries");
        std::os::unix::fs::symlink(
            state.join("pkg/just-1.46.0/just"),
            state.join("binaries/just-1.46.0/just"),
        )
        .expect("binary link");
        // Hermit stats the package's downloaded archive to decide it is
        // installed; the host run needed no read grant on it.
        for shard in std::fs::read_dir(host_state.join("cache"))
            .into_iter()
            .flatten()
            .flatten()
        {
            for archive in std::fs::read_dir(shard.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                if archive
                    .file_name()
                    .to_string_lossy()
                    .contains("just-1.46.0")
                {
                    let to = state
                        .join("cache")
                        .join(shard.file_name())
                        .join(archive.file_name());
                    std::fs::create_dir_all(to.parent().expect("parent")).expect("shard");
                    std::fs::copy(archive.path(), &to).expect("archive");
                }
            }
        }
        let repo_bin = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bin");
        let bin = fx.seat_a.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        for file in ["hermit", "hermit.hcl"] {
            std::fs::copy(repo_bin.join(file), bin.join(file)).expect("copy");
        }
        std::os::unix::fs::symlink("hermit", bin.join(".just-1.46.0.pkg")).expect("marker");
        std::os::unix::fs::symlink(".just-1.46.0.pkg", bin.join("just")).expect("shim");
        let mut facts = inputs(&fx, "s1", &fx.seat_a, &[], &[]);
        facts.hermit_state = Some(&state);
        let plan = match prepare(&facts) {
            Ok(ExecutionPlan::Prepared(plan)) => plan,
            other => panic!("prepared: {other:?}"),
        };
        let output = run_in(
            &plan,
            &fx.seat_a,
            "/usr/bin/python3 -c 'import subprocess; r=subprocess.run([\"bin/just\",\"--version\"],capture_output=True,text=True); print(r.stdout + r.stderr)'",
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("just 1.46.0"),
            "{output:?}"
        );
        let output = run_in(
            &plan,
            &fx.seat_a,
            &format!("cat '{}/secret.hcl'", foreign.display()),
        );
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("FOREIGN_MANIFEST_CANARY"),
            "an undeclared manifest source was readable"
        );
        if state.join("pkg/node-24.15.0").exists() {
            let output = run_in(
                &plan,
                &fx.seat_a,
                &format!("ls '{}/pkg/node-24.15.0'", state.display()),
            );
            assert!(
                !output.status.success(),
                "an undeclared package is not granted"
            );
        }
    }

    /// A Claude CLI stand-in that answers `auth status` the way the measured
    /// CLI does inside the boundary: signed in, configuration in the private
    /// directory it was pointed at. No model is involved.
    fn fixture_claude_cli(dir: &Path) -> PathBuf {
        let cli = dir.join("claude-fixture");
        std::fs::write(
            &cli,
            "#!/bin/sh\nprintf '{\"loggedIn\":true,\"configDirectory\":\"%s\"}\\n' \"$CLAUDE_CONFIG_DIR\"\n",
        )
        .expect("cli");
        let mut perms = std::fs::metadata(&cli).expect("meta").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&cli, perms).expect("chmod");
        cli
    }

    /// A bounded Claude seat opens with the project's own settings and
    /// `.mcp.json` loaded natively and the host's pinned settings file — no
    /// host-parsed project servers, no strict override — and runs under one
    /// policy: the old per-tool write fence is not generated. The pinned file
    /// carries the host's scope and seat values but never a secret; the child
    /// reads its control files and cannot change them, and owns its state.
    #[tokio::test]
    async fn a_bounded_claude_session_loads_its_project_settings_under_host_pinned_values() {
        let fx = fixture();
        let cli = fixture_claude_cli(&fx.root);
        std::fs::create_dir_all(fx.seat_a.join(".claude")).expect("settings dir");
        std::fs::write(
            fx.seat_a.join(".mcp.json"),
            r#"{"mcpServers":{"project-tools":{"command":"./tools/mcp"}}}"#,
        )
        .expect("mcp");
        let agent = fake_agent(
            &fx.seat_a,
            "recording-agent",
            &format!(
                r#"
cd -- "{seat}" || exit 90
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":2,"agentInfo":{{"name":"claude-agent-acp"}}}}}}
' "$id" ;;
    *'"method":"session/new"'*)
      printf '%s
' "$line" > session-new.json
      settings=$(printf '%s' "$line" | sed -n 's/.*"settings":"\([^"]*\)".*/\1/p')
      if cat "$settings" > /dev/null 2>&1; then echo settings_read=OK >> report; else echo settings_read=DENIED >> report; fi
      if echo x >> "$settings" 2>/dev/null; then echo settings_write=OK >> report; else echo settings_write=DENIED >> report; fi
      control=$(dirname "$settings")
      if echo x >> "$control/gitconfig" 2>/dev/null; then echo gitconfig_write=OK >> report; else echo gitconfig_write=DENIED >> report; fi
      if echo x > "$control/owner.json" 2>/dev/null; then echo owner_write=OK >> report; else echo owner_write=DENIED >> report; fi
      if echo x > "$TMPDIR/own-state" 2>/dev/null; then echo state_write=OK >> report; else echo state_write=DENIED >> report; fi
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"sessionId":"x"}}}}
' "$id" ;;
  esac
done
"#,
                seat = fx.seat_a.display()
            ),
        );
        let agent_env = vec![(
            "CLAUDE_CODE_EXECUTABLE".to_owned(),
            cli.display().to_string(),
        )];
        let identity = vec![
            (
                "BUZZ_RELAY_URL".to_owned(),
                "wss://relay.example.invalid".to_owned(),
            ),
            ("BUZZ_PRIVATE_KEY".to_owned(), "nsec-seat-secret".to_owned()),
        ];
        let mut facts = inputs(&fx, "s1", &fx.seat_a, &agent_env, &identity);
        facts.runtime = RuntimeProfile::Claude;
        let plan = prepare(&facts).expect("prepared");
        let ExecutionPlan::Prepared(prepared) = &plan else {
            panic!("prepared");
        };
        let options = prepared.claude_options.clone().expect("claude options");
        assert_eq!(
            options["settingSources"],
            serde_json::json!(["project", "local"])
        );
        assert!(options.get("strictMcpConfig").is_none(), "{options:?}");
        let settings_path = PathBuf::from(options["settings"].as_str().expect("settings path"));
        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).expect("settings"))
                .expect("json");
        assert_eq!(settings["disableClaudeAiConnectors"], true);
        assert_eq!(settings["sandbox"]["enabled"], false);
        assert_eq!(
            settings["env"]["BUZZ_RELAY_URL"],
            "wss://relay.example.invalid"
        );
        assert!(
            settings["env"]["GIT_CONFIG_GLOBAL"].is_string(),
            "{settings}"
        );
        assert_eq!(
            settings["env"]["BUZZ_PRIVATE_KEY"], "nsec-seat-secret",
            "the seat's identity is pinned, key included, in the host's private file"
        );
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(
                &std::fs::metadata(&settings_path)
                    .expect("meta")
                    .permissions()
            ) & 0o777,
            0o600
        );
        assert!(
            settings["env"].get("CLAUDE_CONFIG_DIR").is_none(),
            "{settings}"
        );

        let (tx, _rx) = mpsc::channel(16);
        let mut manager = SessionManager::new(tx);
        let mut request = create_request(&fx.seat_a, agent, plan);
        request.seat = Some(crate::session::SeatIdentity {
            actor_pubkey: "cd".repeat(32),
            role: "builder".to_owned(),
            relay_url: "wss://relay.example.invalid".to_owned(),
        });
        manager.create(request).await.expect("create");
        wait_for(&fx.seat_a.join("session-new.json")).await;
        let raw = std::fs::read_to_string(fx.seat_a.join("session-new.json")).expect("request");
        let request: serde_json::Value = serde_json::from_str(&raw).expect("json");
        assert_eq!(
            request["params"]["mcpServers"],
            serde_json::json!([]),
            "the host passes only its own servers; the project's load natively"
        );
        assert_eq!(
            request["params"]["_meta"]["claudeCode"]["options"]["settings"],
            settings_path.display().to_string()
        );
        assert!(
            !raw.contains("nsec-seat-secret"),
            "a secret reached the ACP request"
        );
        let report = report(&fx.seat_a);
        for (probe, expected) in [
            ("settings_read", "OK"),
            ("settings_write", "DENIED"),
            ("gitconfig_write", "DENIED"),
            ("owner_write", "DENIED"),
            ("state_write", "OK"),
        ] {
            assert_eq!(
                report.get(probe).map(String::as_str),
                Some(expected),
                "{probe}: {report:?}"
            );
        }
        assert!(
            !fx.seat_a
                .join(crate::agent_fence::WRITE_FENCE_SETTINGS_FILE)
                .exists(),
            "the legacy per-tool fence was generated on a bounded launch"
        );
        manager.shutdown("s1");
    }

    fn plan_of(result: Result<ExecutionPlan, CreateFailure>) -> Box<PreparedExecution> {
        match result {
            Ok(ExecutionPlan::Prepared(plan)) => plan,
            other => panic!("prepared: {other:?}"),
        }
    }

    fn state_of(plan: &PreparedExecution) -> PathBuf {
        let tmp = plan.launch.env().get("TMPDIR").expect("TMPDIR");
        Path::new(tmp.trim_end_matches('/'))
            .parent()
            .expect("state")
            .to_path_buf()
    }

    /// Continuation cannot be steered elsewhere through the directories the
    /// child owns: under its real boundary the child cannot replace its state
    /// or working-tree anchors with a link, and when such a replacement is
    /// found anyway (made from outside), the next preparation refuses rather
    /// than granting the link's target. An ordinary resume of the same scope
    /// is unaffected.
    #[test]
    fn a_continuation_is_never_granted_what_a_replaced_anchor_points_at() {
        let fx = fixture();
        let foreign = fx.root.join("repos/b");
        let facts = inputs(&fx, "s1", &fx.seat_a, &[], &[]);
        let first = plan_of(prepare(&facts));
        let state = state_of(&first);

        // The child tries: denied at its own anchors.
        let output = run_in(
            &first,
            &fx.seat_a,
            &format!(
                "rm -rf '{s}/tmp' '{s}/xdg'; rmdir '{s}' 2>/dev/null && echo STATE_REPLACED; \
                 cd / && rm -rf '{t}/README.md' && rmdir '{t}' 2>/dev/null && echo TREE_REPLACED; true",
                s = state.display(),
                t = fx.seat_a.display()
            ),
        );
        let said = String::from_utf8_lossy(&output.stdout);
        assert!(
            !said.contains("STATE_REPLACED") && !said.contains("TREE_REPLACED"),
            "{said}"
        );
        assert!(state.is_dir() && fx.seat_a.is_dir());

        // Own resume, same scope: prepared, native history reattached.
        let binding = first.binding.clone();
        let mut resumed = inputs(&fx, "s1", &fx.seat_a, &[], &[]);
        resumed.prior = Some((Some(&binding), Some("acp-1")));
        let again = plan_of(prepare(&resumed));
        assert_eq!(again.native_refusal, None);

        // A state directory replaced by a link (from outside the boundary).
        std::fs::rename(&state, state.with_extension("moved")).expect("move state");
        std::os::unix::fs::symlink(&foreign, &state).expect("link state");
        let error = prepare(&resumed).expect_err("replaced state");
        assert_eq!(error.code, EXECUTION_SCOPE_INVALID, "{}", error.message);
        std::fs::remove_file(&state).expect("unlink");
        std::fs::rename(state.with_extension("moved"), &state).expect("restore state");

        // A runtime directory inside it, likewise.
        let xdg = state.join("xdg");
        std::fs::remove_dir_all(&xdg).expect("rm xdg");
        std::os::unix::fs::symlink(&foreign, &xdg).expect("link xdg");
        assert_eq!(
            prepare(&resumed).expect_err("replaced xdg").code,
            EXECUTION_SCOPE_INVALID
        );
        std::fs::remove_file(&xdg).expect("unlink xdg");

        // The working tree itself replaced by a link to another project's
        // directory: the launch is refused, not merely stripped of its
        // history, and the target is never granted.
        std::fs::rename(&fx.seat_a, fx.seat_a.with_extension("moved")).expect("move tree");
        std::os::unix::fs::symlink(&foreign, &fx.seat_a).expect("link tree");
        let mut bound = resumed.clone();
        bound.association = WorkspaceAssociation::HostBound;
        let error = prepare(&bound).expect_err("replaced tree");
        assert_eq!(error.code, EXECUTION_SCOPE_INVALID);
        assert!(
            error
                .message
                .contains("instead of the directory it was bound to"),
            "{}",
            error.message
        );
        assert_eq!(
            std::fs::read_to_string(&fx.b_plan).expect("b"),
            "B_PLAN_CANARY\n"
        );
    }

    #[test]
    #[ignore = "integration: set BEEKEEPER_TEST_BEE to a built bee (cargo build -p buzz-cli)"]
    fn the_hosts_bee_is_found_by_name_standalone_and_bundled() {
        let built = crate::session::testing::required_tool("BEEKEEPER_TEST_BEE");
        let fx = fixture();
        let standalone = fx.root.join("host-tools/bee");
        let bundled = fx.root.join("Beekeeper.app/Contents/MacOS/bee");
        for bee in [&standalone, &bundled] {
            std::fs::create_dir_all(bee.parent().expect("dir")).expect("dir");
            std::fs::copy(&built, bee).expect("copy bee");
        }
        std::fs::write(
            fx.root.join("host-tools/unrelated.txt"),
            "UNRELATED_TOOL_DATA",
        )
        .expect("beside");
        for (session, bee) in [("sa", &standalone), ("sb", &bundled)] {
            let mut facts = inputs(&fx, session, &fx.seat_a, &[], &[]);
            facts.actor = None;
            facts.seat_bee = Some(bee);
            let plan = plan_of(prepare(&facts));
            let output = run_in(
                &plan,
                &fx.seat_a,
                &format!(
                    "echo BEE=$BEE; bee plans example | head -20; cat '{}' 2>&1 | head -1",
                    fx.root.join("host-tools/unrelated.txt").display()
                ),
            );
            let out = String::from_utf8_lossy(&output.stdout);
            assert!(out.contains(&format!("BEE={}", bee.display())), "{out}");
            assert!(
                out.contains("beekeeper-plan/v1"),
                "bee by name did not run: {out}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                !out.contains("UNRELATED_TOOL_DATA"),
                "the bee's directory was lent: {out}"
            );
        }
    }

    /// The operator's selected Git transport, through real `git push` to a
    /// disposable authenticated receiver: an unseated session pushes with
    /// the configured credential helper resolved by the host and the
    /// operator's keyfile (granted to it alone, read-only); a seat pushes
    /// with its own key and cannot read the operator's keyfile.
    #[test]
    fn solo_pushes_with_the_operators_transport_and_a_seat_never_reads_its_keyfile() {
        let fx = fixture();
        let remote_root = fx.root.join("receiver");
        std::fs::create_dir_all(&remote_root).expect("receiver root");
        git(&remote_root, &["init", "-q", "--bare", "remote.git"]);
        let receiver = crate::session::testing::git_receiver(&remote_root, "operator-key,seat-key");
        // The operator's global configuration: a bare-named helper that lives
        // beside the host's bee, and a keyfile.
        let tools = fx.root.join("host-tools");
        std::fs::create_dir_all(&tools).expect("tools");
        let bee = tools.join("bee");
        std::fs::write(&bee, "#!/bin/sh\necho bee\n").expect("bee");
        let helper = tools.join("git-credential-fixture");
        std::fs::write(
            &helper,
            "#!/bin/sh\n[ \"$1\" = get ] || exit 0\nkey=\"$NOSTR_PRIVATE_KEY\"\n\
             if [ -z \"$key\" ]; then f=$(git config --get nostr.keyfile) && key=$(cat \"$f\"); fi\n\
             [ -n \"$key\" ] || exit 0\necho username=t\necho password=$key\n",
        )
        .expect("helper");
        for file in [&bee, &helper] {
            let mut perms = std::fs::metadata(file).expect("meta").permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
            std::fs::set_permissions(file, perms).expect("chmod");
        }
        let keyfile = fx.root.join("operator-home/.config/nostr.key");
        std::fs::create_dir_all(keyfile.parent().expect("dir")).expect("dir");
        std::fs::write(&keyfile, "operator-key").expect("keyfile");
        let global = fx.root.join("operator-gitconfig");
        std::fs::write(
            &global,
            format!(
                "[credential]\n\thelper = fixture\n[nostr]\n\tkeyfile = {}\n[user]\n\tname = Op\n\temail = op@example.invalid\n",
                keyfile.display()
            ),
        )
        .expect("global");
        crate::execution_scope_git::TEST_OPERATOR_CONFIG
            .with(|config| *config.borrow_mut() = Some(global.clone()));
        let url = format!("http://127.0.0.1:{}/remote.git", receiver.port);
        let push = |plan: &PreparedExecution, branch: &str| {
            run_in(
                plan,
                &fx.seat_a,
                &format!(
                    "git -c user.name=t -c user.email=t@x commit -q --allow-empty -m {branch} && \
                     git push -q '{url}' HEAD:refs/heads/{branch} && echo PUSHED; \
                     if cat '{k}' >/dev/null 2>&1; then echo KEYFILE=READ; else echo KEYFILE=DENIED; fi",
                    k = keyfile.display()
                ),
            )
        };

        let mut solo = inputs(&fx, "solo", &fx.seat_a, &[], &[]);
        solo.actor = None;
        solo.seat_bee = Some(&bee);
        let plan = plan_of(prepare(&solo));
        let out = push(&plan, "solo");
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(
            said.contains("PUSHED"),
            "{said}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            said.contains("KEYFILE=READ"),
            "Solo pushes with the operator's own keyfile: {said}"
        );

        let seat_identity = vec![("NOSTR_PRIVATE_KEY".to_owned(), "seat-key".to_owned())];
        let mut seated = inputs(&fx, "seat", &fx.seat_a, &[], &seat_identity);
        seated.seat_bee = Some(&bee);
        let plan = plan_of(prepare(&seated));
        let out = push(&plan, "seat");
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(
            said.contains("PUSHED"),
            "{said}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            said.contains("KEYFILE=DENIED"),
            "a seat read the operator's keyfile: {said}"
        );
        crate::execution_scope_git::TEST_OPERATOR_CONFIG.with(|config| *config.borrow_mut() = None);
        let pushed = std::process::Command::new("git")
            .args([
                "--git-dir",
                &remote_root.join("remote.git").display().to_string(),
                "branch",
            ])
            .output()
            .expect("branches");
        let branches = String::from_utf8_lossy(&pushed.stdout);
        assert!(
            branches.contains("solo") && branches.contains("seat"),
            "{branches}"
        );
    }

    /// An unseated session's own agents context: a read-only clone staged
    /// from the repository this host recorded for the project, at its
    /// fetched tip — readable, not writable, and not the host's own clone.
    #[tokio::test]
    async fn solo_reads_its_own_agents_clone_staged_from_the_recorded_repository() {
        let fx = fixture();
        let upstream = fx.root.join("upstream-agents");
        std::fs::create_dir_all(upstream.join("plans")).expect("upstream");
        git(&upstream, &["init", "-q", "-b", "main"]);
        std::fs::write(upstream.join("plans/own.md"), "OWN_PLAN\n").expect("plan");
        git(&upstream, &["add", "."]);
        git(&upstream, &["commit", "-q", "-m", "plan"]);
        let cache = fx.root.join("app/packs/project-a-agents");
        std::fs::create_dir_all(cache.parent().expect("parent")).expect("packs");
        git(
            &fx.root,
            &[
                "clone",
                "-q",
                &upstream.display().to_string(),
                &cache.display().to_string(),
            ],
        );
        let record = crate::agents_checkout::AgentsRepoRecord {
            path: cache.clone(),
            ref_name: "refs/heads/main".to_owned(),
            url: Some(upstream.display().to_string()),
        };
        let dest = fx.state_dir.join(AGENTS_CLONES_DIR).join("solo");
        std::fs::create_dir_all(dest.parent().expect("parent")).expect("clones");
        crate::agents_checkout::stage_read_only_clone(&record, &dest)
            .await
            .expect("staged");
        let mut facts = inputs(&fx, "solo", &fx.seat_a, &[], &[]);
        facts.actor = None;
        facts.agents_checkout = Some((&dest, false));
        let plan = plan_of(prepare(&facts));
        let output = run_in(
            &plan,
            &fx.seat_a,
            &format!(
                "cat '{d}/plans/own.md'; echo x >> '{d}/plans/own.md' 2>/dev/null && echo WROTE; \
                 cat '{c}/plans/own.md' 2>/dev/null && echo CACHE_READ; true",
                d = dest.display(),
                c = cache.display()
            ),
        );
        let out = String::from_utf8_lossy(&output.stdout);
        assert!(out.contains("OWN_PLAN"), "{out}");
        assert!(
            !out.contains("WROTE"),
            "the Solo agents clone is read-only: {out}"
        );
        assert!(
            !out.contains("CACHE_READ"),
            "the host's own clone stays out: {out}"
        );
        // A recorded repository that cannot be staged is an error, not "no
        // agents context".
        let broken = crate::agents_checkout::AgentsRepoRecord {
            path: fx.root.join("no-such-cache"),
            ..record
        };
        assert!(crate::agents_checkout::stage_read_only_clone(
            &broken,
            &fx.state_dir.join(AGENTS_CLONES_DIR).join("broken")
        )
        .await
        .is_err());
    }

    /// The provider fixtures' layout: an executable adapter script beside the
    /// working tree (not inside it), named through the `/var` spelling of the
    /// temp directory, started directly rather than through `bash <script>`.
    #[test]
    fn an_adapter_outside_the_tree_is_readable_and_runnable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        let state = dir.path().join("state");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::create_dir_all(&state).expect("state");
        let agent = fake_agent(dir.path(), "good-agent", "echo ADAPTER_RAN");
        let mut inputs = ScopeInputs::new(ScopePurpose::Session, &state, "s1", &cwd);
        inputs.driver = "claude-agent-acp";
        inputs.runtime = RuntimeProfile::TestDouble;
        inputs.agent_command = &agent;
        let plan = match prepare(&inputs) {
            Ok(ExecutionPlan::Prepared(plan)) => plan,
            other => panic!("prepared: {other:?}"),
        };
        let boundary = plan.launch.boundary();
        let (program, args) = boundary.wrap(&agent, &[]);
        let output = std::process::Command::new(program)
            .args(args)
            .current_dir(&cwd)
            .env_clear()
            .envs(plan.launch.env().vars())
            .output()
            .expect("spawn");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("ADAPTER_RAN"),
            "{output:?}\ngrants: {:#?}",
            boundary.grants()
        );
    }

    async fn wait_for(path: &Path) {
        for _ in 0..200 {
            if path.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("{} never appeared", path.display());
    }

    async fn finish_turn(rx: &mut mpsc::Receiver<SessionEvent>) {
        loop {
            match tokio::time::timeout(Duration::from_secs(20), rx.recv())
                .await
                .expect("event")
                .expect("open")
            {
                SessionEvent::TurnFinished { .. } => return,
                _ => continue,
            }
        }
    }

    fn head(repo: &Path, name: &str) -> String {
        let output = std::process::Command::new("git")
            .args(["rev-parse", name])
            .current_dir(repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// The probe's placement guard refuses before any mutation when it is not
    /// inside its own disposable fixture: a wrong launch directory or a
    /// foreign `GIT_DIR` leaves the other repository exactly as it was.
    #[test]
    fn the_probe_guard_aborts_before_mutating_a_foreign_repository() {
        let fx = fixture();
        let other = fixture();
        let script = fx.seat_a.join("guarded-probe");
        std::fs::write(&script, probe_agent(&fx, true)).expect("script");
        let before = (head(&other.repo_a, "main"), head(&other.repo_a, "HEAD"));
        // 1. Launched from another repository's directory: the guard moves to
        //    its own fixture first, so the other repository is never the cwd.
        let status = std::process::Command::new("/bin/bash")
            .arg(&script)
            .current_dir(&other.repo_a)
            .stdin(std::process::Stdio::null())
            .status()
            .expect("bash");
        assert!(
            status.success(),
            "no input: the loop ends without a mutation"
        );
        // 2. A foreign GIT_DIR: the common-dir check fails before any probe.
        let status = std::process::Command::new("/bin/bash")
            .arg(&script)
            .current_dir(&other.repo_a)
            .env("GIT_DIR", other.repo_a.join(".git"))
            .stdin(std::process::Stdio::null())
            .status()
            .expect("bash");
        assert!(
            matches!(status.code(), Some(93 | 94)),
            "the guard must refuse a foreign Git directory: {status:?}"
        );
        assert_eq!(
            (head(&other.repo_a, "main"), head(&other.repo_a, "HEAD")),
            before,
            "the other repository was touched"
        );
        assert!(!other.repo_a.join("report").exists());
        assert!(!fx.seat_a.join("report").exists(), "no probe ran");
    }

    /// RED on the legacy spawn, GREEN under the prepared scope: the same probe
    /// agent shape, through `SessionManager`. The legacy run performs no
    /// mutation at all.
    #[tokio::test]
    async fn the_legacy_spawn_reaches_other_projects_and_the_prepared_scope_does_not() {
        // RED: what the provider did before a scope existed (reads only).
        let fx = fixture();
        let agent = fake_agent(&fx.seat_a, "probe-agent", &probe_agent(&fx, false));
        let (tx, _rx) = mpsc::channel(64);
        let mut manager = SessionManager::new(tx);
        manager
            .create(create_request(
                &fx.seat_a,
                agent,
                ExecutionPlan::Legacy {
                    reason: "unit-test",
                },
            ))
            .await
            .expect("legacy create");
        wait_for(&fx.seat_a.join("phase1")).await;
        let red = report(&fx.seat_a);
        assert!(
            !red.contains_key("git_commit"),
            "the legacy run mutates nothing"
        );
        for probe in ["sibling_abs", "other_root", "host_cache", "python_child"] {
            assert_eq!(
                red.get(probe).map(String::as_str),
                Some("OK"),
                "legacy {probe}: {red:?}"
            );
        }
        manager.shutdown("s1");

        // GREEN: the prepared scope, with the mutating probes.
        let fx = fixture();
        let agent = fake_agent(&fx.seat_a, "probe-agent", &probe_agent(&fx, true));
        let agent_env = vec![("RUNTIME_SENTINEL".to_owned(), "runtime-a".to_owned())];
        let identity = vec![
            ("SEAT_SENTINEL".to_owned(), "seat-a".to_owned()),
            ("BUZZ_PRIVATE_KEY".to_owned(), "nsec-seat-a".to_owned()),
        ];
        let main_before = head(&fx.repo_a, "main");
        let plan =
            prepare(&inputs(&fx, "s1", &fx.seat_a, &agent_env, &identity)).expect("prepared");
        assert!(matches!(plan.state(), BoundaryState::Enforced { .. }));
        let (tx, mut rx) = mpsc::channel(64);
        let mut manager = SessionManager::new(tx);
        manager
            .create(create_request(&fx.seat_a, agent, plan))
            .await
            .expect("bounded create");
        wait_for(&fx.seat_a.join("phase1")).await;
        let green = report(&fx.seat_a);
        for probe in ["own_read", "own_write", "git_commit"] {
            assert_eq!(
                green.get(probe).map(String::as_str),
                Some("OK"),
                "{probe}: {green:?}"
            );
        }
        for probe in [
            "sibling_abs",
            "sibling_rel",
            "sibling_symlink",
            "sibling_list",
            "other_root",
            "host_cache",
            "python_child",
            "shell_child",
            "sibling_write",
            "git_other_branch",
            "git_new_branch",
            "git_shared_config",
            "git_hook",
        ] {
            assert_eq!(
                green.get(probe).map(String::as_str),
                Some("DENIED"),
                "{probe}: {green:?}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(&fx.b_plan).expect("b"),
            "B_PLAN_CANARY\n"
        );
        assert_eq!(head(&fx.repo_a, "main"), main_before, "main did not move");
        assert_ne!(
            head(&fx.repo_a, "seat-branch"),
            main_before,
            "the seat committed"
        );

        // The environment the adapter's *grandchild* saw, by named sentinel.
        let env = std::fs::read_to_string(fx.seat_a.join("child-env")).expect("child env");
        for expected in [
            "SEAT_SENTINEL=seat-a",
            "RUNTIME_SENTINEL=runtime-a",
            "BUZZ_PRIVATE_KEY=<present>",
            "CARGO_MANIFEST_DIR=<absent>",
            "CARGO_PKG_NAME=<absent>",
        ] {
            assert!(env.lines().any(|l| l == expected), "{expected}: {env}");
        }
        let private = fx.state_dir.join("executions/s1-");
        let private = private.display();
        assert!(
            env.lines()
                .any(|l| l.starts_with(&format!("TMPDIR={private}")) && l.ends_with("/tmp/")),
            "{env}"
        );
        assert!(
            env.lines()
                .any(|l| l.starts_with(&format!("GIT_CONFIG_GLOBAL={private}"))
                    && l.ends_with("/gitconfig")),
            "{env}"
        );

        // A sibling created while this very process is alive.
        let late = fx.root.join("repos/late/plans/plan.md");
        std::fs::create_dir_all(late.parent().expect("parent")).expect("late");
        std::fs::write(&late, "LATE_CANARY\n").expect("late plan");
        std::fs::write(
            fx.seat_a.join("late-path"),
            late.to_string_lossy().as_bytes(),
        )
        .expect("name it");
        manager
            .handle("s1")
            .expect("handle")
            .deliver(crate::session::SessionCommand::Turn {
                command_id: "late".into(),
                text: "probe the late sibling".into(),
                attachments: Vec::new(),
                operator_pubkey: None,
                framing: None,
            })
            .expect("deliver");
        finish_turn(&mut rx).await;
        let after = report(&fx.seat_a);
        assert_eq!(
            after.get("late_sibling").map(String::as_str),
            Some("DENIED"),
            "{after:?}"
        );
        assert_eq!(
            after.get("own_after").map(String::as_str),
            Some("OK"),
            "{after:?}"
        );
        manager.shutdown("s1");
    }

    /// A→B→A, then A and B at once: each execution's descendants see exactly
    /// its own named values and nothing only the other holds. A also resolves
    /// a project-only tool and the seat's `bee` from a build child.
    #[tokio::test]
    async fn environments_never_cross_between_executions() {
        let fx = fixture();
        let seat_b = fx.root.join("repos/b-seat");
        std::fs::create_dir_all(seat_b.join(".probe")).expect("b seat");
        std::fs::write(seat_b.join(".probe/sentinels.py"), SENTINEL_PRINTER).expect("printer");
        // A's project tool directory and its seat's bee, both in A's tree.
        let tools = fx.seat_a.join("tools/bin");
        let bee_dir = fx.seat_a.join("beebin");
        for (dir, name, output) in [
            (&tools, "project-tool", "PROJECT_TOOL_RAN"),
            // Same name as a system tool: the project's must win.
            (&tools, "uname", "PROJECT_UNAME_WON"),
            (&bee_dir, "bee", "SEAT_BEE_RAN"),
        ] {
            std::fs::create_dir_all(dir).expect("dir");
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\necho {output}\n")).expect("tool");
            let mut perms = std::fs::metadata(&path).expect("meta").permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
            std::fs::set_permissions(&path, perms).expect("chmod");
        }
        let dump_agent = |dir: &Path, name: &str| {
            fake_agent(
                dir,
                name,
                &format!(
                    r#"
cd -- "{dir}" || exit 90
/bin/sh -c '/usr/bin/python3 .probe/sentinels.py' > "$PWD/env-$$"
/usr/bin/python3 -c 'import subprocess; print(subprocess.run(["/bin/sh","-c","project-tool; bee; uname"],capture_output=True,text=True).stdout)' >> "$PWD/env-$$" 2>/dev/null
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":2}}}}\n' "$id" ;;
    *'"method":"session/new"'*) printf '{{"jsonrpc":"2.0","id":%s,"result":{{"sessionId":"x"}}}}\n' "$id" ;;
  esac
done
"#,
                    dir = dir.display()
                ),
            )
        };
        let a_runtime = vec![
            ("PROJECT_A_ONLY".to_owned(), "canary-a".to_owned()),
            (
                "PATH".to_owned(),
                format!("{}:/usr/bin:/bin", tools.display()),
            ),
        ];
        let a_id = vec![
            ("SEAT_SENTINEL".to_owned(), "seat-a".to_owned()),
            (
                "PATH".to_owned(),
                format!(
                    "{}:/usr/bin:/bin:/Users/nobody/host-only",
                    bee_dir.display()
                ),
            ),
        ];
        let b_runtime: Vec<(String, String)> = Vec::new();
        let b_id = vec![("SEAT_SENTINEL".to_owned(), "seat-b".to_owned())];
        let agent_a = dump_agent(&fx.seat_a, "dump-a");
        let agent_b = dump_agent(&seat_b, "dump-b");
        let start = |cwd: PathBuf,
                     session: &'static str,
                     runtime: Vec<(String, String)>,
                     id: Vec<(String, String)>,
                     agent: String| {
            let fx = &fx;
            let bee = bee_dir.join("bee");
            async move {
                let mut facts = inputs(fx, session, &cwd, &runtime, &id);
                if session.starts_with('a') {
                    facts.seat_bee = Some(&bee);
                } else {
                    // Project B's own directory, bound to B by a host record.
                    facts.project_ref = Some("30621:aa:project-b");
                    facts.project_checkout = None;
                    facts.association = WorkspaceAssociation::HostBound;
                }
                let plan = prepare(&facts).expect("prepared");
                let (tx, _rx) = mpsc::channel(8);
                let mut manager = SessionManager::new(tx);
                let mut request = create_request(&cwd, agent, plan);
                request.target.session_id = session.to_owned();
                manager.create(request).await.expect("create");
                (manager, session)
            }
        };
        for (cwd, session, runtime, id, agent) in [
            (
                fx.seat_a.clone(),
                "a1",
                a_runtime.clone(),
                a_id.clone(),
                agent_a.clone(),
            ),
            (
                seat_b.clone(),
                "b1",
                b_runtime.clone(),
                b_id.clone(),
                agent_b.clone(),
            ),
            (
                fx.seat_a.clone(),
                "a2",
                a_runtime.clone(),
                a_id.clone(),
                agent_a.clone(),
            ),
        ] {
            let (mut manager, session) = start(cwd, session, runtime, id, agent).await;
            tokio::time::sleep(Duration::from_millis(400)).await;
            manager.shutdown(session);
        }
        let (mut ma, sa) = start(
            fx.seat_a.clone(),
            "a3",
            a_runtime.clone(),
            a_id.clone(),
            agent_a.clone(),
        )
        .await;
        let (mut mb, sb) = start(
            seat_b.clone(),
            "b2",
            b_runtime.clone(),
            b_id.clone(),
            agent_b.clone(),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        ma.shutdown(sa);
        mb.shutdown(sb);

        let dumps = |dir: &Path| -> Vec<String> {
            std::fs::read_dir(dir)
                .expect("dir")
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("env-"))
                .map(|e| std::fs::read_to_string(e.path()).expect("dump"))
                .collect()
        };
        let a_dumps = dumps(&fx.seat_a);
        let b_dumps = dumps(&seat_b);
        assert_eq!(a_dumps.len(), 3, "A ran three times");
        assert_eq!(b_dumps.len(), 2, "B ran twice");
        for dump in &a_dumps {
            assert!(dump.contains("PROJECT_A_ONLY=canary-a"), "{dump}");
            assert!(dump.contains("SEAT_SENTINEL=seat-a"), "{dump}");
            assert!(
                dump.contains("PROJECT_TOOL_RAN"),
                "project tool unresolved: {dump}"
            );
            assert!(dump.contains("SEAT_BEE_RAN"), "seat bee unresolved: {dump}");
            assert!(
                dump.contains("PROJECT_UNAME_WON"),
                "a host directory outranked the project's tool: {dump}"
            );
            assert!(!dump.contains("seat-b"), "{dump}");
        }
        for dump in &b_dumps {
            assert!(
                dump.contains("PROJECT_A_ONLY=<absent>"),
                "A's value reached B: {dump}"
            );
            assert!(dump.contains("SEAT_SENTINEL=seat-b"), "{dump}");
            assert!(
                !dump.contains("seat-a") && !dump.contains("PROJECT_TOOL_RAN"),
                "{dump}"
            );
        }
    }
}
