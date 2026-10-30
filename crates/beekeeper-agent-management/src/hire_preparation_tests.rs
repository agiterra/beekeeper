use super::*;
use serde_json::json;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Result<String, Error> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_CONFIG_COUNT")
        .output()
        .map_err(fail)?;
    if !out.status.success() {
        return Err(fail(String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().into())
}

fn exercise(mode: &str) {
    let clone = mode != "code";
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("source");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-b", "main"]).unwrap();
    git(&root, &["config", "user.name", "Fixture"]).unwrap();
    git(&root, &["config", "user.email", "fixture@example.invalid"]).unwrap();
    std::fs::write(root.join("marker"), "pinned material").unwrap();
    git(&root, &["add", "marker"]).unwrap();
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "fixture"],
    )
    .unwrap();
    let revision = git(&root, &["rev-parse", "HEAD"]).unwrap();
    let destination = dir.path().join("reserved");
    let journal = dir.path().join("preparation.json");
    let binding = json!({"id":"fixed-create","destination":destination,"revision":revision});
    let create = if clone {
        vec![
            "clone",
            "--no-checkout",
            "--",
            root.to_str().unwrap(),
            destination.to_str().unwrap(),
        ]
    } else {
        vec![
            "worktree",
            "add",
            "--no-checkout",
            "-b",
            "fixed-create",
            "--",
            destination.to_str().unwrap(),
            &revision,
        ]
    };
    let checkout = match mode {
        "code" => vec!["checkout", "--force", "fixed-create"],
        "agents-write" => vec!["checkout", "--force", "-B", "main", &revision],
        _ => vec!["checkout", "--force", "--detach", &revision],
    };
    let first = prepare(
        &journal,
        binding.clone(),
        &destination,
        &root,
        &create,
        &checkout,
        &revision,
        |p, args| {
            let result = git(p, args)?;
            if args == create {
                return Err(Error::Other(
                    "crash after Git create before checkout".into(),
                ));
            }
            Ok(result)
        },
        |_| Ok(()),
    );
    assert!(first.is_err());
    assert!(destination.exists());
    assert!(!destination.join("marker").exists());
    assert!(prepare(
        &journal,
        binding.clone(),
        &destination,
        &root,
        &create,
        &checkout,
        &revision,
        git,
        |_| Ok(())
    )
    .unwrap());
    assert_eq!(
        std::fs::read_to_string(destination.join("marker")).unwrap(),
        "pinned material"
    );
    assert!(!prepare(
        &journal,
        binding.clone(),
        &destination,
        &root,
        &create,
        &checkout,
        &revision,
        git,
        |_| Ok(())
    )
    .unwrap());
    std::fs::write(destination.join("marker"), "possibly executed work").unwrap();
    assert!(prepare(
        &journal,
        binding,
        &destination,
        &root,
        &create,
        &checkout,
        &revision,
        git,
        |_| Ok(())
    )
    .is_err());
    assert_eq!(
        std::fs::read_to_string(destination.join("marker")).unwrap(),
        "possibly executed work"
    );
}

#[test]
fn code_git_step_crash_resumes_without_resetting_completed_work() {
    exercise("code");
}
#[test]
fn agents_read_git_step_crash_resumes_without_resetting_completed_work() {
    exercise("agents-read");
}
#[test]
fn agents_write_git_step_crash_resumes_without_resetting_completed_work() {
    exercise("agents-write");
}

#[test]
fn unknown_existing_tree_is_never_reset_or_claimed() {
    let dir = tempfile::tempdir().unwrap();
    let tree = dir.path().join("possibly-executed");
    std::fs::create_dir(&tree).unwrap();
    std::fs::write(tree.join("work"), "preserve me").unwrap();
    let journal = dir.path().join("intent.json");
    let result = prepare(
        &journal,
        json!({"create":"fixed"}),
        &tree,
        dir.path(),
        &[],
        &[],
        "pinned",
        |_, _| panic!("unknown existing tree must receive no Git operation"),
        |_| Ok(()),
    );
    assert!(matches!(result, Err(Error::Conflict(_))));
    assert!(!journal.exists());
    assert_eq!(
        std::fs::read_to_string(tree.join("work")).unwrap(),
        "preserve me"
    );
}
