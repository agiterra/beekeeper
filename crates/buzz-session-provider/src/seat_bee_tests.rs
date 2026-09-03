//! Acceptance tests for the host's choice of `bee` and the stamp it observes.
//!
//! Every case here is one of the two failures the 2026-09-01 run produced: a
//! seat that ran a binary nobody chose, and a run that could not say which
//! binary had answered.

use super::*;
use std::collections::BTreeSet;

/// A directory layout a test owns, so resolution is provable without a global
/// `PATH` and without real executables.
struct FakeTree {
    executables: BTreeSet<PathBuf>,
}

impl FakeTree {
    fn with(paths: &[&str]) -> Self {
        Self {
            executables: paths.iter().map(PathBuf::from).collect(),
        }
    }

    fn probe(&self) -> impl Fn(&Path) -> bool + '_ {
        move |path: &Path| self.executables.contains(path)
    }
}

fn path_var(entries: &[&str]) -> OsString {
    std::env::join_paths(entries.iter().map(Path::new)).expect("a joinable PATH")
}

/// The headline: the seat whose `PATH` *starts* with a stale
/// `…/target/debug` holding its own `bee` — exactly the shape of the
/// 2026-09-01 run — still gets the bundled sidecar, and that stale directory
/// stays reachable behind it.
#[test]
fn a_stale_target_debug_bee_first_on_path_does_not_win_and_is_not_confiscated() {
    let tree = FakeTree::with(&[
        "/Applications/Beekeeper.app/Contents/MacOS/bee",
        "/Users/brian/Projects/beekeeper/beekeeper/target/debug/bee",
    ]);
    let inherited = path_var(&[
        "/Users/brian/Projects/beekeeper/beekeeper/target/debug",
        "/usr/local/bin",
        "/usr/bin",
    ]);
    let chosen = resolve_seat_bee(
        Some(Path::new("/Applications/Beekeeper.app/Contents/MacOS")),
        Some(&inherited),
        &tree.probe(),
    )
    .expect("the host holds a bee");

    assert_eq!(
        chosen.path,
        PathBuf::from("/Applications/Beekeeper.app/Contents/MacOS/bee"),
        "the bundled sidecar outranks a stale debug build that happens to be first"
    );
    assert_eq!(chosen.source, BeeStampSource::Bundled);

    let composed = compose_seat_path(chosen.directory(), Some(&inherited)).expect("a PATH");
    let entries: Vec<PathBuf> = std::env::split_paths(&composed).collect();
    assert_eq!(
        entries.first().map(PathBuf::as_path),
        Some(Path::new("/Applications/Beekeeper.app/Contents/MacOS")),
        "the chosen binary's own directory leads: {entries:?}"
    );
    assert!(
        entries.contains(&PathBuf::from(
            "/Users/brian/Projects/beekeeper/beekeeper/target/debug"
        )),
        "we choose which bee answers; we do not confiscate the directory: {entries:?}"
    );
}

