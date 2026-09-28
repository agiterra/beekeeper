//! Project preparation that ordinary work depends on, exercised through the
//! production [`prepare`] and the boundary it renders: temporary storage,
//! the allocated branch, the project's own agents context and environment.
//! Each denial is paired with the authorized operation that must still work.

use super::tests::launched::run_in;
use super::tests::{fixture, inputs};
use super::*;

fn prepared(plan: Result<ExecutionPlan, CreateFailure>) -> Box<PreparedExecution> {
    match plan {
        Ok(ExecutionPlan::Prepared(plan)) => plan,
        other => panic!("prepared: {other:?}"),
    }
}

fn private_temp(plan: &PreparedExecution) -> PathBuf {
    let tmpdir = plan
        .launch
        .env()
        .vars()
        .into_iter()
        .find(|(name, _)| *name == "TMPDIR")
        .map(|(_, value)| PathBuf::from(value))
        .expect("TMPDIR is part of the prepared environment");
    tmpdir.canonicalize().expect("private temp exists")
}

/// Run11 (ledger 272(c)): `mktemp -d` failed for the lead and verifier.
/// macOS `mktemp` prefers the per-user Darwin temp directory over `TMPDIR`,
/// which the boundary rightly does not open. Every ordinary form must land
/// in the execution's private temp, and the shared directory stays closed.
#[test]
fn ordinary_temporary_files_land_in_the_executions_private_storage() {
    let fx = fixture();
    let plan = prepared(prepare(&inputs(&fx, "s1", &fx.seat_a, &[], &[])));
    let temp = private_temp(&plan);
    for form in [
        "mktemp",
        "mktemp -d",
        "mktemp -t probe",
        "mktemp -d -t probe",
        "mktemp probe.XXXXXX",
        "mktemp -d \"$TMPDIR/probe.XXXXXX\"",
        "mktemp -p \"$TMPDIR\"",
    ] {
        let output = run_in(&plan, &fx.seat_a, &format!("p=$({form}) && echo \"$p\""));
        assert!(output.status.success(), "{form}: {output:?}");
        let made = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        let made = if made.is_absolute() {
            made
        } else {
            fx.seat_a.join(made)
        };
        let made = made.canonicalize().expect("created");
        assert!(
            made.starts_with(&temp) || made.starts_with(&fx.seat_a),
            "{form} created {made:?}, outside the execution's storage"
        );
    }
    let output = run_in(
        &plan,
        &fx.seat_a,
        "python3 -c 'import tempfile,os;d=tempfile.mkdtemp();print(d)'",
    );
    assert!(output.status.success(), "python tempfile: {output:?}");
    // The operator's shared temp directory is not reopened to make this work.
    let shared = std::process::Command::new("/usr/bin/getconf")
        .arg("DARWIN_USER_TEMP_DIR")
        .output()
        .expect("getconf");
    let shared = PathBuf::from(String::from_utf8_lossy(&shared.stdout).trim().to_owned());
    let canary = tempfile::Builder::new()
        .prefix("tmp.bk-canary")
        .tempdir_in(&shared)
        .expect("shared canary");
    std::fs::write(canary.path().join("c"), "SHARED_TEMP_CANARY\n").expect("canary");
    let output = run_in(
        &plan,
        &fx.seat_a,
        &format!(
            "cat '{}/c'; ls '{}'",
            canary.path().display(),
            shared.display()
        ),
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("SHARED_TEMP_CANARY")
            && !String::from_utf8_lossy(&output.stdout).contains("tmp.bk-canary"),
        "the shared temp directory must stay closed: {output:?}"
    );
}

/// The same through a login shell (Codex runs `bash -lc`; Claude's shell
/// snapshot comes from a login shell): `/etc/zprofile`'s `path_helper` would
/// move the host's tool directory behind `/usr/bin`. And a host project
/// command (verify, build) gets the same private temp.
#[test]
fn login_shells_and_host_commands_keep_temporary_files_private() {
    let fx = fixture();
    let plan = prepared(prepare(&inputs(&fx, "s1", &fx.seat_a, &[], &[])));
    let temp = private_temp(&plan);
    for shell in ["/bin/zsh -l -c", "/bin/bash -l -c"] {
        let output = run_in(
            &plan,
            &fx.seat_a,
            &format!("{shell} 'd=$(mktemp -d) && echo \"$d\"'"),
        );
        assert!(output.status.success(), "{shell}: {output:?}");
        let made = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
            .canonicalize()
            .expect("created");
        assert!(made.starts_with(&temp), "{shell} created {made:?}");
        assert!(
            output.stderr.is_empty(),
            "a denied path_helper is silent to the login profile: {output:?}"
        );
    }
    // Tools the system search path supplies still resolve in a login shell.
    let output = run_in(
        &plan,
        &fx.seat_a,
        "/bin/zsh -l -c 'command -v git && command -v python3'",
    );
    assert!(output.status.success(), "login shell tools: {output:?}");

    let mut host = inputs(&fx, "host-1", &fx.seat_a, &[], &[]);
    host.purpose = ScopePurpose::HostCommand;
    let plan = prepared(prepare(&host));
    let temp = private_temp(&plan);
    let output = run_in(&plan, &fx.seat_a, "d=$(mktemp -d) && echo \"$d\"");
    assert!(output.status.success(), "host command mktemp: {output:?}");
    let made = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
        .canonicalize()
        .expect("created");
    assert!(made.starts_with(&temp), "host command created {made:?}");
    // Disclosed limit: the system binary named absolutely is not rerouted,
    // and it is refused rather than given the shared directory.
    let output = run_in(&plan, &fx.seat_a, "/usr/bin/mktemp -d");
    assert!(
        !output.status.success(),
        "absolute /usr/bin/mktemp: {output:?}"
    );
}

