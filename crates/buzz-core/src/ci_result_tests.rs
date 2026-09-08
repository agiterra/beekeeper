use super::*;
use crate::kind::{
    git_event_repo_names, is_git_project_gated_kind, is_relay_only_kind,
    is_workflow_execution_kind, repo_event_hidden_from,
};
use nostr::{Event, EventBuilder, Keys, Kind, Tag};
use serde_json::json;
use std::collections::HashSet;

const OWNER: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const WORKFLOW: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";

fn identity() -> CiResultIdentity {
    CiResultIdentity {
        project: format!("30621:{OWNER}:beekeeper"),
        repository: format!("30617:{OWNER}:beekeeper"),
        commit: "abcdef0123456789abcdef0123456789abcdef01".into(),
        check: "repository CI".into(),
        run: "pipeline/182".into(),
        attempt: 2,
        workflow: WORKFLOW.into(),
        phase: CiPhase::Build,
    }
}

fn result() -> CiResult {
    CiResult {
        schema: CI_RESULT_SCHEMA.into(),
        identity: identity(),
        conclusion: CiConclusion::Success,
        evidence_url: Some("https://ci.example/runs/182".into()),
        summary: Some("all required jobs passed".into()),
    }
}

fn event_with(result: &CiResult, keys: &Keys) -> Event {
    let (tags, content) = build_ci_result(result).expect("valid result");
    EventBuilder::new(Kind::Custom(KIND_CI_RESULT as u16), content)
        .tags(tags.into_iter().map(|tag| Tag::parse(tag).expect("tag")))
        .sign_with_keys(keys)
        .expect("event")
}

#[test]
fn canonical_build_and_round_trip_are_stable() {
    let expected = result();
    let digest = correlation_id(&expected.identity).expect("digest");
    assert_eq!(
        digest,
        "e91da4d2db2514ec3b9f9e7c17b9ffe1a0339c87de944ba1b2f3cb86a24a04fb"
    );
    assert_eq!(
        digest,
        correlation_id(&expected.identity).expect("same digest")
    );

    let (tags, content) = build_ci_result(&expected).expect("build");
    assert_eq!(
        tags,
        vec![
            vec!["d", &digest],
            vec!["a", &expected.identity.repository],
            vec!["project", &expected.identity.project],
            vec!["workflow", WORKFLOW],
            vec!["schema", CI_RESULT_SCHEMA],
        ]
    );
    assert_eq!(serde_json::to_string(&expected).unwrap(), content);
    assert_eq!(
        decode_ci_result(&event_with(&expected, &Keys::generate())).unwrap(),
        expected
    );

    let mut reordered = tags;
    reordered.rotate_left(2);
    let reordered = EventBuilder::new(Kind::Custom(KIND_CI_RESULT as u16), content)
        .tags(reordered.into_iter().map(|tag| Tag::parse(tag).unwrap()))
        .sign_with_keys(&Keys::generate())
        .unwrap();
    assert_eq!(decode_ci_result(&reordered).unwrap(), expected);
}

#[test]
fn correlation_uses_framed_canonical_identity_fields() {
    let first = identity();
    let mut same_concatenated_words = first.clone();
    same_concatenated_words.check = "repository C".into();
    same_concatenated_words.run = "Ipipeline/182".into();
    assert_ne!(
        correlation_id(&first).unwrap(),
        correlation_id(&same_concatenated_words).unwrap()
    );

    for mutate in [
        |i: &mut CiResultIdentity| i.project.push('x'),
        |i: &mut CiResultIdentity| i.repository.push('x'),
        |i: &mut CiResultIdentity| i.commit.replace_range(..1, "1"),
        |i: &mut CiResultIdentity| i.check.push('x'),
        |i: &mut CiResultIdentity| i.run.push('x'),
        |i: &mut CiResultIdentity| i.attempt += 1,
        |i: &mut CiResultIdentity| i.workflow = "aaaaaaaa-0000-4000-8000-000000000000".into(),
        |i: &mut CiResultIdentity| i.phase = CiPhase::Deploy,
    ] {
        let mut changed = first.clone();
        mutate(&mut changed);
        if validate_identity(&changed).is_ok() {
            assert_ne!(
                correlation_id(&first).unwrap(),
                correlation_id(&changed).unwrap()
            );
        }
    }
}

#[test]
fn identity_rejects_every_boundary_violation() {
    let mut cases = Vec::new();
    let mut value = identity();
    value.project = format!("30621:{}:beekeeper", OWNER.to_ascii_uppercase());
    cases.push(value);
    let mut value = identity();
    value.repository = format!("30621:{OWNER}:beekeeper");
    cases.push(value);
    let mut value = identity();
    value.commit = "A".repeat(40);
    cases.push(value);
    let mut value = identity();
    value.check.clear();
    cases.push(value);
    let mut value = identity();
    value.check = "é".repeat(65);
    cases.push(value);
    let mut value = identity();
    value.run = "x".repeat(MAX_CI_RUN_BYTES + 1);
    cases.push(value);
    let mut value = identity();
    value.attempt = 0;
    cases.push(value);
    let mut value = identity();
    value.workflow = WORKFLOW.to_ascii_uppercase();
    cases.push(value);

    for case in cases {
        assert!(validate_identity(&case).is_err(), "accepted {case:?}");
        assert!(correlation_id(&case).is_err());
    }

    let mut boundary = identity();
    boundary.check = "é".repeat(64);
    boundary.run = "r".repeat(MAX_CI_RUN_BYTES);
    assert!(validate_identity(&boundary).is_ok());
}