/// Exactly one directory is prepended, and the chosen one appears once.
/// Two occurrences restore the ambiguity the resolution exists to remove.
#[test]
fn the_composed_path_holds_the_chosen_directory_exactly_once() {
    let inherited = path_var(&[
        "/Applications/Beekeeper.app/Contents/MacOS",
        "/usr/local/bin",
        "/Applications/Beekeeper.app/Contents/MacOS",
        "/usr/bin",
    ]);
    let composed = compose_seat_path(
        Some(Path::new("/Applications/Beekeeper.app/Contents/MacOS")),
        Some(&inherited),
    )
    .expect("a PATH");
    let entries: Vec<PathBuf> = std::env::split_paths(&composed).collect();
    let occurrences = entries
        .iter()
        .filter(|entry| entry.as_path() == Path::new("/Applications/Beekeeper.app/Contents/MacOS"))
        .count();
    assert_eq!(occurrences, 1, "one directory, once: {entries:?}");
    assert_eq!(
        entries,
        vec![
            PathBuf::from("/Applications/Beekeeper.app/Contents/MacOS"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
        ],
        "every other entry keeps its relative order"
    );
}

/// No sidecar: the host falls back to the first `bee` on `PATH`, scanned left
/// to right, and the record says so — `found on PATH` is a different fact from
/// `bundled` and the surfaces render it differently.
#[test]
fn with_no_sidecar_the_first_bee_on_path_is_chosen_and_recorded_as_such() {
    let tree = FakeTree::with(&["/opt/homebrew/bin/bee", "/usr/local/bin/bee"]);
    let inherited = path_var(&["/usr/bin", "/opt/homebrew/bin", "/usr/local/bin"]);
    let chosen = resolve_seat_bee(
        Some(Path::new("/Applications/Beekeeper.app/Contents/MacOS")),
        Some(&inherited),
        &tree.probe(),
    )
    .expect("PATH holds one");
    assert_eq!(chosen.path, PathBuf::from("/opt/homebrew/bin/bee"));
    assert_eq!(chosen.source, BeeStampSource::Path);
    assert_eq!(chosen.source.as_wire(), "path");
}

/// There is no third step. A host holding no `bee` gives the seat none rather
/// than a guess, and the seat keeps the `PATH` it would have had.
#[test]
fn a_host_with_no_bee_anywhere_chooses_nothing() {
    let tree = FakeTree::with(&["/usr/bin/git"]);
    let inherited = path_var(&["/usr/bin"]);
    assert!(resolve_seat_bee(
        Some(Path::new("/Applications/Beekeeper.app/Contents/MacOS")),
        Some(&inherited),
        &tree.probe(),
    )
    .is_none());
}

/// `BEE` sits outside the `BUZZ_` fence, so an operator's ambient value would
/// otherwise survive into the seat. The post-fence pair replaces it — asserted
/// by value, because "it is set" and "it is set to the binary we chose" are
/// different claims.
#[test]
fn the_seat_env_names_the_chosen_binary_and_replaces_an_ambient_value() {
    let bee = SeatBee {
        path: PathBuf::from("/Applications/Beekeeper.app/Contents/MacOS/bee"),
        source: BeeStampSource::Bundled,
    };
    let inherited = path_var(&["/usr/local/bin", "/usr/bin"]);
    let env = seat_bee_env(&bee, Some(&inherited));

    let value = env
        .iter()
        .find(|(key, _)| key == BEE_ENV)
        .map(|(_, value)| value.as_str());
    assert_eq!(
        value,
        Some("/Applications/Beekeeper.app/Contents/MacOS/bee"),
        "BEE must name the binary the host chose, not whatever the shell held"
    );
    let path = env
        .iter()
        .find(|(key, _)| key == "PATH")
        .map(|(_, value)| value.as_str())
        .expect("a PATH is composed");
    assert!(
        path.starts_with("/Applications/Beekeeper.app/Contents/MacOS:"),
        "the chosen directory leads the seat's PATH: {path}"
    );
    assert_eq!(
        env.len(),
        2,
        "exactly two variables — one path and one PATH: {env:?}"
    );
}

/// The three stamp shapes `bee --version` can print, each read correctly. The
/// `-dirty` suffix becomes the separate `dirty` flag so no consumer has to
/// string-strip a commit name before comparing it.
#[test]
fn every_version_shape_the_cli_prints_parses_to_the_fact_it_states() {
    let clean = parse_bee_version("bee 0.1.0 (23728227b)\n").expect("a clean build");
    assert_eq!(clean.version, "0.1.0");
    assert_eq!(clean.sha.as_deref(), Some("23728227b"));
    assert_eq!(clean.dirty, Some(false));

    let dirty = parse_bee_version("bee 0.1.0 (07c470be0-dirty)\n").expect("a dirty build");
    assert_eq!(dirty.sha.as_deref(), Some("07c470be0"));
    assert_eq!(dirty.dirty, Some(true));

    let unknown = parse_bee_version("bee 0.1.0 (unknown)\n").expect("a build with no commit");
    assert_eq!(unknown.version, "0.1.0");
    assert!(
        unknown.sha.is_none() && unknown.dirty.is_none(),
        "`unknown` is the absence of a commit; there is nothing for dirty to qualify"
    );
}

/// Anything else is unknown, never half-believed. A version line is provenance
/// — a partially-parsed one is exactly the comfortable guess this refuses.
#[test]
fn an_unrecognised_version_line_is_never_partially_believed() {
    for line in [
        "",
        "bee",
        "bee 0.1.0",
        "bee 0.1.0 (not-a-sha)",
        "bee 0.1.0 (ABCDEF123)",
        "bee 0.1.0 (abc)",
        "bee 0.1.0 (a) (b)",
        "some other tool 1.2.3 (deadbeef1)\nbee 0.1.0 (23728227b)",
    ] {
        assert!(
            parse_bee_version(line).is_none(),
            "{line:?} is not a bee version line and must not be read as one"
        );
    }
}

/// A binary that will not run, or answers non-zero, still yields a record: the
/// path and how it was chosen, with the build unknown. It never fails a seat —
/// a refusal here would be a new way to lose a run over a version string.
#[test]
fn a_binary_that_cannot_answer_records_unknown_and_never_fails_the_seat() {
    let bee = SeatBee {
        path: PathBuf::from("/nonexistent/definitely-not-a-binary/bee"),
        source: BeeStampSource::Path,
    };
    let stamp = observe_bee_stamp(&bee);
    assert_eq!(stamp.path, "/nonexistent/definitely-not-a-binary/bee");
    assert_eq!(stamp.source, BeeStampSource::Path);
    assert!(
        stamp.version.is_none() && stamp.sha.is_none() && stamp.dirty.is_none(),
        "an answer that never came is unknown on all three counts: {stamp:?}"
    );
}

/// Nothing composes an empty `PATH`: `Command::env("PATH", "")` strips a child
/// of every system binary, which looks like a broken machine rather than a
/// missing choice.
#[test]
fn nothing_to_compose_sets_no_path_at_all() {
    assert!(compose_seat_path(None, None).is_none());
}
