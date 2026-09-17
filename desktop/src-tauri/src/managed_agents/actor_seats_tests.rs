//! Tests for host-local seat custody and the role-driven staging rule.
//!
//! Split out of `actor_seats.rs` to keep both files under the repository's
//! 1000-line ceiling; `use super::*` keeps every claim against the same
//! module it was written for.

use super::*;

const PUBKEY: &str = "aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66";

fn agent_record(
    source_team: Option<&str>,
    slug: Option<&str>,
) -> crate::managed_agents::types::ManagedAgentRecord {
    crate::managed_agents::types::AgentDefinition {
        id: "def".into(),
        display_name: "Builder".into(),
        avatar_url: None,
        system_prompt: String::new(),
        runtime: None,
        model: None,
        provider: None,
        name_pool: vec![],
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: source_team.map(str::to_owned),
        source_team_persona_slug: slug.map(str::to_owned),
        catalog_source: None,
        env_vars: Default::default(),
        respond_to: None,
        respond_to_allowlist: vec![],
        parallelism: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
    .into_agent_record()
}

fn team_record(id: &str, source_dir: Option<PathBuf>) -> crate::managed_agents::types::TeamRecord {
    crate::managed_agents::types::TeamRecord {
        id: id.into(),
        name: id.into(),
        description: None,
        instructions: None,
        persona_ids: vec![],
        crew: None,
        is_builtin: false,
        source_dir,
        is_symlink: false,
        symlink_target: None,
        version: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

/// A directory that really is a role pack, with one persona in it.
fn role_pack(root: &Path, persona: &str) -> PathBuf {
    let pack = root.join("pack");
    std::fs::create_dir_all(pack.join(".plugin")).expect("plugin dir");
    std::fs::create_dir_all(pack.join("personas")).expect("personas dir");
    std::fs::write(
        pack.join(".plugin/plugin.json"),
        format!(
            r#"{{"id":"com.test.roles","name":"Roles","version":"0.1.0","personas":["personas/{persona}.persona.md"]}}"#
        ),
    )
    .expect("plugin.json");
    std::fs::write(
        pack.join(format!("personas/{persona}.persona.md")),
        format!(
            "---\nname: {persona}\ndisplay_name: {persona}\ndescription: Builds.\nrole: builder\n---\nYou build.\n"
        ),
    )
    .expect("persona");
    pack
}

/// A pack whose persona declares no role at all — the shape every agent
/// installed before crew roles carries.
fn roleless_pack(root: &Path, persona: &str) -> PathBuf {
    let pack = root.join(format!("{persona}-roleless"));
    std::fs::create_dir_all(pack.join(".plugin")).expect("plugin dir");
    std::fs::create_dir_all(pack.join("personas")).expect("personas dir");
    std::fs::write(
        pack.join(".plugin/plugin.json"),
        format!(
            r#"{{"id":"com.test.{persona}","name":"{persona}","version":"0.1.0","personas":["personas/{persona}.persona.md"]}}"#
        ),
    )
    .expect("plugin.json");
    std::fs::write(
        pack.join(format!("personas/{persona}.persona.md")),
        format!(
            "---\nname: {persona}\ndisplay_name: {persona}\ndescription: Helps.\n---\nYou help.\n"
        ),
    )
    .expect("persona");
    pack
}

/// A role pack in its own directory, whose persona declares `role`.
fn named_role_pack(root: &Path, role: &str) -> PathBuf {
    let pack = root.join(format!("{role}-pack"));
    std::fs::create_dir_all(pack.join(".plugin")).expect("plugin dir");
    std::fs::create_dir_all(pack.join("personas")).expect("personas dir");
    std::fs::write(
        pack.join(".plugin/plugin.json"),
        format!(
            r#"{{"id":"com.test.{role}","name":"{role}","version":"0.1.0","personas":["personas/{role}.persona.md"]}}"#
        ),
    )
    .expect("plugin.json");
    std::fs::write(
        pack.join(format!("personas/{role}.persona.md")),
        format!(
            "---\nname: {role}\ndisplay_name: {role}\ndescription: Does {role} work.\nrole: {role}\n---\nYou are the {role}.\n"
        ),
    )
    .expect("persona");
    pack
}

/// An installed crew-role agent: home role declared, pack linked.
fn installed_role_agent(
    role: &str,
    pack: &Path,
) -> crate::managed_agents::types::ManagedAgentRecord {
    let mut record = agent_record(None, None);
    record.home_role = Some(role.to_owned());
    record.persona_team_dir = Some(pack.to_path_buf());
    record.persona_name_in_team = Some(role.to_owned());
    record
}

/// The staging bug this lane exists for (`docs/CREW_FRONT_DOOR.md`: "the
/// pack that gets staged is still the **home** role's pack").
///
/// RED, before the fix — `resolve_seat_pack(&builder, &[])` returned
/// `…/builder-pack` for a seat created with role `architect`:
///
/// ```text
/// assertion `left == right` failed: a seat whose role is `architect`
///   left: ".../builder-pack"
///  right: ".../architect-pack"
/// ```
#[test]
fn a_seat_stages_the_pack_of_the_role_it_was_seated_with() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let builder_pack = named_role_pack(tmp.path(), "builder");
    let architect_pack = named_role_pack(tmp.path(), "architect");
    let builder = installed_role_agent("builder", &builder_pack);
    let architect = installed_role_agent("architect", &architect_pack);
    let records = vec![builder.clone(), architect];

    // The builder identity, seated as an architect: the architect's pack.
    let staged = resolve_local_seat_pack(&builder, &records, &[], Some("architect"))
        .expect("this computer has an architect pack");
    assert_eq!(
        staged.0, architect_pack,
        "a seat whose role is `architect` must stage the architect pack, not {:?}",
        staged.0
    );
    assert_eq!(staged.1, "architect");

    // Seated at its own role, it still stages its own pack.
    assert_eq!(
        resolve_local_seat_pack(&builder, &records, &[], Some("builder")),
        Some((builder_pack, "builder".to_owned()))
    );
}

#[test]
fn a_seat_never_stages_another_projects_pack_for_a_shared_role() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let p1 = format!("30621:{}:p1", "ab".repeat(32));
    let p2 = format!("30621:{}:p2", "ab".repeat(32));
    let architect_pack = named_role_pack(tmp.path(), "architect");
    let builder_pack = named_role_pack(tmp.path(), "builder");
    let mut p1_builder = installed_role_agent("builder", &builder_pack);
    p1_builder.project_ref = Some(p1.clone());
    let mut p2_architect = installed_role_agent("architect", &architect_pack);
    p2_architect.project_ref = Some(p2);
    let records = vec![p1_builder.clone(), p2_architect.clone()];
    assert!(
        resolve_local_seat_pack(&p1_builder, &records, &[], Some("architect")).is_none(),
        "P2's architect pack must not be staged into a P1 seat"
    );
    p2_architect.project_ref = Some(p1);
    let records = vec![p1_builder.clone(), p2_architect];
    assert_eq!(
        resolve_local_seat_pack(&p1_builder, &records, &[], Some("architect")).map(|(dir, _)| dir),
        Some(architect_pack),
    );
}

#[test]
fn a_role_this_computer_holds_no_pack_for_stages_no_pack() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let builder_pack = named_role_pack(tmp.path(), "builder");
    let builder = installed_role_agent("builder", &builder_pack);
    let records = vec![builder.clone()];
    // Not the builder's pack, and not a guess: a role with no pack here is
    // a packless seat, which every screen is required to disclose.
    assert!(
        resolve_local_seat_pack(&builder, &records, &[], Some("verifier")).is_none(),
        "a seat with no pack for its role must not be given another role's"
    );
}

