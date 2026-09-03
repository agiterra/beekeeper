//! `bee packs` argument handling — the parts that decide without a relay.

use super::*;

const OWNER: &str = "6cbdf4451d3989c10c20d13240c665a9e11e3959a95488382193481692b68df2";

/// Exactly one pin, and the refusal says which flags to use.
#[test]
fn set_source_requires_exactly_one_pin() {
    let both = PackSourcePin {
        ref_name: Some("refs/heads/main"),
        sha: Some(&"a".repeat(40)),
    };
    let error = both.resolve().expect_err("both pins refused");
    assert!(format!("{error}").contains("exactly one"), "{error}");

    let neither = PackSourcePin {
        ref_name: None,
        sha: None,
    };
    let error = neither.resolve().expect_err("no pin refused");
    assert!(format!("{error}").contains("exactly one"), "{error}");

    assert_eq!(
        PackSourcePin {
            ref_name: Some("refs/heads/main"),
            sha: None,
        }
        .resolve()
        .expect("a ref"),
        PackPin::Ref("refs/heads/main".to_string())
    );
    assert_eq!(
        PackSourcePin {
            ref_name: None,
            sha: Some(&"A".repeat(40)),
        }
        .resolve()
        .expect("a sha"),
        PackPin::Sha("A".repeat(40)),
        "the pin is normalized by the builder, not here"
    );
}

/// A `--project` that is not a project coordinate is refused at exit 1, with
/// the shape it wanted printed back.
#[test]
fn a_project_argument_must_be_a_project_coordinate() {
    let error = normalize_project("agiterra").expect_err("refused");
    assert!(
        format!("{error}").contains("30621:<64-hex>:<slug>"),
        "{error}"
    );
    let error = normalize_project(&format!("30617:{OWNER}:repo")).expect_err("refused");
    assert!(format!("{error}").contains("30621"), "{error}");
    assert_eq!(
        normalize_project(&format!("  30621:{}:agiterra ", OWNER.to_ascii_uppercase()))
            .expect("normalizes"),
        format!("30621:{OWNER}:agiterra"),
        "the coordinate is case-folded and trimmed like every other reader's"
    );
}

/// The role directory probe reads a real directory and stays empty — never
/// erroring — for a path this machine has never fetched.
#[test]
fn role_directories_reads_what_is_there_and_nothing_when_it_is_not() {
    let root = std::env::temp_dir().join(format!("bee-packs-{}", uuid::Uuid::new_v4().simple()));
    let packs = root.join("personas/roles");
    std::fs::create_dir_all(packs.join("builder")).expect("mkdir");
    std::fs::create_dir_all(packs.join("lead")).expect("mkdir");
    std::fs::write(packs.join("README.md"), "not a role").expect("write");

    assert_eq!(
        role_directories(&packs),
        vec!["builder".to_string(), "lead".to_string()],
        "only directories count, and they are sorted"
    );
    assert!(
        role_directories(&root.join("never/fetched")).is_empty(),
        "a path this machine never fetched is empty, not an error"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// `--from` must name a real directory, and without it the nearest
/// `personas/roles` is found by walking up — never invented.
#[test]
fn the_seed_directory_is_resolved_or_refused_by_name() {
    let missing = std::env::temp_dir().join(format!("bee-nope-{}", uuid::Uuid::new_v4().simple()));
    let error = resolve_seed_dir(Some(&missing)).expect_err("refused");
    assert!(format!("{error}").contains("is not a directory"), "{error}");

    let root = std::env::temp_dir().join(format!("bee-seed-{}", uuid::Uuid::new_v4().simple()));
    let roles = root.join("personas/roles/builder");
    std::fs::create_dir_all(&roles).expect("mkdir");
    assert_eq!(
        resolve_seed_dir(Some(&root.join("personas/roles"))).expect("a directory"),
        root.join("personas/roles")
    );
    std::fs::remove_dir_all(&root).ok();
}

/// The seed step builds one signed commit holding the packs under the record's
/// path and pushes it — exercised against a throwaway bare repository, never a
/// worktree of this repository.
#[test]
fn seeding_writes_the_packs_under_the_path_and_pushes_one_signed_commit() {
    let root = std::env::temp_dir().join(format!("bee-seedrun-{}", uuid::Uuid::new_v4().simple()));
    let seed = root.join("seed");
    std::fs::create_dir_all(seed.join("builder")).expect("mkdir");
    std::fs::create_dir_all(seed.join("lead")).expect("mkdir");
    std::fs::write(seed.join("builder/PERSONA.md"), "you build\n").expect("write");
    std::fs::write(seed.join("lead/PERSONA.md"), "you lead\n").expect("write");

    let remote = root.join("remote.git");
    let init = std::process::Command::new("git")
        .args(["init", "--bare", "--quiet", "--initial-branch=main"])
        .arg(&remote)
        .output()
        .expect("git init --bare");
    assert!(init.status.success(), "bare init: {init:?}");

    let seeded = seed_packs_repository(
        &seed,
        "personas/roles",
        remote.to_str().expect("utf8 remote path"),
    )
    .expect("seeding succeeds against a throwaway bare repository");
    assert_eq!(seeded.pushed_ref, "refs/heads/main");
    assert_eq!(seeded.commit.len(), 40, "a full commit id, never a prefix");

    let listing = std::process::Command::new("git")
        .args(["-C"])
        .arg(&remote)
        .args(["ls-tree", "-r", "--name-only", "refs/heads/main"])
        .output()
        .expect("ls-tree");
    let files = String::from_utf8_lossy(&listing.stdout);
    assert!(
        files.contains("personas/roles/builder/PERSONA.md")
            && files.contains("personas/roles/lead/PERSONA.md"),
        "the packs must land under the record's path: {files}"
    );

    let message = std::process::Command::new("git")
        .args(["-C"])
        .arg(&remote)
        .args(["log", "-1", "--format=%B", "refs/heads/main"])
        .output()
        .expect("git log");
    let body = String::from_utf8_lossy(&message.stdout);
    assert!(
        body.contains("Signed-off-by:"),
        "the seed commit must carry the DCO trailer: {body}"
    );

    std::fs::remove_dir_all(&root).ok();
}
