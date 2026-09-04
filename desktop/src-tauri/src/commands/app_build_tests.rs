use super::*;

const SHA: &str = "42dd921d831c483e6e16111491b39947b4cf1f86";

#[test]
fn a_full_stamp_is_read_back_whole() {
    let identity = parse_app_build_identity(Some(SHA), Some("3291"), None);
    assert_eq!(identity.commit.as_deref(), Some(SHA));
    assert_eq!(identity.commit_count, Some(3291));
    assert_eq!(identity.source_dirty, None);
}

#[test]
fn a_commit_is_lowercased_and_anything_short_of_forty_hex_is_refused() {
    assert_eq!(
        parse_app_build_identity(Some(&SHA.to_ascii_uppercase()), None, None)
            .commit
            .as_deref(),
        Some(SHA),
        "git prints lowercase; an uppercase stamp still names the same object"
    );
    for junk in ["", "42dd921d8", "not-hex", &"z".repeat(40), &"a".repeat(41)] {
        assert_eq!(
            parse_app_build_identity(Some(junk), None, None).commit,
            None,
            "refused rather than served: {junk:?}"
        );
    }
    assert_eq!(parse_app_build_identity(None, None, None).commit, None);
}

#[test]
fn a_count_is_never_zero_never_junk_and_never_wraps() {
    for junk in ["", " ", "0", "-1", "+1", "1e3", "12a", "4294967296"] {
        assert_eq!(
            parse_app_build_identity(Some(SHA), Some(junk), None).commit_count,
            None,
            "a count that is not what git prints must be absent, not guessed: {junk:?}"
        );
    }
    assert_eq!(
        parse_app_build_identity(Some(SHA), Some("1"), None).commit_count,
        Some(1),
        "the root commit's own count is 1 and is legitimate"
    );
}

#[test]
fn a_count_without_a_commit_describes_nothing_and_is_dropped() {
    let identity = parse_app_build_identity(None, Some("3291"), None);
    assert_eq!(identity.commit, None);
    assert_eq!(
        identity.commit_count, None,
        "an ordinal with no commit names no history; carrying it would invite \
         a comparison against a commit it does not describe"
    );
}

#[test]
fn a_clean_build_is_never_claimed_only_a_dirty_one_is_observed() {
    assert_eq!(
        parse_app_build_identity(Some(SHA), None, Some("1")).source_dirty,
        Some(true)
    );
    // build.rs emits the variable only when it saw a modified tree, so every
    // other value means "not observed", never "observed clean". Rendering
    // `None` as clean would be a claim the build script deliberately refused
    // to make.
    for absent in [None, Some("0"), Some(""), Some("true")] {
        assert_eq!(
            parse_app_build_identity(Some(SHA), None, absent).source_dirty,
            None,
            "{absent:?} must not be read as a clean claim"
        );
    }
}

#[test]
fn the_identity_serializes_camel_case_for_the_frontend() {
    let json = serde_json::to_value(parse_app_build_identity(Some(SHA), Some("3291"), Some("1")))
        .expect("serialize");
    assert_eq!(json["commit"], SHA);
    assert_eq!(json["commitCount"], 3291);
    assert_eq!(json["sourceDirty"], true);
}

#[test]
fn an_undeterminable_build_serializes_nulls_rather_than_omitting_fields() {
    let json = serde_json::to_value(parse_app_build_identity(None, None, None)).expect("serialize");
    for key in ["commit", "commitCount", "sourceDirty"] {
        assert!(
            json.get(key).is_some_and(serde_json::Value::is_null),
            "{key} must be present and null — an absent key reads as 'this build \
             predates the field' rather than 'this build could not tell'"
        );
    }
}