#[test]
fn an_agent_whose_pack_claims_no_role_still_stages_it() {
    // A persona with no `role:` frontmatter makes no claim a seat role
    // could contradict — that pack is simply this agent's own skills, and
    // withholding it would regress every agent installed before roles.
    let tmp = tempfile::tempdir().expect("temp dir");
    let dir = roleless_pack(tmp.path(), "helper");
    let mut record = agent_record(None, None);
    record.persona_team_dir = Some(dir.clone());
    record.persona_name_in_team = Some("helper".into());
    let records = vec![record.clone()];
    assert_eq!(
        resolve_local_seat_pack(&record, &records, &[], Some("runner")),
        Some((dir, "helper".to_owned())),
    );
}

#[test]
fn a_seat_with_no_role_keeps_the_behaviour_it_always_had() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let dir = role_pack(tmp.path(), "builder");
    let record = agent_record(Some("team-1"), Some("builder"));
    let teams = vec![team_record("team-1", Some(dir.clone()))];
    assert_eq!(
        resolve_local_seat_pack(&record, &[], &teams, None),
        Some((dir, "builder".to_owned())),
    );
    // Whitespace is not a role.
    assert!(resolve_local_seat_pack(&record, &[], &teams, Some("  ")).is_some());
}

/// A `named_role_pack` whose persona has since been rewritten to declare
/// `declares` — the agent's record still says `role`, the pack no longer does.
/// Finding 94's shape: the label is stale, the frontmatter is the fact.
fn relabelled_role_pack(root: &Path, role: &str, declares: &str) -> PathBuf {
    let pack = named_role_pack(root, role);
    std::fs::write(
        pack.join(format!("personas/{role}.persona.md")),
        format!(
            "---\nname: {role}\ndisplay_name: {role}\ndescription: Was {role}.\nrole: {declares}\n---\nYou are now the {declares}.\n"
        ),
    )
    .expect("persona");
    pack
}

