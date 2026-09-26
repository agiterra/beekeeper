use super::fast_forward_checkout;
use crate::commands::project_git::compare_local_remote_status;
use crate::commands::project_git_exec::{build_test_git_auth_config, run_git};

/// A pull's fast-forward writes the working tree, so the filter the
/// checkout's own configuration names runs — inside the checkout's
/// boundary. The control clone, fast-forwarded by plain host Git, shows
/// the same filter reading outside the project; the pull's does not, and
/// still writes the project's bytes.
#[test]
fn a_pull_fast_forwards_with_the_projects_filter_inside_its_boundary() {
    let auth = build_test_git_auth_config().expect("auth");
    let root = tempfile::tempdir().expect("tempdir");
    let root = root.path().canonicalize().expect("canonical");
    let foreign = root.join("elsewhere/secret.txt");
    std::fs::create_dir_all(foreign.parent().expect("parent")).expect("elsewhere");
    std::fs::write(&foreign, "NOT_THIS_PROJECTS\n").expect("foreign");
    let remote = root.join("remote.git");
    let remote_path = remote.to_str().expect("remote path");
    let upstream = root.join("upstream");
    let commit = |dir: &std::path::Path, message: &str| {
        run_git(
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@example.invalid",
                "commit",
                "-q",
                "-m",
                message,
            ],
            Some(dir),
            &auth,
        )
        .expect("commit");
    };
    run_git(
        &[
            "init",
            "-q",
            "--bare",
            "--initial-branch=main",
            "--",
            remote_path,
        ],
        None,
        &auth,
    )
    .expect("remote");
    run_git(
        &[
            "clone",
            "-q",
            "--",
            remote_path,
            upstream.to_str().expect("upstream"),
        ],
        None,
        &auth,
    )
    .expect("upstream");
    run_git(&["checkout", "-q", "-B", "main"], Some(&upstream), &auth).expect("main");
    std::fs::write(upstream.join(".gitattributes"), "*.txt filter=canary\n").expect("attributes");
    std::fs::write(upstream.join("notes.txt"), "own notes\n").expect("notes");
    run_git(&["add", "."], Some(&upstream), &auth).expect("add");
    commit(&upstream, "first");
    run_git(&["push", "-q", "origin", "main"], Some(&upstream), &auth).expect("push first");

    let smudge = format!(
        "if cat '{}' >/dev/null 2>&1; then printf FOREIGN_READ:; fi; sed s/^/smudged:/",
        foreign.display()
    );
    let clone = |name: &str| {
        let dir = root.join(name);
        run_git(
            &["clone", "-q", "--", remote_path, dir.to_str().expect("dir")],
            None,
            &auth,
        )
        .expect("clone");
        for (key, value) in [
            ("filter.canary.smudge", smudge.as_str()),
            ("filter.canary.clean", "sed s/^smudged://"),
            ("filter.canary.required", "true"),
        ] {
            run_git(&["config", key, value], Some(&dir), &auth).expect("filter config");
        }
        dir
    };
    let checkout = clone("checkout");
    let control = clone("control");

    std::fs::write(upstream.join("notes.txt"), "second\n").expect("second");
    run_git(&["add", "notes.txt"], Some(&upstream), &auth).expect("add second");
    commit(&upstream, "second");
    run_git(&["push", "-q", "origin", "main"], Some(&upstream), &auth).expect("push second");
    let head = run_git(&["rev-parse", "HEAD"], Some(&upstream), &auth).expect("head");

    run_git(
        &["pull", "-q", "--ff-only", "origin", "main"],
        Some(&control),
        &auth,
    )
    .expect("control pull");
    assert_eq!(
        std::fs::read_to_string(control.join("notes.txt")).expect("control"),
        "FOREIGN_READ:smudged:second\n",
        "the fixture's filter must be live and able to read outside, or this test proves nothing"
    );

    let status =
        compare_local_remote_status(&checkout, "origin", remote_path, Some("main"), None, &auth);
    assert!(status.can_pull, "{:?}", status.pull_block_reason);
    fast_forward_checkout(&checkout, "origin", "main", &auth).expect("fast-forward");
    assert_eq!(
        std::fs::read_to_string(checkout.join("notes.txt")).expect("notes"),
        "smudged:second\n",
        "the project's filter wrote the project's bytes, and read nothing outside the project"
    );
    assert_eq!(
        run_git(&["rev-parse", "HEAD"], Some(&checkout), &auth)
            .expect("HEAD")
            .trim(),
        head.trim()
    );
}
