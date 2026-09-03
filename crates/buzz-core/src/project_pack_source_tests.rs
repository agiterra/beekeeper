//! Kind:30624 shape tests — the wire contract Lanes B and C build against.

use nostr::{EventBuilder, Keys, Kind, Tag};
use serde_json::json;

use super::*;

const OWNER: &str = "6cbdf4451d3989c10c20d13240c665a9e11e3959a95488382193481692b68df2";
const OTHER: &str = "3d3b71690000000000000000000000000000000000000000000000000000beef";

fn project() -> String {
    format!("30621:{OWNER}:agiterra")
}

fn repo() -> String {
    format!("30617:{OWNER}:agiterra-packs")
}

fn sign(tags: Vec<Vec<String>>, content: &str) -> nostr::Event {
    let keys = Keys::generate();
    let tags: Vec<Tag> = tags
        .into_iter()
        .map(|tag| Tag::parse(tag).expect("tag parses"))
        .collect();
    EventBuilder::new(Kind::Custom(KIND_PROJECT_PACK_SOURCE as u16), content)
        .tags(tags)
        .sign_with_keys(&keys)
        .expect("signs")
}

fn signed_draft(draft: &ProjectPackSourceDraft) -> nostr::Event {
    sign(draft.tags.clone(), &draft.content)
}

#[test]
fn a_pinned_record_round_trips_through_the_builder_and_the_decoder() {
    let draft = build_project_pack_source(
        &project(),
        &repo(),
        &PackPin::Sha("A".repeat(40)),
        None,
        Some("pinned for run 5"),
    )
    .expect("valid draft");
    assert_eq!(draft.d_tag, project());
    assert_eq!(
        draft.tags,
        vec![
            vec!["d".to_string(), project()],
            vec!["repo".to_string(), repo()],
            vec!["sha".to_string(), "a".repeat(40)],
        ],
        "the default path is omitted, not written"
    );

    let decoded = decode_project_pack_source(&signed_draft(&draft)).expect("decodes");
    assert_eq!(decoded.project(), project());
    assert_eq!(decoded.repo(), repo());
    assert_eq!(decoded.pin(), &PackPin::Sha("a".repeat(40)));
    assert_eq!(decoded.path(), DEFAULT_PACK_PATH);
    assert_eq!(decoded.note(), Some("pinned for run 5"));
    assert_eq!(
        decoded.role_path("builder").as_deref(),
        Some("personas/roles/builder")
    );
    assert_eq!(
        decoded.cache_dir_name().as_deref(),
        Some("6cbdf445-agiterra-packs")
    );
}

#[test]
fn a_ref_record_keeps_its_ref_and_a_custom_path() {
    let draft = build_project_pack_source(
        &project(),
        &repo(),
        &PackPin::Ref("refs/heads/main".to_string()),
        Some("packs/roles/"),
        None,
    )
    .expect("valid draft");
    let decoded = decode_project_pack_source(&signed_draft(&draft)).expect("decodes");
    assert_eq!(decoded.pin().as_ref_name(), Some("refs/heads/main"));
    assert_eq!(decoded.pin().as_sha(), None);
    assert_eq!(decoded.path(), "packs/roles", "a trailing slash is trimmed");
    assert_eq!(decoded.note(), None);
    assert_eq!(
        decoded.role_path("lead").as_deref(),
        Some("packs/roles/lead")
    );
}

/// The rule the whole kind turns on: one pin, never two and never none.
#[test]
fn exactly_one_of_ref_and_sha_is_admitted() {
    let content = json!({"schema": PROJECT_PACK_SOURCE_SCHEMA}).to_string();

    let both = sign(
        vec![
            vec!["d".into(), project()],
            vec!["repo".into(), repo()],
            vec!["ref".into(), "refs/heads/main".into()],
            vec!["sha".into(), "b".repeat(40)],
        ],
        &content,
    );
    let error = decode_project_pack_source(&both).expect_err("both pins refused");
    assert!(error.contains("not both"), "{error}");

    let neither = sign(
        vec![vec!["d".into(), project()], vec!["repo".into(), repo()]],
        &content,
    );
    let error = decode_project_pack_source(&neither).expect_err("no pin refused");
    assert!(error.contains("got neither"), "{error}");
}

#[test]
fn a_malformed_coordinate_or_pin_is_refused_by_name() {
    let content = json!({"schema": PROJECT_PACK_SOURCE_SCHEMA}).to_string();
    let cases: Vec<(Vec<Vec<String>>, &str)> = vec![
        (
            vec![
                vec!["d".into(), format!("30617:{OWNER}:agiterra")],
                vec!["repo".into(), repo()],
                vec!["sha".into(), "c".repeat(40)],
            ],
            "project coordinate",
        ),
        (
            vec![
                vec!["d".into(), project()],
                vec!["repo".into(), format!("30621:{OWNER}:agiterra")],
                vec!["sha".into(), "c".repeat(40)],
            ],
            "repository coordinate",
        ),
        (
            vec![
                vec!["d".into(), project()],
                vec!["repo".into(), repo()],
                vec!["sha".into(), "c".repeat(39)],
            ],
            "40 hex",
        ),
        (
            vec![
                vec!["d".into(), project()],
                vec!["repo".into(), repo()],
                vec!["ref".into(), "main".into()],
            ],
            "fully qualified",
        ),
        (
            vec![
                vec!["d".into(), project()],
                vec!["repo".into(), repo()],
                vec!["sha".into(), "c".repeat(40)],
                vec!["path".into(), "../../etc".into()],
            ],
            "relative segments",
        ),
        (
            vec![
                vec!["d".into(), project()],
                vec!["repo".into(), repo()],
                vec!["sha".into(), "c".repeat(40)],
                vec!["path".into(), "/etc/passwd".into()],
            ],
            "must be relative",
        ),
        (
            vec![
                vec!["d".into(), project()],
                vec!["repo".into(), repo()],
                vec!["sha".into(), "c".repeat(40)],
                vec!["packs".into(), "yes".into()],
            ],
            "unknown tag",
        ),
        (
            vec![
                vec!["d".into(), project()],
                vec!["d".into(), format!("30621:{OTHER}:other")],
                vec!["repo".into(), repo()],
                vec!["sha".into(), "c".repeat(40)],
            ],
            "more than one d tag",
        ),
    ];
    for (tags, needle) in cases {
        let event = sign(tags, &content);
        let error = decode_project_pack_source(&event).expect_err("refused");
        assert!(
            error.contains(needle),
            "expected {needle:?} in the refusal, got {error:?}"
        );
    }
}