/// Finding 94: `resolve_local_seat_pack` returned a pack on the strength of
/// the agent's `home_role` label alone, without asking the persona what role
/// it declares. A `verifier`-labelled agent whose pack now declares `builder`
/// staged that builder pack into a verifier seat.
///
/// RED, before the fix — step 1 returned the relabelled pack:
///
/// ```text
/// assertion failed: resolve_local_seat_pack(&stale, &records, &[], Some("verifier")).is_none()
/// ```
#[test]
fn resolve_local_seat_pack_refuses_a_home_role_label_whose_pack_declares_another_role() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let stale_pack = relabelled_role_pack(tmp.path(), "verifier", "builder");
    let stale = installed_role_agent("verifier", &stale_pack);
    let records = vec![stale.clone()];
    assert!(
        resolve_local_seat_pack(&stale, &records, &[], Some("verifier")).is_none(),
        "a pack whose persona declares `builder` is not a verifier seat's pack, \
         whatever the agent's record says"
    );
}

/// The same stale label on *another* installed agent (step 2): the label
/// selects the candidate, the declared role refuses it, and the search moves
/// on — here to `None`, because nothing on this computer declares `verifier`.
#[test]
fn resolve_local_seat_pack_refuses_another_agents_label_whose_pack_declares_another_role() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let builder_pack = named_role_pack(tmp.path(), "builder");
    let builder = installed_role_agent("builder", &builder_pack);
    let stale_pack = relabelled_role_pack(tmp.path(), "verifier", "builder");
    let stale = installed_role_agent("verifier", &stale_pack);
    let records = vec![builder.clone(), stale];
    assert!(
        resolve_local_seat_pack(&builder, &records, &[], Some("verifier")).is_none(),
        "the only verifier-labelled agent's pack declares `builder`; no verifier pack here"
    );
}

/// A refusal on step 1 falls through rather than ending the search: the
/// actor's own relabelled pack is passed over and the pack that really
/// declares the seat's role, installed on another agent, is staged.
#[test]
fn resolve_local_seat_pack_falls_through_a_relabelled_own_pack_to_one_declaring_the_seat_role() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let stale_pack = relabelled_role_pack(tmp.path(), "verifier", "builder");
    let stale = installed_role_agent("verifier", &stale_pack);
    let real_pack = tmp.path().join("real");
    let real_verifier_pack = named_role_pack(&real_pack, "verifier");
    let real = installed_role_agent("verifier", &real_verifier_pack);
    let records = vec![stale.clone(), real];
    assert_eq!(
        resolve_local_seat_pack(&stale, &records, &[], Some("verifier")),
        Some((real_verifier_pack, "verifier".to_owned())),
        "the declared role decides; the home-role label only orders the search"
    );
}

/// The declared role decides in the other direction too: an agent whose
/// record is labelled `builder` but whose pack declares `verifier` is a
/// verifier pack for a verifier seat (reached on step 3, its own pack).
#[test]
fn resolve_local_seat_pack_stages_a_pack_declaring_the_seat_role_whatever_the_label_says() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let pack = relabelled_role_pack(tmp.path(), "builder", "verifier");
    let mislabelled = installed_role_agent("builder", &pack);
    let records = vec![mislabelled.clone()];
    assert_eq!(
        resolve_local_seat_pack(&mislabelled, &records, &[], Some("verifier")),
        Some((pack.clone(), "builder".to_owned())),
    );
    // And it is refused for the seat its stale label names.
    assert!(
        resolve_local_seat_pack(&mislabelled, &records, &[], Some("builder")).is_none(),
        "a pack declaring `verifier` is not a builder seat's pack"
    );
}

/// Step 3 is unchanged: a home-role match on a pack whose persona declares
/// no role falls through steps 1 and 2 (no declaration to match) and is still
/// staged as the actor's own, claim-free pack.
#[test]
fn resolve_local_seat_pack_still_stages_a_roleless_pack_as_the_actors_own() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let dir = roleless_pack(tmp.path(), "runner");
    let mut record = agent_record(None, None);
    record.home_role = Some("runner".into());
    record.persona_team_dir = Some(dir.clone());
    record.persona_name_in_team = Some("runner".into());
    let records = vec![record.clone()];
    assert_eq!(
        resolve_local_seat_pack(&record, &records, &[], Some("runner")),
        Some((dir, "runner".to_owned())),
    );
}

