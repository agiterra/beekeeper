use super::*;

fn canonical_tempdir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().canonicalize().expect("canonical tempdir");
    (dir, path)
}

#[test]
fn the_policy_denies_everything_then_grants_in_order() {
    let grants = vec![
        Grant::file("/state/home/auth.json", Access::NoUnlink, "guarded link"),
        Grant::tree("/work/a", Access::ReadWrite, "own checkout"),
        Grant::file("/tools/cli", Access::ReadOnly, "runtime"),
        Grant::scratch("/tmp/xcrun", "xcrun_db", Access::ReadWrite, "tool cache"),
    ];
    let policy = render_policy(&grants).expect("renders");
    let deny = policy
        .find("(deny file-read* file-write* (subpath \"/\"))")
        .expect("complete deny");
    let own = policy
        .find("(allow file-read* file-write* (subpath \"/work/a\"))")
        .expect("own grant");
    assert!(
        deny < own,
        "grants must follow the complete deny:\n{policy}"
    );
    assert!(policy.contains("(allow file-read* (literal \"/tools/cli\"))"));
    assert!(policy.contains("(regex #\"^/tmp/xcrun/(.*/)?xcrun_db[^/]*$\")"));
    let deny = policy
        .rfind("(deny file-write-unlink (literal \"/state/home/auth.json\"))")
        .expect("unlink guard");
    assert!(deny > own, "a denial is rendered after every allow");
    assert!(
        !policy.contains("/Users"),
        "no home region is named: the deny is complete, not a list of regions"
    );
}

#[test]
fn a_grant_the_policy_cannot_express_is_refused() {
    for bad in [
        Grant::tree("relative/path", Access::ReadOnly, "relative"),
        Grant::tree("/", Access::ReadOnly, "root"),
        Grant::tree("/a\"b", Access::ReadOnly, "quote"),
        Grant::tree("/a\\b", Access::ReadOnly, "backslash"),
        Grant::file("/a\nb", Access::ReadOnly, "newline"),
        Grant::scratch("/a", "", Access::ReadWrite, "no prefix"),
        Grant::scratch("/a", "x/y", Access::ReadWrite, "a directory prefix"),
    ] {
        assert!(
            matches!(
                render_policy(std::slice::from_ref(&bad)),
                Err(BoundaryError::InvalidSpec(_))
            ),
            "{bad:?} was accepted"
        );
    }
}

#[test]
fn a_project_under_a_system_root_cannot_be_bounded() {
    assert_eq!(
        overlaps_system_root(Path::new("/opt/homebrew/src/project")),
        Some("/opt/homebrew")
    );
    assert_eq!(
        overlaps_system_root(Path::new("/usr/local/src/x")),
        Some("/usr")
    );
    assert_eq!(overlaps_system_root(Path::new("/Users/me/project")), None);
    // A grant *above* a system root would expose it too.
    assert_eq!(
        overlaps_system_root(Path::new("/opt")),
        Some("/opt/homebrew")
    );
}