#[test]
fn content_must_be_this_schema_and_a_note_is_never_null() {
    let tags = vec![
        vec!["d".to_string(), project()],
        vec!["repo".to_string(), repo()],
        vec!["sha".to_string(), "d".repeat(40)],
    ];

    let wrong_schema = sign(
        tags.clone(),
        &json!({"schema": "something-else/v1"}).to_string(),
    );
    let error = decode_project_pack_source(&wrong_schema).expect_err("refused");
    assert!(error.contains(PROJECT_PACK_SOURCE_SCHEMA), "{error}");

    let null_note = sign(
        tags.clone(),
        &json!({"schema": PROJECT_PACK_SOURCE_SCHEMA, "note": serde_json::Value::Null}).to_string(),
    );
    let error = decode_project_pack_source(&null_note).expect_err("refused");
    assert!(error.contains("note must not be null"), "{error}");

    let unknown_key = sign(
        tags.clone(),
        &json!({"schema": PROJECT_PACK_SOURCE_SCHEMA, "packs": "here"}).to_string(),
    );
    assert!(decode_project_pack_source(&unknown_key).is_err());

    let long_note = sign(
        tags,
        &json!({"schema": PROJECT_PACK_SOURCE_SCHEMA, "note": "x".repeat(MAX_PACK_SOURCE_NOTE_BYTES + 1)})
            .to_string(),
    );
    let error = decode_project_pack_source(&long_note).expect_err("refused");
    assert!(error.contains("at most"), "{error}");
}

#[test]
fn the_wrong_kind_is_refused_before_anything_else() {
    let keys = Keys::generate();
    let event = EventBuilder::new(
        Kind::Custom(crate::kind::KIND_PROJECT as u16),
        json!({"schema": PROJECT_PACK_SOURCE_SCHEMA}).to_string(),
    )
    .tags(vec![Tag::parse(["d", &project()]).expect("tag")])
    .sign_with_keys(&keys)
    .expect("signs");
    let error = decode_project_pack_source(&event).expect_err("refused");
    assert!(error.contains("kind 30624"), "{error}");
}

#[test]
fn a_cache_directory_name_is_owner_prefixed_so_two_owners_never_collide() {
    let mine = pack_cache_dir_name(&format!("30617:{OWNER}:packs")).expect("a name");
    let theirs = pack_cache_dir_name(&format!("30617:{OTHER}:packs")).expect("a name");
    assert_ne!(mine, theirs);
    assert_eq!(mine, "6cbdf445-packs");
    assert_eq!(pack_cache_dir_name("not-a-coordinate"), None);
    assert_eq!(pack_cache_dir_name(&project()), None);
}

/// A role that is not a role slug gets no path — the join never invents one.
#[test]
fn role_path_refuses_a_value_that_is_not_a_role_slug() {
    let draft = build_project_pack_source(
        &project(),
        &repo(),
        &PackPin::Sha("e".repeat(40)),
        None,
        None,
    )
    .expect("valid draft");
    let decoded = decode_project_pack_source(&signed_draft(&draft)).expect("decodes");
    assert_eq!(decoded.role_path("../escape"), None);
    assert_eq!(decoded.role_path("Builder"), None);
    assert_eq!(decoded.role_path(""), None);
}

/// The shared conformance vectors, decoded by this crate. Lane C's wire
/// decoder reads the same file, so a shape one side admits and the other
/// refuses fails here first.
#[test]
fn shared_pack_source_conformance_vectors_match_this_decoder() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../conformance/project-pack-source/fixtures/pack-source-vectors.json"
    ))
    .expect("fixture parses");
    assert_eq!(fixture["schema"], "buzz-project-pack-source-conformance/v1");
    let vectors = fixture["packSourceVectors"]
        .as_array()
        .expect("packSourceVectors is an array");
    assert!(vectors.len() >= 6, "the fixture must cover both outcomes");
    for vector in vectors {
        let name = vector["name"].as_str().expect("a name");
        let expected_valid = vector["valid"].as_bool().expect("a verdict");
        let tags: Vec<Vec<String>> =
            serde_json::from_value(vector["tags"].clone()).expect("tags decode");
        let content = vector["content"].to_string();
        let event = sign(tags, &content);
        let decoded = decode_project_pack_source(&event);
        assert_eq!(
            decoded.is_ok(),
            expected_valid,
            "vector {name:?} disagreed with this decoder: {decoded:?}"
        );
    }
}