#[test]
fn a_packref_rides_only_the_pack_it_names() {
    let pack_ref = packs_cache::PackRef {
        repo: format!("30617:{PUBKEY}:packs"),
        sha: "a".repeat(40),
        role: "builder".into(),
        path: "personas/roles/builder".into(),
    };
    let tmp = tempfile::tempdir().expect("temp dir");
    let dir = role_pack(tmp.path(), "builder");
    let seated = build_actor_seat_entry(
        PUBKEY,
        "nsec1secret",
        None,
        "wss://r",
        None,
        Some((dir, "builder".to_owned())),
        Some(pack_ref.clone()),
    )
    .expect("seat");
    assert_eq!(seated.pack_ref.as_ref(), Some(&pack_ref));
    let json = serde_json::to_value(&seated).expect("serialize");
    assert_eq!(
        json.pointer("/packRef/sha").and_then(|v| v.as_str()),
        Some(pack_ref.sha.as_str()),
        "the provider reads packRef off this entry: {json}"
    );

    // No pack staged, so nothing to describe — a packRef here would be a
    // proof of something that did not happen.
    let packless = build_actor_seat_entry(
        PUBKEY,
        "nsec1secret",
        None,
        "wss://r",
        None,
        None,
        Some(pack_ref),
    )
    .expect("seat");
    assert_eq!(packless.pack_ref, None);
    assert!(serde_json::to_value(&packless)
        .expect("serialize")
        .get("packRef")
        .is_none());
}

#[test]
fn a_seat_carries_the_pack_its_agent_was_installed_from() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let dir = role_pack(tmp.path(), "builder");
    let record = agent_record(Some("team-1"), Some("builder"));
    let teams = vec![team_record("team-1", Some(dir.clone()))];
    let pack = resolve_seat_pack(&record, &teams).expect("the pack is host-local, not on the wire");
    assert_eq!(pack.0, dir);
    assert_eq!(pack.1, "builder");

    let entry = build_actor_seat_entry(
        PUBKEY,
        "nsec1secret",
        None,
        "wss://relay.example",
        None,
        Some(pack),
        None,
    )
    .expect("seat");
    let json = serde_json::to_value(&entry).expect("serialize");
    assert_eq!(
        json.get("packDir").and_then(|v| v.as_str()),
        Some(dir.to_string_lossy().as_ref()),
        "the provider reads packDir/personaId off this entry"
    );
    assert_eq!(
        json.get("personaId").and_then(|v| v.as_str()),
        Some("builder")
    );
}

#[test]
fn a_pack_that_does_not_hold_the_persona_stages_no_pack() {
    // The provenance fallback used to pair the agent's slug with whatever
    // directory its team names, checking only that the directory exists.
    // An agent whose definition arrived from another device carries the
    // 30175 d-tag uuid as its slug (persona_events.rs), and no pack has a
    // persona by that name — so the seat was staged with a pack the
    // provider cannot read, and the provider's materialize_seat_skills
    // turns that into CreateFailure{PROVIDER_UNAVAILABLE}: a create that
    // worked before role packs existed, refused afterwards. A seat with no
    // readable pack is a packless seat, not a refused create.
    let tmp = tempfile::tempdir().expect("temp dir");
    let dir = role_pack(tmp.path(), "builder");
    let teams = vec![team_record("team-1", Some(dir.clone()))];

    assert!(
        resolve_seat_pack(
            &agent_record(Some("team-1"), Some("9f2c8d1e-inbound-uuid")),
            &teams,
        )
        .is_none(),
        "a slug the pack has no persona for is not a pack"
    );
    // Same for a renamed or removed persona reached by the instance-side
    // link rather than the provenance fallback.
    let mut linked = agent_record(None, None);
    linked.persona_team_dir = Some(dir.clone());
    linked.persona_name_in_team = Some("architect".into());
    assert!(
        resolve_seat_pack(&linked, &teams).is_none(),
        "a persona no longer in the pack is not a pack"
    );
    // A directory that is not a pack at all.
    let plain = tmp.path().join("plain");
    std::fs::create_dir_all(&plain).expect("plain dir");
    assert!(
        resolve_seat_pack(
            &agent_record(Some("team-2"), Some("builder")),
            &[team_record("team-2", Some(plain))],
        )
        .is_none(),
        "an ordinary directory is not a pack"
    );
    // The persona the pack really holds still resolves.
    assert_eq!(
        resolve_seat_pack(&agent_record(Some("team-1"), Some("builder")), &teams),
        Some((dir, "builder".to_owned())),
    );
}

