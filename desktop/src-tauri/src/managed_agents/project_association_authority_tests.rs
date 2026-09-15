use super::*;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

const SLUG: &str = "tank-loop";

fn coordinate(creator: &Keys) -> String {
    format!("30621:{}:{SLUG}", creator.public_key().to_hex())
}

fn event(keys: &Keys, kind: u32, created_at: u64, tags: Vec<Vec<String>>) -> Event {
    EventBuilder::new(Kind::Custom(kind as u16), "")
        .tags(tags.into_iter().map(|tag| Tag::parse(tag).expect("tag")))
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("sign")
}

fn tag(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| part.to_string()).collect()
}

fn member(keys: &Keys, role: Option<&str>) -> Vec<String> {
    let pubkey = keys.public_key().to_hex();
    match role {
        Some(role) => tag(&["p", &pubkey, "", role]),
        None => tag(&["p", &pubkey]),
    }
}

fn head(creator: &Keys, extra: Vec<Vec<String>>) -> Event {
    let mut tags = vec![tag(&["d", SLUG])];
    tags.extend(extra);
    event(creator, KIND_PROJECT, 100, tags)
}

fn roster(signer: &Keys, coordinate: &str, rows: Vec<Vec<String>>) -> Event {
    let mut tags = vec![tag(&["d", coordinate])];
    tags.extend(rows);
    event(signer, KIND_PROJECT_MEMBERS, 200, tags)
}

fn hex(keys: &Keys) -> String {
    keys.public_key().to_hex()
}

#[test]
fn creator_is_authorized_without_a_roster() {
    let creator = Keys::generate();
    let result = association_authority(
        &coordinate(&creator),
        &hex(&creator),
        &[head(&creator, Vec::new())],
        &[],
        "",
    );
    assert_eq!(result, Ok(ProjectVisibility::Public));
}

#[test]
fn roster_collaborator_and_owner_are_authorized() {
    let (creator, relay) = (Keys::generate(), Keys::generate());
    let (collaborator, owner) = (Keys::generate(), Keys::generate());
    let coord = coordinate(&creator);
    let rosters = [roster(
        &relay,
        &coord,
        vec![
            member(&collaborator, Some("collaborator")),
            member(&owner, Some("owner")),
        ],
    )];
    let heads = [head(&creator, Vec::new())];
    for identity in [&collaborator, &owner] {
        assert_eq!(
            association_authority(&coord, &hex(identity), &heads, &rosters, &hex(&relay)),
            Ok(ProjectVisibility::Public)
        );
    }
}

#[test]
fn roster_viewer_and_non_member_are_refused() {
    let (creator, relay) = (Keys::generate(), Keys::generate());
    let (viewer, stranger) = (Keys::generate(), Keys::generate());
    let coord = coordinate(&creator);
    let rosters = [roster(
        &relay,
        &coord,
        vec![member(&viewer, Some("viewer"))],
    )];
    let heads = [head(&creator, Vec::new())];
    for identity in [&viewer, &stranger] {
        assert_eq!(
            association_authority(&coord, &hex(identity), &heads, &rosters, &hex(&relay)),
            Err(ASSOCIATION_NOT_PROJECT_WRITER.to_string())
        );
    }
}

#[test]
fn a_missing_head_refuses_as_unreadable_even_for_the_creator() {
    let creator = Keys::generate();
    let coord = coordinate(&creator);
    let refusal = association_authority(&coord, &hex(&creator), &[], &[], "")
        .expect_err("no head must refuse");
    assert_eq!(refusal, association_project_unreadable(&coord));
    assert_eq!(
        refusal,
        "This computer could not read tank-loop from the relay, so it cannot confirm you may associate agents with it. Nothing was changed."
    );
}

#[test]
fn a_head_signed_by_another_author_is_ignored() {
    let (creator, impostor) = (Keys::generate(), Keys::generate());
    let coord = coordinate(&creator);
    let forged_head = head(&impostor, vec![member(&impostor, Some("owner"))]);
    assert_eq!(
        association_authority(
            &coord,
            &hex(&impostor),
            &[forged_head],
            &[],
            &hex(&Keys::generate())
        ),
        Err(association_project_unreadable(&coord))
    );
}

#[test]
fn a_head_with_another_d_tag_is_ignored() {
    let creator = Keys::generate();
    let coord = coordinate(&creator);
    let other = event(&creator, KIND_PROJECT, 100, vec![tag(&["d", "other"])]);
    assert_eq!(
        association_authority(&coord, &hex(&creator), &[other], &[], ""),
        Err(association_project_unreadable(&coord))
    );
}

