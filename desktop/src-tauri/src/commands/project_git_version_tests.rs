use std::path::{Path, PathBuf};

use super::{
    candidate_gits, describe_selection, parse_git_version, remote_git_refusal, select_git,
    GitVersion, MINIMUM_REMOTE_GIT,
};

const HINT: &str = "Install it with `brew install git` and relaunch.";

fn version(major: u32, minor: u32, patch: u32) -> GitVersion {
    GitVersion {
        major,
        minor,
        patch,
    }
}

#[test]
fn every_vendors_version_line_parses_to_its_numbers() {
    assert_eq!(
        parse_git_version("git version 2.39.5 (Apple Git-154)"),
        Some(version(2, 39, 5))
    );
    assert_eq!(
        parse_git_version("git version 2.55.0\n"),
        Some(version(2, 55, 0))
    );
    assert_eq!(parse_git_version("2.55.0"), Some(version(2, 55, 0)));
    assert_eq!(
        parse_git_version("git version 2.47.0.windows.1"),
        Some(version(2, 47, 0))
    );
    // A short version is not an error: the missing components are zero.
    assert_eq!(parse_git_version("git version 3"), Some(version(3, 0, 0)));
}

#[test]
fn garbage_is_never_read_as_a_version() {
    assert_eq!(parse_git_version(""), None);
    assert_eq!(parse_git_version("git version"), None);
    assert_eq!(parse_git_version("command not found: git"), None);
    assert_eq!(parse_git_version("git version x.y.z"), None);
}

#[test]
fn candidates_lead_with_the_resolved_git_and_never_repeat_one() {
    let candidates = candidate_gits(
        Some(PathBuf::from("/usr/bin/git")),
        Some(Path::new("/w/space")),
    );
    assert_eq!(
        candidates,
        vec![
            PathBuf::from("/usr/bin/git"),
            PathBuf::from("/opt/homebrew/bin/git"),
            PathBuf::from("/usr/local/bin/git"),
            PathBuf::from("/opt/homebrew/opt/git/bin/git"),
            PathBuf::from("/w/space/bin/git"),
        ]
    );
    // The `PATH` git is often already a well-known one; it is not probed
    // twice for it.
    assert_eq!(
        candidate_gits(Some(PathBuf::from("/opt/homebrew/bin/git")), None),
        vec![
            PathBuf::from("/opt/homebrew/bin/git"),
            PathBuf::from("/usr/local/bin/git"),
            PathBuf::from("/opt/homebrew/opt/git/bin/git"),
        ]
    );
}

#[test]
fn the_first_capable_git_wins_and_the_first_answer_is_still_reported() {
    // The bundle's own shape: Apple's git on `PATH`, Homebrew's behind it.
    let candidates = candidate_gits(
        Some(PathBuf::from("/usr/bin/git")),
        Some(Path::new("/w/space")),
    );
    let selection = select_git(&candidates, |path| match path.to_str() {
        Some("/usr/bin/git") => Some(version(2, 39, 5)),
        Some("/opt/homebrew/bin/git") => Some(version(2, 55, 0)),
        _ => None,
    });
    assert_eq!(
        selection.capable,
        Some((PathBuf::from("/opt/homebrew/bin/git"), version(2, 55, 0)))
    );
    assert_eq!(
        selection.found,
        Some((PathBuf::from("/usr/bin/git"), version(2, 39, 5)))
    );
}

#[test]
fn the_minimum_is_inclusive_and_nothing_below_it_is_chosen() {
    let exactly = select_git(&[PathBuf::from("/a/git")], |_| Some(MINIMUM_REMOTE_GIT));
    assert_eq!(
        exactly.capable,
        Some((PathBuf::from("/a/git"), MINIMUM_REMOTE_GIT))
    );
    let below = select_git(&[PathBuf::from("/a/git")], |_| Some(version(2, 45, 9)));
    assert_eq!(below.capable, None);
    assert_eq!(
        below.found,
        Some((PathBuf::from("/a/git"), version(2, 45, 9)))
    );
}

#[test]
fn a_candidate_that_does_not_answer_is_skipped_not_counted() {
    let selection = select_git(
        &[
            PathBuf::from("/missing/git"),
            PathBuf::from("/opt/homebrew/bin/git"),
        ],
        |path| {
            if path.to_str() == Some("/opt/homebrew/bin/git") {
                Some(version(2, 50, 0))
            } else {
                None
            }
        },
    );
    assert_eq!(
        selection.found,
        Some((PathBuf::from("/opt/homebrew/bin/git"), version(2, 50, 0)))
    );
}

#[test]
fn the_refusal_names_the_git_it_found_its_version_the_need_and_the_remedy() {
    assert_eq!(
        remote_git_refusal(Some((Path::new("/usr/bin/git"), version(2, 39, 5))), HINT),
        "git 2.39.5 at /usr/bin/git cannot authenticate to the relay: the Nostr credential \
         helper needs git 2.46 or newer. Install it with `brew install git` and relaunch."
    );
}

#[test]
fn a_machine_with_no_git_is_refused_without_naming_a_version_it_does_not_have() {
    let refusal = remote_git_refusal(None, HINT);
    assert!(refusal.starts_with("no git was found"), "{refusal}");
    assert!(refusal.contains("2.46 or newer"), "{refusal}");
    assert!(!refusal.contains(" at "), "{refusal}");
}

#[test]
fn the_disclosure_reports_the_chosen_git_or_the_one_that_disappointed() {
    let capable = select_git(&[PathBuf::from("/opt/homebrew/bin/git")], |_| {
        Some(version(2, 55, 0))
    });
    let described = describe_selection(&capable);
    assert_eq!(described.path.as_deref(), Some("/opt/homebrew/bin/git"));
    assert_eq!(described.version.as_deref(), Some("2.55.0"));
    assert!(described.meets_minimum);
    assert_eq!(described.minimum, "2.46");

    let old = select_git(&[PathBuf::from("/usr/bin/git")], |_| {
        Some(version(2, 39, 5))
    });
    let described = describe_selection(&old);
    assert_eq!(described.path.as_deref(), Some("/usr/bin/git"));
    assert_eq!(described.version.as_deref(), Some("2.39.5"));
    assert!(!described.meets_minimum);

    let none = describe_selection(&select_git(&[PathBuf::from("/usr/bin/git")], |_| None));
    assert_eq!(none.path, None);
    assert_eq!(none.version, None);
    assert!(!none.meets_minimum);
}