#[test]
fn an_agent_with_no_pack_on_this_computer_stages_no_pack() {
    // No provenance at all.
    assert!(resolve_seat_pack(&agent_record(None, None), &[]).is_none());
    // A slug whose team is JSON-only: there is no directory to read.
    assert!(resolve_seat_pack(
        &agent_record(Some("team-1"), Some("builder")),
        &[team_record("team-1", None)],
    )
    .is_none());
    // A team directory that no longer exists is not a pack either.
    assert!(resolve_seat_pack(
        &agent_record(Some("team-1"), Some("builder")),
        &[team_record("team-1", Some(PathBuf::from("/nope/not/here")))],
    )
    .is_none());

    let entry = build_actor_seat_entry(PUBKEY, "nsec1secret", None, "wss://r", None, None, None)
        .expect("seat");
    let json = serde_json::to_value(&entry).expect("serialize");
    assert!(
        json.get("packDir").is_none() && json.get("personaId").is_none(),
        "a packless seat writes no pack keys: {json}"
    );
}

#[test]
fn a_seat_without_a_key_in_the_keyring_is_refused() {
    let error = build_actor_seat_entry(PUBKEY, "", None, "wss://relay.example", None, None, None)
        .expect_err("an empty nsec must refuse the seat");
    assert!(error.contains("keyring"), "unexpected refusal: {error}");
    assert!(
        error.contains(PUBKEY),
        "refusal must name the agent: {error}"
    );
    // Whitespace is not a key either.
    assert!(
        build_actor_seat_entry(PUBKEY, "   ", None, "wss://relay.example", None, None, None)
            .is_err()
    );
}

#[test]
fn a_seat_pubkey_must_be_lowercase_hex() {
    assert!(
        build_actor_seat_entry("not-a-pubkey", "nsec1x", None, "wss://r", None, None, None)
            .is_err()
    );
    assert!(build_actor_seat_entry(
        &PUBKEY.to_uppercase(),
        "nsec1x",
        None,
        "wss://r",
        None,
        None,
        None
    )
    .is_err());
}