#[test]
fn a_roster_not_signed_by_the_relay_is_ignored_and_the_head_bootstraps() {
    let (creator, relay, forger) = (Keys::generate(), Keys::generate(), Keys::generate());
    let (collaborator, stranger) = (Keys::generate(), Keys::generate());
    let coord = coordinate(&creator);
    // The forged roster names a stranger; the head names the collaborator.
    let forged = roster(&forger, &coord, vec![member(&stranger, Some("owner"))]);
    let heads = [head(&creator, vec![member(&collaborator, None)])];
    assert_eq!(
        association_authority(
            &coord,
            &hex(&stranger),
            &heads,
            std::slice::from_ref(&forged),
            &hex(&relay)
        ),
        Err(ASSOCIATION_NOT_PROJECT_WRITER.to_string())
    );
    assert_eq!(
        association_authority(&coord, &hex(&collaborator), &heads, &[forged], &hex(&relay)),
        Ok(ProjectVisibility::Public)
    );
}

#[test]
fn a_relay_roster_overrides_the_heads_p_tags() {
    let (creator, relay) = (Keys::generate(), Keys::generate());
    let (removed, kept) = (Keys::generate(), Keys::generate());
    let coord = coordinate(&creator);
    let heads = [head(
        &creator,
        vec![member(&removed, Some("owner")), member(&kept, None)],
    )];
    let rosters = [roster(
        &relay,
        &coord,
        vec![member(&kept, Some("collaborator"))],
    )];
    assert_eq!(
        association_authority(&coord, &hex(&removed), &heads, &rosters, &hex(&relay)),
        Err(ASSOCIATION_NOT_PROJECT_WRITER.to_string())
    );
    assert_eq!(
        association_authority(&coord, &hex(&kept), &heads, &rosters, &hex(&relay)),
        Ok(ProjectVisibility::Public)
    );
}

#[test]
fn the_newest_relay_roster_wins() {
    let (creator, relay, demoted) = (Keys::generate(), Keys::generate(), Keys::generate());
    let coord = coordinate(&creator);
    let old = roster(&relay, &coord, vec![member(&demoted, Some("owner"))]);
    let mut tags = vec![tag(&["d", &coord])];
    tags.push(member(&demoted, Some("viewer")));
    let new = event(&relay, KIND_PROJECT_MEMBERS, 300, tags);
    assert_eq!(
        association_authority(
            &coord,
            &hex(&demoted),
            &[head(&creator, Vec::new())],
            &[old, new],
            &hex(&relay)
        ),
        Err(ASSOCIATION_NOT_PROJECT_WRITER.to_string())
    );
}

#[test]
fn forged_signatures_are_ignored() {
    let (creator, relay, collaborator) = (Keys::generate(), Keys::generate(), Keys::generate());
    let coord = coordinate(&creator);
    let mut forged_head = head(&creator, Vec::new());
    forged_head.content = "tampered".to_string();
    assert_eq!(
        association_authority(&coord, &hex(&creator), &[forged_head], &[], ""),
        Err(association_project_unreadable(&coord))
    );
    // A tampered relay roster promoting the collaborator does not count, so
    // the head bootstrap roster (which omits them) applies.
    let mut forged_roster = roster(&relay, &coord, vec![member(&collaborator, Some("owner"))]);
    forged_roster.content = "tampered".to_string();
    assert_eq!(
        association_authority(
            &coord,
            &hex(&collaborator),
            &[head(&creator, Vec::new())],
            &[forged_roster],
            &hex(&relay)
        ),
        Err(ASSOCIATION_NOT_PROJECT_WRITER.to_string())
    );
}

#[test]
fn an_unknown_relay_signer_refuses_a_non_creator_as_unreadable() {
    let (creator, collaborator) = (Keys::generate(), Keys::generate());
    let coord = coordinate(&creator);
    let heads = [head(&creator, vec![member(&collaborator, None)])];
    assert_eq!(
        association_authority(&coord, &hex(&collaborator), &heads, &[], ""),
        Err(association_project_unreadable(&coord))
    );
}

#[test]
fn a_duplicate_viewer_row_is_not_outvoted_by_a_writer_row() {
    let (creator, relay, member_keys) = (Keys::generate(), Keys::generate(), Keys::generate());
    let coord = coordinate(&creator);
    let rosters = [roster(
        &relay,
        &coord,
        vec![
            member(&member_keys, Some("viewer")),
            member(&member_keys, Some("owner")),
        ],
    )];
    assert_eq!(
        association_authority(
            &coord,
            &hex(&member_keys),
            &[head(&creator, Vec::new())],
            &rosters,
            &hex(&relay)
        ),
        Err(ASSOCIATION_NOT_PROJECT_WRITER.to_string())
    );
}