/// A background-only application opened from the writable tree is started by
/// launchd, outside the boundary. The boundary refuses the open, so it can
/// never read what the policy denies, while ordinary tools keep working.
#[test]
fn launch_services_cannot_start_a_program_outside_the_boundary() {
    let fx = fixture();
    let plan = prepared(prepare(&inputs(&fx, "s1", &fx.seat_a, &[], &[])));
    // Evidence the escaped program would leave: a directory the test owns,
    // outside every grant.
    let outside = fx.root.join("escape-evidence");
    std::fs::create_dir_all(&outside).expect("evidence dir");
    let app = fx.seat_a.join("Probe.app");
    let macos = app.join("Contents/MacOS");
    std::fs::create_dir_all(&macos).expect("bundle");
    std::fs::write(
        app.join("Contents/Info.plist"),
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<plist version=\"1.0\"><dict>",
            "<key>CFBundleExecutable</key><string>probe</string>",
            "<key>CFBundleIdentifier</key><string>invalid.beekeeper.boundary-probe</string>",
            "<key>CFBundlePackageType</key><string>APPL</string>",
            "<key>LSBackgroundOnly</key><true/>",
            "</dict></plist>\n"
        ),
    )
    .expect("plist");
    let probe = macos.join("probe");
    std::fs::write(
        &probe,
        format!(
            "#!/bin/sh\ncat '{}' > '{}/read' 2>&1\n",
            fx.b_plan.display(),
            outside.display()
        ),
    )
    .expect("probe");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)).expect("mode");
    }
    let output = run_in(
        &plan,
        &fx.seat_a,
        &format!("/usr/bin/open -g -j '{}'", app.display()),
    );
    std::thread::sleep(std::time::Duration::from_secs(3));
    let _ = std::process::Command::new(
        "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister",
    )
    .args(["-u", &app.display().to_string()])
    .output();
    let escaped = std::fs::read_to_string(outside.join("read")).unwrap_or_default();
    assert!(
        !escaped.contains("B_PLAN_CANARY"),
        "an opened application read a denied file: {output:?}"
    );
    assert!(!output.status.success(), "the open is refused: {output:?}");
    // Ordinary tools still run with the delegation denials in place.
    let output = run_in(&plan, &fx.seat_a, "git status --short >/dev/null && python3 -c 'print(1)' && /usr/bin/osascript -e 'return 1'");
    assert!(output.status.success(), "ordinary tools: {output:?}");
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Run11 (ledger 272, defect 1). The host pins the worktree's branch when it
/// first prepares it; the seat commits and pushes that branch without
/// touching Git metadata, the briefing names it, a differently named remote
/// branch is a push target, and creating a local branch stays refused.
#[test]
fn a_seat_commits_and_pushes_its_pinned_branch_and_nothing_else() {
    let fx = fixture();
    // A disposable bare remote standing in for the relay. It sits in the
    // seat's own tree only so the fixture needs no network server; what is
    // under test is the seat's side of the push (its refs and objects).
    let remote = fx.seat_a.join(".probe/remote.git");
    std::fs::create_dir_all(&remote).expect("remote dir");
    super::tests::git(&remote, &["init", "-q", "--bare"]);
    super::tests::git(
        &fx.seat_a,
        &["remote", "add", "origin", remote.to_str().expect("utf8")],
    );
    let plan = prepared(prepare(&inputs(&fx, "s1", &fx.seat_a, &[], &[])));
    assert_eq!(plan.binding.branch.as_deref(), Some("seat-branch"));
    let briefing = crate::session::boundary_briefing(&ExecutionPlan::Prepared(plan.clone()));
    assert!(
        briefing.contains("on the branch `seat-branch`")
            && briefing.contains("git push origin seat-branch"),
        "{briefing}"
    );
    let output = run_in(
        &plan,
        &fx.seat_a,
        "echo work >> README.md && git -c user.name=s -c user.email=s@example.invalid commit -qam work 2>&1",
    );
    assert!(
        output.status.success(),
        "commit on the pinned branch: {output:?}"
    );
    assert!(
        output.stdout.is_empty() && output.stderr.is_empty(),
        "a commit leaves no refused writes behind (run11 builder seq87): {output:?}"
    );
    for push in [
        "git push -q origin seat-branch",
        "git push -q origin HEAD:refs/heads/work/lapbook-cli",
    ] {
        let output = run_in(&plan, &fx.seat_a, &format!("{push} 2>&1"));
        assert!(output.status.success(), "{push}: {output:?}");
    }
    let pushed = git_out(&remote, &["for-each-ref", "--format=%(refname)"]);
    assert!(
        pushed.contains("refs/heads/seat-branch") && pushed.contains("refs/heads/work/lapbook-cli"),
        "{pushed}"
    );
    let output = run_in(&plan, &fx.seat_a, "git checkout -b work/lapbook-cli 2>&1");
    assert!(
        !output.status.success(),
        "a new local branch is refused: {output:?}"
    );
    assert_eq!(
        git_out(&fx.seat_a, &["branch", "--show-current"]),
        "seat-branch"
    );
}