#[test]
fn the_file_shape_is_the_providers_read_contract() {
    let mut file = ActorSeatsFile::default();
    let entry = build_actor_seat_entry(
        PUBKEY,
        "nsec1secret",
        None,
        "wss://relay.example",
        None,
        None,
        None,
    )
    .expect("a hydrated key seats an agent");
    stage_actor_seat(&mut file, "csl-1234", entry).expect("stage");
    let json: serde_json::Value =
        serde_json::from_slice(&serde_json::to_vec(&file).expect("serialize")).expect("parse back");
    let pending = json.get("pending").expect("pending key");
    let seat = pending.get("csl-1234").expect("keyed by commandId");
    let keys: Vec<&str> = seat
        .as_object()
        .expect("seat object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, vec!["authTag", "nsec", "pubkey", "relayUrl"]);
    assert_eq!(seat.get("pubkey").and_then(|v| v.as_str()), Some(PUBKEY));
    assert_eq!(
        seat.get("nsec").and_then(|v| v.as_str()),
        Some("nsec1secret")
    );
    assert!(seat.get("authTag").expect("authTag present").is_null());
    assert_eq!(
        seat.get("relayUrl").and_then(|v| v.as_str()),
        Some("wss://relay.example")
    );
}

/// Ledger 77 (*Fence*, b): the provider turns this into the seat's `git`
/// author and committer, so a seated execution's commits are attributed to
/// the agent instead of to the operator whose home directory it runs in.
#[test]
fn a_seat_carries_the_agents_display_name_for_its_git_identity() {
    let entry = build_actor_seat_entry(
        PUBKEY,
        "nsec1secret",
        None,
        "wss://relay.example",
        Some("Levain"),
        None,
        None,
    )
    .expect("seat");
    let json = serde_json::to_value(&entry).expect("serialize");
    assert_eq!(
        json.get("displayName").and_then(|v| v.as_str()),
        Some("Levain"),
        "the provider reads displayName off this entry: {json}"
    );

    // Blank is absent, not a name made of spaces: an empty `git` author is
    // worse than falling back to the seat's role.
    let blank = build_actor_seat_entry(
        PUBKEY,
        "nsec1secret",
        None,
        "wss://relay.example",
        Some("   "),
        None,
        None,
    )
    .expect("seat");
    assert_eq!(blank.display_name, None);
    assert!(
        serde_json::to_value(&blank)
            .expect("serialize")
            .get("displayName")
            .is_none(),
        "a nameless seat writes no displayName key"
    );
}

#[test]
fn an_auth_tag_travels_verbatim() {
    let entry = build_actor_seat_entry(
        PUBKEY,
        "nsec1secret",
        Some("[\"tag\",\"value\"]"),
        "wss://relay.example",
        None,
        None,
        None,
    )
    .expect("seat");
    assert_eq!(entry.auth_tag.as_deref(), Some("[\"tag\",\"value\"]"));
}

#[test]
fn staging_needs_a_command_id() {
    let mut file = ActorSeatsFile::default();
    let entry = build_actor_seat_entry(PUBKEY, "nsec1secret", None, "wss://r", None, None, None)
        .expect("seat");
    assert!(stage_actor_seat(&mut file, "  ", entry.clone()).is_err());
    assert!(stage_actor_seat(&mut file, &"c".repeat(257), entry).is_err());
    assert!(file.pending.is_empty());
}

#[test]
fn clearing_reports_whether_the_provider_beat_us_to_it() {
    let mut file = ActorSeatsFile::default();
    let entry = build_actor_seat_entry(PUBKEY, "nsec1secret", None, "wss://r", None, None, None)
        .expect("seat");
    stage_actor_seat(&mut file, "csl-9", entry).expect("stage");
    assert!(clear_actor_seat(&mut file, "csl-9"));
    assert!(!clear_actor_seat(&mut file, "csl-9"));
}

#[test]
fn the_file_round_trips_through_disk_owner_only() {
    let dir = std::env::temp_dir().join(format!(
        "buzz-actor-seats-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = actor_seats_path(&dir);
    assert_eq!(
        read_actor_seats(&path).expect("missing is empty"),
        ActorSeatsFile::default()
    );
    let mut file = ActorSeatsFile::default();
    let entry = build_actor_seat_entry(PUBKEY, "nsec1secret", None, "wss://r", None, None, None)
        .expect("seat");
    stage_actor_seat(&mut file, "csl-7", entry).expect("stage");
    write_actor_seats(&path, &file).expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "the seat file must be owner-only");
    }
    assert_eq!(read_actor_seats(&path).expect("read back"), file);
    std::fs::remove_dir_all(&dir).ok();
}

/// Finding 72: the lead's seat is staged from the pack its agent was installed
/// from — on a dev machine, the checkout's `personas/roles/lead` — while this
/// build's shipped copy of the same pack lives under the desktop crate's
/// target directory. The staged entry must name that pack on the wire as the
/// shipped one, in the exact `packRef` shape the provider republishes on every
/// 44223 of the generation. Before this fix the installed arm produced `None`
/// here and the lead's status said nothing about its pack.
#[test]
fn the_leads_staged_seat_names_the_shipped_pack_its_installed_copy_is() {
    let tmp = tempfile::tempdir().expect("temp dir");
    // What `shipped_packs_dir(app)` answers on a dev build: tauri-build's copy.
    let shipped = tmp
        .path()
        .join("desktop/src-tauri/target/debug/personas/roles");
    // `named_role_pack` writes `<root>/<role>-pack`; the shipped tree and the
    // checkout both hold the role at `personas/roles/<role>`.
    let shipped_lead = shipped.join("lead");
    std::fs::rename(named_role_pack(&shipped, "lead"), &shipped_lead).expect("rename");
    // What the crew-role installer pointed the lead agent at: the checkout.
    let checkout_roles = tmp.path().join("checkout/personas/roles");
    let checkout_lead = checkout_roles.join("lead");
    std::fs::rename(named_role_pack(&checkout_roles, "lead"), &checkout_lead).expect("rename");
    let lead = installed_role_agent("lead", &checkout_lead);

    // The staging rule resolves the installed pack first.
    let (dir, persona) =
        resolve_local_seat_pack(&lead, std::slice::from_ref(&lead), &[], Some("lead"))
            .expect("the lead's own pack");
    assert_eq!(dir, checkout_lead);
    assert_eq!(persona, "lead");

    let (origin, pack_ref) = installed_seat_pack_ref(Some(&shipped), "0.5.16", &dir, Some("lead"));
    assert_eq!(origin, SeatPackOrigin::Shipped);
    let pack_ref = pack_ref.expect("the checkout copy is byte-for-byte the shipped lead pack");
    assert_eq!(pack_ref.repo, packs_cache::PACK_REF_SHIPPED_REPO);
    assert_eq!(pack_ref.sha, "0.5.16");
    assert_eq!(pack_ref.role, "lead");
    assert_eq!(pack_ref.path, "personas/roles/lead");
    assert_ne!(
        shipped_lead, dir,
        "the two directories differ by path; the recognition is by bytes"
    );

    // The entry the provider reads carries it verbatim.
    let entry = build_actor_seat_entry(
        PUBKEY,
        "nsec1secret",
        None,
        "wss://relay.example",
        Some("Keystone"),
        Some((dir, persona)),
        Some(pack_ref.clone()),
    )
    .expect("seat");
    let json = serde_json::to_value(&entry).expect("serialize");
    assert_eq!(
        json.get("packRef"),
        Some(&serde_json::json!({
            "repo": "app:shipped",
            "sha": "0.5.16",
            "role": "lead",
            "path": "personas/roles/lead",
        })),
        "{json}"
    );

    // An edited checkout is a pack nothing vouches for: origin stays
    // `installed`, and the wire says so by carrying no `packRef`.
    std::fs::write(
        checkout_lead.join("personas/lead.persona.md"),
        "---\nname: lead\nrole: lead\n---\nYou lead, differently.\n",
    )
    .expect("edit");
    let (origin, pack_ref) =
        installed_seat_pack_ref(Some(&shipped), "0.5.16", &checkout_lead, Some("lead"));
    assert_eq!(origin, SeatPackOrigin::Installed);
    assert_eq!(pack_ref, None);
}

// ── Finding 84: the project's pack source crosses the command boundary ────
//
// `stage_coding_session_actor_seat` takes the webview's decoded kind:30624
// as `pack_source: Option<ProjectPackSourceInput>` and hands it to
// `plan_seat_pack`; the plan's `pack_ref` then rides the custody entry
// `stage_actor_seat` files and the `StagedActorSeat` the webview is told.
// These pin both halves without an `AppHandle`: the wire shape the TS sends
// (`codingSessionActorSeatCustody.ts`, `packSource: { repo, gitRef, sha,
// path }`) and the entry-from-plan step the command runs.

#[test]
fn the_webviews_pack_source_reaches_the_planner_as_the_cache_reads_it() {
    // Exactly what `stageCodingSessionActorSeat` sends for a project that
    // follows a branch: `ref` renamed to `gitRef`, `sha`/`path` explicit nulls.
    let json = serde_json::json!({
        "repo": format!("30617:{PUBKEY}:agiterra-packs"),
        "gitRef": "refs/heads/main",
        "sha": null,
        "path": null,
    });
    let input: Option<ProjectPackSourceInput> =
        serde_json::from_value(json).expect("the TS shape deserializes");
    let source: packs_cache::ProjectPackSource = input.expect("present").into();
    assert_eq!(source.repo, format!("30617:{PUBKEY}:agiterra-packs"));
    assert_eq!(source.git_ref.as_deref(), Some("refs/heads/main"));
    assert_eq!(source.sha, None);
    assert_eq!(
        source.path,
        packs_cache::DEFAULT_PACK_PATH,
        "a null path is the default, not an empty string"
    );

    // A pinned source, with an explicit path, rides verbatim.
    let pinned: ProjectPackSourceInput = serde_json::from_value(serde_json::json!({
        "repo": format!("30617:{PUBKEY}:agiterra-packs"),
        "gitRef": null,
        "sha": "a".repeat(40),
        "path": "packs",
    }))
    .expect("pinned");
    let pinned: packs_cache::ProjectPackSource = pinned.into();
    assert_eq!(pinned.git_ref, None);
    assert_eq!(pinned.sha.as_deref(), Some("a".repeat(40).as_str()));
    assert_eq!(pinned.path, "packs");

    // No 30624 on the project: the webview sends `null`, the planner gets
    // `None`, and the local ladder answers — today's behaviour.
    let none: Option<ProjectPackSourceInput> =
        serde_json::from_value(serde_json::Value::Null).expect("null is None");
    assert!(none.is_none());
}

#[test]
fn a_planned_project_pack_is_what_stage_actor_seat_files_and_the_webview_is_told() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let dir = role_pack(tmp.path(), "builder");
    let mut record = agent_record(None, None);
    record.pubkey = PUBKEY.into();
    record.private_key_nsec = "nsec1secret".into();
    let pack_ref = packs_cache::PackRef {
        repo: format!("30617:{PUBKEY}:agiterra-packs"),
        sha: "dd935f43".repeat(5),
        role: "builder".into(),
        path: "personas/roles/builder".into(),
    };
    let plan = SeatPackPreview {
        pack_staged: true,
        origin: SeatPackOrigin::Project,
        role: Some("builder".into()),
        pack_dir: Some(dir.to_string_lossy().into_owned()),
        persona_id: Some("builder".into()),
        pack_ref: Some(pack_ref.clone()),
        refusal: None,
        reason: None,
        warnings: Vec::new(),
        compose_digest: None,
        source_kind: None,
        roles_visible: false,
    };

    let entry = seat_entry_for_plan(&record, "wss://relay.example", plan).expect("entry");
    let told = StagedActorSeat::of(&entry);
    let mut file = ActorSeatsFile::default();
    stage_actor_seat(&mut file, "csl-84", entry).expect("stage");

    // What the provider will read off the custody file…
    let filed = file
        .pending
        .get("csl-84")
        .expect("filed under the commandId");
    assert_eq!(filed.pack_dir.as_deref(), Some(dir.as_path()));
    assert_eq!(filed.pack_ref.as_ref(), Some(&pack_ref));
    // …is what the webview is told, in the camelCase it reads
    // (`StagedCodingSessionActorSeat.packRef`), so the screen names the same
    // commit the seat will actually run.
    assert!(told.pack_staged);
    let json = serde_json::to_value(&told).expect("serialize");
    assert_eq!(
        json.pointer("/packRef/repo").and_then(|v| v.as_str()),
        Some(pack_ref.repo.as_str()),
        "{json}"
    );
    assert_eq!(
        json.pointer("/packRef/sha").and_then(|v| v.as_str()),
        Some(pack_ref.sha.as_str())
    );
    assert_eq!(
        json.pointer("/packRef/path").and_then(|v| v.as_str()),
        Some("personas/roles/builder")
    );
}