#[test]
fn a_private_head_is_detected() {
    let creator = Keys::generate();
    let private = head(&creator, vec![tag(&["buzz-access", "private"])]);
    assert_eq!(
        association_authority(&coordinate(&creator), &hex(&creator), &[private], &[], ""),
        Ok(ProjectVisibility::Private)
    );
    let public = head(&creator, vec![tag(&["buzz-access", "public"])]);
    assert_eq!(head_visibility(&public), ProjectVisibility::Public);
}

#[test]
fn a_malformed_coordinate_is_refused() {
    let keys = Keys::generate();
    assert_eq!(
        association_authority("30621:nothex:slug", &hex(&keys), &[], &[], ""),
        Err(ASSOCIATION_MALFORMED_PROJECT.to_string())
    );
}

// ── Visibility verifier ─────────────────────────────────────────────────

fn agent(pubkey: &str, project_ref: Option<&str>, public: Option<bool>) -> ManagedAgentRecord {
    let mut record: ManagedAgentRecord = serde_json::from_value(serde_json::json!({
        "pubkey": pubkey,
        "name": pubkey,
        "relay_url": "wss://relay.example",
        "acp_command": "buzz-acp",
        "agent_command": "goose",
        "agent_args": [],
        "mcp_command": "",
        "turn_timeout_seconds": 320,
        "system_prompt": null,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "last_started_at": null,
        "last_stopped_at": null,
        "last_exit_code": null,
        "last_error": null
    }))
    .expect("record fixture");
    record.project_ref = project_ref.map(str::to_owned);
    record.project_public = public;
    record
}

#[test]
fn every_distinct_associated_project_needs_verification() {
    let a = format!("30621:{}:a", "ab".repeat(32));
    let a_upper = format!("30621:{}:a", "AB".repeat(32));
    let b = format!("30621:{}:b", "ab".repeat(32));
    let c = format!("30621:{}:c", "ab".repeat(32));
    let records = vec![
        agent("1", Some(&a), None),
        agent("2", Some(&a_upper), Some(false)),
        agent("3", Some(&b), Some(true)),
        agent("4", None, None),
        agent("5", Some("not a coordinate"), None),
        agent("6", Some(&c), None),
    ];
    assert_eq!(
        projects_needing_visibility(&records),
        vec![a, b, c],
        "a known visibility is re-read: the project may have changed"
    );
}

#[test]
fn visibility_results_replace_known_values_and_keep_unread_ones() {
    let a = format!("30621:{}:a", "ab".repeat(32));
    let b = format!("30621:{}:b", "ab".repeat(32));
    let unread = format!("30621:{}:unread", "ab".repeat(32));
    let mut records = vec![
        agent("1", Some(&a), None),
        agent("2", Some(&b), None),
        agent("3", Some(&a), Some(true)),
        agent("4", Some(&unread), None),
        agent("5", Some(&b), Some(false)),
        agent("6", Some(&unread), Some(true)),
    ];
    records[4].project_publication_withdrawn = true;
    records[2].carried_project_digest = Some("c".repeat(64));
    let results = BTreeMap::from([
        (a.clone(), ProjectVisibility::Private),
        (b.clone(), ProjectVisibility::Public),
    ]);
    let changed = apply_visibility_results(&mut records, &results);
    assert_eq!(changed, ["1", "2", "3", "5"].map(str::to_string).to_vec());
    assert_eq!(records[0].project_public, Some(false));
    assert!(
        records[0].project_publication_withdrawn,
        "private withdraws"
    );
    assert_eq!(records[1].project_public, Some(true));
    assert!(!records[1].project_publication_withdrawn);
    assert_eq!(records[2].project_public, Some(false), "public→private");
    assert!(records[2].project_publication_withdrawn);
    assert_eq!(records[2].carried_project_digest, None, "carry dropped");
    assert_eq!(
        records[3].project_public, None,
        "an unread head stays unknown"
    );
    assert_eq!(records[4].project_public, Some(true), "private→public");
    assert!(
        !records[4].project_publication_withdrawn,
        "own verified public clears the withdrawal"
    );
    assert_eq!(
        records[5].project_public,
        Some(true),
        "an unread head never replaces a known value"
    );
    assert!(apply_visibility_results(&mut records, &results).is_empty());
}