/// The grant follows the pinned branch, not `HEAD`: a seat that re-points its
/// own `HEAD` at another branch does not gain that branch's ref on the next
/// preparation (a continuation or restart).
#[test]
fn re_pointing_head_does_not_widen_the_branch_grant_on_continuation() {
    let fx = fixture();
    let first = prepared(prepare(&inputs(&fx, "s1", &fx.seat_a, &[], &[])));
    let main_before = git_out(&fx.repo_a, &["rev-parse", "refs/heads/main"]);
    // What a child can do: its worktree administration is writable.
    let output = run_in(&first, &fx.seat_a, "git symbolic-ref HEAD refs/heads/main");
    assert!(output.status.success(), "{output:?}");

    let mut again = inputs(&fx, "s1", &fx.seat_a, &[], &[]);
    again.prior = Some((Some(&first.binding), None));
    let plan = prepared(prepare(&again));
    assert_eq!(plan.binding.branch.as_deref(), Some("seat-branch"));
    let output = run_in(
        &plan,
        &fx.seat_a,
        "echo x >> README.md && git -c user.name=s -c user.email=s@example.invalid commit -qam x",
    );
    assert!(!output.status.success(), "main is not writable: {output:?}");
    assert_eq!(
        git_out(&fx.repo_a, &["rev-parse", "refs/heads/main"]),
        main_before,
        "the shared main branch is unchanged"
    );
}

/// Host-side Git outside a boundary must run none of the repository's own
/// programs. `status` runs a configured clean filter on a dirty tree even
/// with hooks and fsmonitor off, so the unbounded dirty probe reads stat
/// information only.
#[test]
fn the_unbounded_dirty_probe_runs_no_repository_filter() {
    let fx = fixture();
    let marker = fx.root.join("filter-ran");
    let filter = fx.root.join("filter.sh");
    std::fs::write(
        &filter,
        format!("#!/bin/sh\ntouch '{}'\ncat\n", marker.display()),
    )
    .expect("filter");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&filter, std::fs::Permissions::from_mode(0o755)).expect("mode");
    }
    super::tests::git(
        &fx.repo_a,
        &["config", "filter.x.clean", filter.to_str().expect("utf8")],
    );
    std::fs::write(fx.repo_a.join(".gitattributes"), "* filter=x\n").expect("attributes");
    // Same size, new content: only hashing (through the filter) tells.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(fx.repo_a.join("README.md"), "B_OWN\n").expect("dirty");
    let _ = std::fs::remove_file(&marker);

    assert_eq!(crate::host_command::stat_only_dirty(&fx.repo_a), Some(true));
    assert!(!marker.exists(), "the stat-only probe ran the clean filter");

    // Why `status` is not used here: it runs the filter despite the overrides.
    let _ = crate::host_command::metadata_git_command(&fx.repo_a)
        .args(["--no-optional-locks", "status", "--porcelain"])
        .output();
    assert!(
        marker.exists(),
        "control: status runs the repository's filter"
    );

    // A clean tree reads clean.
    let _ = std::fs::remove_file(&marker);
    let clean = fx.root.join("repos/clean");
    std::fs::create_dir_all(&clean).expect("clean");
    super::tests::git(&clean, &["init", "-q", "-b", "main"]);
    std::fs::write(clean.join("f"), "x\n").expect("f");
    super::tests::git(&clean, &["add", "."]);
    super::tests::git(&clean, &["commit", "-q", "-m", "i"]);
    assert_eq!(crate::host_command::stat_only_dirty(&clean), Some(false));
}