#[test]
fn a_plan_that_refuses_stages_nothing_and_says_why() {
    let mut record = agent_record(None, None);
    record.pubkey = PUBKEY.into();
    record.private_key_nsec = "nsec1secret".into();
    let plan = SeatPackPreview {
        pack_staged: false,
        origin: SeatPackOrigin::None,
        role: Some("builder".into()),
        pack_dir: None,
        persona_id: None,
        pack_ref: None,
        refusal: Some(packs_cache::HIRE_PACK_UNAVAILABLE.to_string()),
        reason: Some("no builder directory at that commit".into()),
        warnings: Vec::new(),
        compose_digest: None,
        source_kind: None,
        roles_visible: false,
    };
    let error = seat_entry_for_plan(&record, "wss://relay.example", plan)
        .expect_err("a refused plan is not an entry");
    assert!(
        error.starts_with(packs_cache::HIRE_PACK_UNAVAILABLE),
        "{error}"
    );
    assert!(
        error.ends_with("(no builder directory at that commit)"),
        "{error}"
    );
}

#[test]
fn a_local_plan_is_told_with_no_pack_ref() {
    // The installed/shipped rungs answer with no repository to vouch for the
    // pack; the webview is told `packStaged` alone and must not invent one.
    let tmp = tempfile::tempdir().expect("temp dir");
    let dir = role_pack(tmp.path(), "builder");
    let mut record = agent_record(None, None);
    record.pubkey = PUBKEY.into();
    record.private_key_nsec = "nsec1secret".into();
    let plan = SeatPackPreview {
        pack_staged: true,
        origin: SeatPackOrigin::Installed,
        role: Some("builder".into()),
        pack_dir: Some(dir.to_string_lossy().into_owned()),
        persona_id: Some("builder".into()),
        pack_ref: None,
        refusal: None,
        reason: None,
        warnings: Vec::new(),
        compose_digest: None,
        source_kind: None,
        roles_visible: false,
    };
    let entry = seat_entry_for_plan(&record, "wss://relay.example", plan).expect("entry");
    let told = StagedActorSeat::of(&entry);
    assert!(told.pack_staged);
    let json = serde_json::to_value(&told).expect("serialize");
    assert!(json.get("packRef").is_none(), "{json}");
}

#[test]
fn concurrent_custody_mutations_keep_every_new_seat() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = actor_seats_path(dir.path());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
    let workers: Vec<_> = (0..16)
        .map(|index| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let entry = build_actor_seat_entry(
                    PUBKEY,
                    "test-secret",
                    None,
                    "wss://relay.test",
                    None,
                    None,
                    None,
                )
                .expect("entry");
                barrier.wait();
                mutate_actor_seats_file(&path, |file| {
                    stage_actor_seat(file, &format!("command-{index}"), entry)
                })
                .expect("atomic stage");
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("worker");
    }
    let file = read_actor_seats(&path).expect("read");
    assert_eq!(file.pending.len(), 16);
    for index in 0..16 {
        assert!(file.pending.contains_key(&format!("command-{index}")));
    }
}