#[test]
fn result_rejects_bad_schema_urls_and_oversized_summary() {
    let mut bad_schema = result();
    bad_schema.schema = "buzz-ci-result/v2".into();
    assert!(build_ci_result(&bad_schema).is_err());

    for url in ["ftp://ci.example/182", "not a URL"] {
        let mut bad_url = result();
        bad_url.evidence_url = Some(url.into());
        assert!(build_ci_result(&bad_url).is_err());
    }
    let mut long_url = result();
    long_url.evidence_url = Some(format!(
        "https://ci.example/{}",
        "x".repeat(MAX_CI_EVIDENCE_URL_BYTES)
    ));
    assert!(build_ci_result(&long_url).is_err());
    let mut long_summary = result();
    long_summary.summary = Some("é".repeat(MAX_CI_SUMMARY_BYTES / 2 + 1));
    assert!(build_ci_result(&long_summary).is_err());

    let mut absent = result();
    absent.evidence_url = None;
    absent.summary = None;
    let (_, content) = build_ci_result(&absent).unwrap();
    assert!(!content.contains("evidence_url"));
    assert!(!content.contains("summary"));
}

#[test]
fn decoder_rejects_unknown_content_fields_and_any_tag_drift() {
    let valid = result();
    let (tags, content) = build_ci_result(&valid).unwrap();
    for mutation in [
        |value: &mut serde_json::Value| value["unexpected"] = json!(true),
        |value: &mut serde_json::Value| value["identity"]["unexpected"] = json!(true),
        |value: &mut serde_json::Value| value["identity"]["phase"] = json!("release"),
        |value: &mut serde_json::Value| value["conclusion"] = json!("unknown"),
    ] {
        let mut value: serde_json::Value = serde_json::from_str(&content).unwrap();
        mutation(&mut value);
        let unknown = EventBuilder::new(Kind::Custom(KIND_CI_RESULT as u16), value.to_string())
            .tags(tags.iter().cloned().map(|tag| Tag::parse(tag).unwrap()))
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert!(decode_ci_result(&unknown).is_err());
    }

    let mutations = [
        vec![],
        vec![vec!["extra".into(), "value".into()]],
        vec![vec!["d".into(), "0".repeat(64)]],
        vec![vec!["a".into(), valid.identity.project.clone()]],
    ];
    for additions in mutations {
        let mut changed = tags.clone();
        if additions.is_empty() {
            changed.pop();
        } else if additions[0][0] == "d" || additions[0][0] == "a" {
            let key = &additions[0][0];
            let index = changed.iter().position(|tag| &tag[0] == key).unwrap();
            changed[index] = additions[0].clone();
        } else {
            changed.extend(additions);
        }
        let event = EventBuilder::new(Kind::Custom(KIND_CI_RESULT as u16), &content)
            .tags(changed.into_iter().map(|tag| Tag::parse(tag).unwrap()))
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert!(decode_ci_result(&event).is_err());
    }

    let mut duplicate = tags.clone();
    duplicate.push(tags[0].clone());
    let duplicate = EventBuilder::new(Kind::Custom(KIND_CI_RESULT as u16), &content)
        .tags(duplicate.into_iter().map(|tag| Tag::parse(tag).unwrap()))
        .sign_with_keys(&Keys::generate())
        .unwrap();
    assert!(decode_ci_result(&duplicate).is_err());

    let wrong_kind = EventBuilder::new(Kind::Custom(46009), content)
        .tags(tags.into_iter().map(|tag| Tag::parse(tag).unwrap()))
        .sign_with_keys(&Keys::generate())
        .unwrap();
    assert!(decode_ci_result(&wrong_kind).is_err());
}

#[test]
fn decoding_does_not_mistake_an_arbitrary_valid_signature_for_relay_authority() {
    let expected = result();
    let first = event_with(&expected, &Keys::generate());
    let second = event_with(&expected, &Keys::generate());
    assert_ne!(first.pubkey, second.pubkey);
    assert_eq!(decode_ci_result(&first).unwrap(), expected);
    assert_eq!(decode_ci_result(&second).unwrap(), expected);
}

#[test]
fn kind_is_relay_only_non_recursive_and_private_repo_gated() {
    assert!(is_relay_only_kind(KIND_CI_RESULT));
    assert!(is_workflow_execution_kind(KIND_CI_RESULT));
    assert!(is_git_project_gated_kind(KIND_CI_RESULT));

    let event = event_with(&result(), &Keys::generate());
    assert_eq!(git_event_repo_names(&event), vec!["beekeeper"]);
    assert!(repo_event_hidden_from(
        &event,
        &"f".repeat(64),
        &HashSet::from(["beekeeper".into()]),
        &HashSet::new(),
    ));
}