/// An operator-level value meant for project A (here in the host's own
/// environment) never reaches another project's session, its build
/// children, or its host command by ambient fallback. A host command gets it
/// only when that project's own action definition names it — an explicit,
/// disclosed passthrough, not a project binding.
#[test]
fn an_a_only_value_never_reaches_b_by_ambient_fallback() {
    use std::ffi::OsString;
    let fx = fixture();
    let ambient: Vec<(OsString, OsString)> = std::env::vars_os()
        .chain([(
            OsString::from("A_ONLY_TOKEN"),
            OsString::from("A_ONLY_CANARY"),
        )])
        .collect();
    let probe = "env; sh -c env; python3 -c 'import os; print(os.environ.get(\"A_ONLY_TOKEN\"))'";

    let mut session = inputs(&fx, "b-session", &fx.seat_a, &[], &[]);
    session.project_ref = Some("30621:aa:project-b");
    session.ambient_env = Some(&ambient);
    let plan = prepared(prepare(&session));
    let output = run_in(&plan, &fx.seat_a, probe);
    assert!(output.status.success(), "{output:?}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("A_ONLY_CANARY"),
        "session B or its children saw A's value: {output:?}"
    );

    let mut host = inputs(&fx, "b-host", &fx.seat_a, &[], &[]);
    host.purpose = ScopePurpose::HostCommand;
    host.project_ref = Some("30621:aa:project-b");
    host.ambient_env = Some(&ambient);
    let plan = prepared(prepare(&host));
    let output = run_in(&plan, &fx.seat_a, probe);
    assert!(output.status.success(), "{output:?}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("A_ONLY_CANARY"),
        "B's host command saw A's value without naming it: {output:?}"
    );

    // Control: a definition that names it receives it, as declared.
    let declared = [(
        "A_ONLY_TOKEN".to_owned(),
        "A_ONLY_CANARY".to_owned(),
        buzz_acp::exec_env::EnvSource::ProjectFromHost,
    )];
    host.project_env = &declared;
    let plan = prepared(prepare(&host));
    let output = run_in(&plan, &fx.seat_a, "printenv A_ONLY_TOKEN");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "A_ONLY_CANARY"
    );
}

/// Run11 (ledger 272, defect 3): the lead had no agents checkout and cloned
/// its own. A lead granted `write` gets its clone prepared before the first
/// turn and, inside the boundary, reads the project's requirements, drafts a
/// plan, commits and pushes it, and sees the new commit — with no clone of
/// its own to improvise. The binding (which the card and brief read) says so.
#[test]
fn a_writing_lead_drafts_commits_and_pushes_its_plan_in_the_prepared_clone() {
    let fx = fixture();
    let clone = fx.root.join("repos/a-wt-seat-agents");
    std::fs::create_dir_all(clone.join("plans")).expect("clone");
    super::tests::git(&clone, &["init", "-q", "-b", "main"]);
    std::fs::write(clone.join("plans/requirements.md"), "A_REQUIREMENTS\n").expect("reqs");
    super::tests::git(&clone, &["add", "."]);
    super::tests::git(&clone, &["commit", "-q", "-m", "requirements"]);
    // A disposable remote standing in for the relay's agents repository.
    let remote = clone.join(".relay.git");
    super::tests::git(
        &clone,
        &["init", "-q", "--bare", remote.to_str().expect("utf8")],
    );
    super::tests::git(
        &clone,
        &["remote", "add", "origin", remote.to_str().expect("utf8")],
    );
    super::tests::git(&clone, &["push", "-q", "origin", "main"]);

    let mut facts = inputs(&fx, "lead", &fx.seat_a, &[], &[]);
    facts.agents_checkout = Some((&clone, true));
    let plan = prepared(prepare(&facts));
    assert_eq!(plan.binding.agents.as_deref(), Some(clone.as_path()));
    assert!(
        plan.binding.agents_writable,
        "the binding records the write grant"
    );

    let script = format!(
        "cd '{c}' && cat plans/requirements.md \
         && printf 'schema: beekeeper-plan/v1\\n' > plans/lapbook.md \
         && git add plans/lapbook.md \
         && git -c user.name=l -c user.email=l@example.invalid commit -qm 'plan: lapbook' 2>&1 \
         && git push -q origin main 2>&1 \
         && git fetch -q origin 2>&1 && git rev-parse HEAD origin/main",
        c = clone.display()
    );
    let output = run_in(&plan, &fx.seat_a, &script);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("A_REQUIREMENTS"), "{stdout}");
    let heads: Vec<&str> = stdout.lines().rev().take(2).collect();
    assert_eq!(
        heads[0], heads[1],
        "the pushed plan commit is what the clone sees: {stdout}"
    );
    assert!(!stdout.contains("Operation not permitted"), "{stdout}");
}