#[test]
fn the_policy_may_not_live_inside_a_grant() {
    let (_dir, root) = canonical_tempdir();
    let own = root.join("own");
    std::fs::create_dir_all(&own).expect("own");
    std::fs::write(own.join("f"), "x").expect("file");
    let spec = BoundarySpec {
        grants: vec![Grant::tree(&own, Access::ReadWrite, "own checkout")],
        policy_dir: own.join("policies"),
        probe_readable: own.join("f"),
    };
    assert!(matches!(prepare(spec), Err(BoundaryError::InvalidSpec(_))));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn no_backend_is_claimed_off_macos() {
    let spec = BoundarySpec {
        grants: Vec::new(),
        policy_dir: PathBuf::from("/tmp/policies"),
        probe_readable: PathBuf::from("/etc/hosts"),
    };
    assert!(matches!(prepare(spec), Err(BoundaryError::Unsupported(_))));
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::io::{BufRead, Write};
    use std::process::{Command, Stdio};

    /// Two projects A and B side by side, a third outside the parent, and a
    /// boundary granting only A.
    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        a: PathBuf,
        b: PathBuf,
        other_root: PathBuf,
        boundary: PreparedBoundary,
    }

    fn fixture() -> Fixture {
        let (dir, root) = canonical_tempdir();
        let a = root.join("projects/a");
        let b = root.join("projects/b");
        let other_root = root.join("elsewhere/c");
        for d in [&a, &b, &other_root] {
            std::fs::create_dir_all(d).expect("dir");
        }
        std::fs::write(a.join("own.txt"), "A_OWN\n").expect("own");
        std::fs::write(b.join("plan.md"), "B_PLAN_CANARY\n").expect("b");
        std::fs::write(other_root.join("plan.md"), "C_PLAN_CANARY\n").expect("c");
        std::os::unix::fs::symlink(b.join("plan.md"), a.join("link-to-b")).expect("symlink");
        let boundary = prepare(BoundarySpec {
            grants: vec![Grant::tree(&a, Access::ReadWrite, "own checkout")],
            policy_dir: root.join("host"),
            probe_readable: a.join("own.txt"),
        })
        .expect("boundary prepared and self-tested");
        Fixture {
            _dir: dir,
            root,
            a,
            b,
            other_root,
            boundary,
        }
    }

    fn run(fixture: &Fixture, program: &str, args: &[&str]) -> std::process::Output {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (program, argv) = fixture.boundary.wrap(program, &args);
        Command::new(program)
            .args(argv)
            .current_dir(&fixture.a)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .expect("spawn")
    }

    fn refused(output: &std::process::Output) -> bool {
        !output.status.success()
            && String::from_utf8_lossy(&output.stderr).contains("Operation not permitted")
    }

    #[test]
    fn own_files_are_readable_and_writable() {
        let fx = fixture();
        let read = run(&fx, "/bin/cat", &["own.txt"]);
        assert!(read.status.success(), "{read:?}");
        assert_eq!(String::from_utf8_lossy(&read.stdout), "A_OWN\n");
        let write = run(
            &fx,
            "/bin/sh",
            &["-c", "echo new > made.txt && cat made.txt"],
        );
        assert!(write.status.success(), "{write:?}");
    }

    #[test]
    fn every_spelling_of_a_sibling_is_refused_for_the_boundary_reason() {
        let fx = fixture();
        let sibling = fx.b.join("plan.md");
        let sibling = sibling.to_string_lossy();
        for (label, program, args) in [
            ("absolute", "/bin/cat", vec![sibling.as_ref()]),
            ("relative", "/bin/cat", vec!["../b/plan.md"]),
            ("symlink", "/bin/cat", vec!["link-to-b"]),
            ("shell child", "/bin/sh", vec!["-c", "cat ../b/plan.md"]),
            (
                "python child",
                "/usr/bin/python3",
                vec![
                    "-c",
                    "import sys; print(open(sys.argv[1]).read())",
                    sibling.as_ref(),
                ],
            ),
            ("listing", "/bin/ls", vec!["../"]),
        ] {
            let output = run(&fx, program, &args);
            assert!(
                !String::from_utf8_lossy(&output.stdout).contains("B_PLAN_CANARY"),
                "{label}: the sibling canary was read"
            );
            assert!(
                refused(&output)
                    || String::from_utf8_lossy(&output.stderr).contains("PermissionError"),
                "{label}: not refused by the boundary: {output:?}"
            );
        }
    }

    #[test]
    fn a_root_outside_the_projects_parent_is_refused_too() {
        let fx = fixture();
        let path = fx.other_root.join("plan.md");
        let output = run(&fx, "/bin/cat", &[path.to_string_lossy().as_ref()]);
        assert!(refused(&output), "{output:?}");
    }

    #[test]
    fn sibling_writes_and_policy_writes_are_refused() {
        let fx = fixture();
        let target = fx.b.join("plan.md");
        let output = run(
            &fx,
            "/bin/sh",
            &["-c", &format!("echo pwned > '{}'", target.display())],
        );
        assert!(!output.status.success());
        assert_eq!(
            std::fs::read_to_string(&target).expect("read"),
            "B_PLAN_CANARY\n"
        );
        let policy = fx.root.join(format!("host/{}.sb", fx.boundary.digest()));
        // The owner could make its own read-only file writable; only the
        // boundary refuses that.
        let output = run(
            &fx,
            "/bin/sh",
            &[
                "-c",
                &format!(
                    "chmod u+w '{p}'; echo '(allow default)' >> '{p}'",
                    p = policy.display()
                ),
            ],
        );
        assert!(!output.status.success());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&policy)
                .expect("policy")
                .permissions()
                .mode()
                & 0o777,
            0o400,
            "the policy's mode was changed from inside the boundary"
        );
        assert!(!std::fs::read_to_string(&policy)
            .expect("policy")
            .ends_with("(allow default)\n"));
    }

    /// Astra's audit note 1: one confined process, alive and ready, is asked
    /// to read a sibling created *after* it started.
    #[test]
    fn a_sibling_created_after_the_process_started_is_refused_to_that_process() {
        let fx = fixture();
        let script = "echo READY; while IFS= read -r path; do if cat \"$path\"; then echo READ; else echo REFUSED; fi; done";
        let (program, argv) = fx
            .boundary
            .wrap("/bin/sh", &["-c".to_owned(), script.to_owned()]);
        let mut child = Command::new(program)
            .args(argv)
            .current_dir(&fx.a)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn");
        let mut stdout = std::io::BufReader::new(child.stdout.take().expect("stdout"));
        let mut line = String::new();
        stdout.read_line(&mut line).expect("ready");
        assert_eq!(line.trim(), "READY");

        let late = fx.root.join("projects/late");
        std::fs::create_dir_all(&late).expect("late");
        std::fs::write(late.join("plan.md"), "LATE_CANARY\n").expect("late plan");
        let mut stdin = child.stdin.take().expect("stdin");
        writeln!(stdin, "{}", late.join("plan.md").display()).expect("ask");
        writeln!(stdin, "{}", fx.a.join("own.txt").display()).expect("ask own");
        drop(stdin);
        let mut rest = String::new();
        std::io::Read::read_to_string(&mut stdout, &mut rest).expect("answers");
        let _ = child.wait();
        assert!(!rest.contains("LATE_CANARY"), "{rest}");
        let answers: Vec<&str> = rest
            .lines()
            .filter(|l| *l == "READ" || *l == "REFUSED")
            .collect();
        assert_eq!(answers, ["REFUSED", "READ"], "{rest}");
    }

    #[test]
    fn the_self_test_refuses_a_grant_that_opens_the_canary() {
        let (_dir, root) = canonical_tempdir();
        let own = root.join("own");
        std::fs::create_dir_all(&own).expect("own");
        std::fs::write(own.join("f"), "x").expect("f");
        let host = root.join("host");
        // Neither a scratch pattern matching the canary's name nor a grant
        // naming the canary file is an ancestor of the policy directory; only
        // the probe can catch them.
        let too_wide = BoundarySpec {
            grants: vec![
                Grant::tree(&own, Access::ReadOnly, "own"),
                Grant::scratch(&host, "boundary-can", Access::ReadOnly, "too wide"),
            ],
            policy_dir: host.clone(),
            probe_readable: own.join("f"),
        };
        assert!(
            matches!(prepare(too_wide), Err(BoundaryError::SelfTestFailed(_))),
            "a scratch pattern that matches the canary's name is caught by the probe"
        );
        let spec = BoundarySpec {
            grants: vec![
                Grant::tree(&own, Access::ReadOnly, "own"),
                Grant::file(
                    host.join("boundary-canary"),
                    Access::ReadOnly,
                    "names the canary",
                ),
            ],
            policy_dir: host,
            probe_readable: own.join("f"),
        };
        assert!(
            matches!(prepare(spec), Err(BoundaryError::SelfTestFailed(_))),
            "a boundary that exposes the canary must not be returned"
        );
    }

    #[test]
    fn a_foreign_file_cannot_be_aliased_into_the_own_tree() {
        let fx = fixture();
        let foreign = fx.b.join("plan.md");
        let foreign = foreign.to_string_lossy();
        for (label, script) in [
            (
                "hard link",
                format!("ln '{foreign}' ./alias && cat ./alias"),
            ),
            ("clone", format!("cp -c '{foreign}' ./alias && cat ./alias")),
            (
                "rename in",
                format!("mv '{foreign}' ./alias && cat ./alias"),
            ),
        ] {
            let output = run(&fx, "/bin/sh", &["-c", &script]);
            assert!(!output.status.success(), "{label}: {output:?}");
            assert!(!String::from_utf8_lossy(&output.stdout).contains("B_PLAN_CANARY"));
        }
        assert!(
            fx.b.join("plan.md").exists(),
            "the foreign file was not moved"
        );
    }

    #[test]
    fn an_unlink_guard_allows_writing_through_but_not_replacing() {
        let (_dir, root) = canonical_tempdir();
        let own = root.join("own");
        std::fs::create_dir_all(&own).expect("own");
        let target = root.join("elsewhere/login.json");
        std::fs::create_dir_all(target.parent().expect("parent")).expect("dir");
        std::fs::write(&target, "v1\n").expect("target");
        let link = own.join("login.json");
        std::os::unix::fs::symlink(&target, &link).expect("link");
        let boundary = prepare(BoundarySpec {
            grants: vec![
                Grant::tree(&own, Access::ReadWrite, "own"),
                Grant::file(&target, Access::ReadWrite, "the one login file"),
                Grant::file(&link, Access::NoUnlink, "the link to it"),
            ],
            policy_dir: root.join("host"),
            probe_readable: target.clone(),
        })
        .expect("prepared");
        let sh = |script: &str| {
            let (program, argv) = boundary.wrap("/bin/sh", &["-c".to_owned(), script.to_owned()]);
            Command::new(program)
                .args(argv)
                .current_dir(&own)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .output()
                .expect("spawn")
        };
        assert!(sh("printf 'v2\\n' > login.json").status.success());
        assert_eq!(
            std::fs::read_to_string(&target).expect("t"),
            "v2\n",
            "written through"
        );
        assert!(!sh("rm -f login.json").status.success());
        assert!(!sh("echo v3 > next && mv next login.json").status.success());
        assert!(std::fs::symlink_metadata(&link)
            .expect("link")
            .file_type()
            .is_symlink());
    }

    /// Two scopes prepared at once into one policy directory each get their
    /// own immutable file, and each launch enforces its own grants.
    #[test]
    fn concurrent_preparations_never_share_or_rewrite_a_policy() {
        let (_dir, root) = canonical_tempdir();
        let host = root.join("host");
        let mut handles = Vec::new();
        for name in ["a", "b", "a", "b"] {
            let root = root.clone();
            let host = host.clone();
            handles.push(std::thread::spawn(move || {
                let own = root.join(name);
                std::fs::create_dir_all(&own).expect("own");
                std::fs::write(own.join("f"), name).expect("f");
                prepare(BoundarySpec {
                    grants: vec![Grant::tree(&own, Access::ReadWrite, "own")],
                    policy_dir: host,
                    probe_readable: own.join("f"),
                })
                .expect("prepared")
            }));
        }
        let prepared: Vec<PreparedBoundary> = handles
            .into_iter()
            .map(|h| h.join().expect("thread"))
            .collect();
        assert_eq!(prepared[0].digest(), prepared[2].digest());
        assert_ne!(prepared[0].digest(), prepared[1].digest());
        let read = |boundary: &PreparedBoundary, name: &str| {
            let path = root.join(name).join("f");
            let (program, argv) = boundary.wrap("/bin/cat", &[path.to_string_lossy().into_owned()]);
            Command::new(program)
                .args(argv)
                .output()
                .expect("spawn")
                .status
                .success()
        };
        assert!(read(&prepared[0], "a") && !read(&prepared[0], "b"));
        assert!(read(&prepared[1], "b") && !read(&prepared[1], "a"));
        let files: Vec<_> = std::fs::read_dir(&host)
            .expect("host")
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "sb"))
            .collect();
        assert_eq!(files.len(), 2, "one immutable policy per distinct scope");
    }
}
