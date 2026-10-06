//! Tests for the Roles view's ladder walk.
//!
//! Split out of `role_packs_view.rs` to keep both files under the
//! repository's 1000-line ceiling; `use super::*` keeps every claim against
//! the same module it was written for.

use super::*;

const SHIPPED_VERSION: &str = "1.2.3";

/// `<parent>/<role>`: a pack whose persona declares `declared_role`,
/// claims `claimed`, and ships `skills` as `(name, SKILL.md body)`.
fn write_role_pack(
    parent: &Path,
    role: &str,
    declared_role: &str,
    claimed: &[&str],
    skills: &[(&str, &str)],
) -> PathBuf {
    let pack = parent.join(role);
    std::fs::create_dir_all(pack.join(".plugin")).expect("plugin dir");
    std::fs::create_dir_all(pack.join("personas")).expect("personas dir");
    std::fs::write(
        pack.join(".plugin/plugin.json"),
        format!(
            r#"{{"id":"com.test.{role}","name":"{role}","version":"0.9.0","personas":["personas/{role}.persona.md"]}}"#
        ),
    )
    .expect("plugin.json");
    let claims = if claimed.is_empty() {
        String::new()
    } else {
        let lines = claimed
            .iter()
            .map(|name| format!("  - skills/{name}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("skills:\n{lines}\n")
    };
    std::fs::write(
        pack.join(format!("personas/{role}.persona.md")),
        format!(
            "---\nname: {role}\ndisplay_name: The {role}\ndescription: Does {role} work.\nrole: {declared_role}\n{claims}---\n\nYou are the {role}.\nSecond line.\n\nNot the summary.\n"
        ),
    )
    .expect("persona");
    for (name, body) in skills {
        let dir = pack.join("skills").join(name);
        std::fs::create_dir_all(&dir).expect("skill dir");
        std::fs::write(dir.join("SKILL.md"), body).expect("SKILL.md");
    }
    pack
}

fn agent_record() -> ManagedAgentRecord {
    crate::managed_agents::AgentDefinition {
        id: "def".into(),
        display_name: "Agent".into(),
        avatar_url: None,
        system_prompt: String::new(),
        runtime: None,
        model: None,
        provider: None,
        name_pool: vec![],
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: None,
        source_team_persona_slug: None,
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

/// An installed crew-role agent: home role declared, pack linked.
fn installed_role_agent(role: &str, pack: Option<&Path>) -> ManagedAgentRecord {
    let mut record = agent_record();
    record.home_role = Some(role.to_owned());
    record.persona_team_dir = pack.map(Path::to_path_buf);
    record.persona_name_in_team = pack.map(|_| role.to_owned());
    record
}

/// Where a test's compositions are staged, and the catalog they resolve
/// against: one empty catalog, one scratch root per test. Both must outlive
/// the ladder that borrows them.
struct Staging {
    root: tempfile::TempDir,
    catalog: packs_cache::TemplateCatalog,
}

fn staging() -> Staging {
    Staging {
        root: tempfile::tempdir().expect("staging dir"),
        catalog: packs_cache::TemplateCatalog::empty(SHIPPED_VERSION),
    }
}

fn ladder<'a>(
    staging: &'a Staging,
    project: ProjectRung,
    checkout: Option<&'a Path>,
    records: &'a [ManagedAgentRecord],
    shipped_root: Option<&'a Path>,
) -> RolePackLadder<'a> {
    RolePackLadder {
        project,
        checkout,
        records,
        teams: &[],
        shipped_root,
        shipped_version: SHIPPED_VERSION,
        catalog: &staging.catalog,
        packs_root: staging.root.path(),
    }
}

/// A row's `pack_dir` is a staged copy under the packs root, keyed by the
/// source and holding the composer's provenance — never the source itself.
fn assert_staged_under(row: &RolePackSummary, staging: &Staging, source_key_prefix: &str) {
    let dir = Path::new(&row.pack_dir);
    let staged_root = packs_cache::staged_packs_root(staging.root.path());
    assert!(
        dir.starts_with(&staged_root),
        "{} is not under {}",
        row.pack_dir,
        staged_root.display()
    );
    let key = dir
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    assert!(
        key.starts_with(source_key_prefix),
        "source key {key:?} does not start with {source_key_prefix:?}"
    );
    assert!(
        dir.join("compose.json").is_file(),
        "no compose.json in {}",
        row.pack_dir
    );
    assert!(row
        .compose_digest
        .as_deref()
        .is_some_and(|d| d.starts_with("sha256:")));
}

fn shipped_ref(role: &str) -> PackRef {
    PackRef {
        repo: packs_cache::PACK_REF_SHIPPED_REPO.to_string(),
        sha: SHIPPED_VERSION.to_string(),
        role: role.to_string(),
        path: format!("{}/{role}", packs_cache::DEFAULT_PACK_PATH),
    }
}

#[test]
fn shipped_roles_are_sorted_by_slug_with_origin_ref_and_skills() {
    let staging = staging();
    let tmp = tempfile::tempdir().expect("temp dir");
    let shipped = tmp.path().join("shipped");
    write_role_pack(
        &shipped,
        "lead",
        "lead",
        &["write-brief"],
        &[
            (
                "write-brief",
                "---\nname: write-brief\ndescription: Write a locked brief.\n---\n",
            ),
            (
                "beekeeper-project",
                "---\nname: beekeeper-project\ndescription: The project.\n---\n",
            ),
        ],
    );
    write_role_pack(&shipped, "builder", "builder", &[], &[]);

    let rows = walk_role_pack_ladder(&ladder(
        &staging,
        ProjectRung::Absent,
        None,
        &[],
        Some(&shipped),
    ));

    let roles: Vec<&str> = rows.iter().map(|row| row.role.as_str()).collect();
    assert_eq!(roles, vec!["builder", "lead"]);

    let builder = &rows[0];
    assert_eq!(builder.origin, SeatPackOrigin::Shipped);
    assert_eq!(builder.pack_ref, Some(shipped_ref("builder")));
    assert_staged_under(builder, &staging, "app-");
    assert!(builder.skills.is_empty(), "{:?}", builder.skills);
    assert_eq!(builder.display_name, "The builder");
    assert_eq!(builder.description, "Does builder work.");
    assert_eq!(builder.summary, "You are the builder. Second line.");
    assert_eq!(builder.version.as_deref(), Some("0.9.0"));
    assert_eq!(builder.refusal, None);

    let lead = &rows[1];
    assert_eq!(lead.origin, SeatPackOrigin::Shipped);
    assert_eq!(lead.pack_ref, Some(shipped_ref("lead")));
    assert_eq!(lead.skills.len(), 2, "{:?}", lead.skills);
    let claimed = lead
        .skills
        .iter()
        .find(|skill| skill.name == "write-brief")
        .expect("claimed skill listed");
    assert!(!claimed.shared);
    assert_eq!(claimed.description, "Write a locked brief.");
    let shared = lead
        .skills
        .iter()
        .find(|skill| skill.name == "beekeeper-project")
        .expect("shared skill listed");
    assert!(shared.shared);
}

#[test]
fn a_session_checkout_outranks_the_shipped_pack_for_its_role_only() {
    let staging = staging();
    let tmp = tempfile::tempdir().expect("temp dir");
    let shipped = tmp.path().join("shipped");
    write_role_pack(&shipped, "lead", "lead", &[], &[]);
    write_role_pack(&shipped, "builder", "builder", &[], &[]);
    let checkout = tmp.path().join("checkout");
    let checkout_roles = checkout.join(packs_cache::DEFAULT_PACK_PATH);
    write_role_pack(&checkout_roles, "lead", "lead", &[], &[]);

    let rows = walk_role_pack_ladder(&ladder(
        &staging,
        ProjectRung::Absent,
        Some(&checkout),
        &[],
        Some(&shipped),
    ));

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].role, "builder");
    assert_eq!(rows[0].origin, SeatPackOrigin::Shipped);
    assert_eq!(rows[1].role, "lead");
    assert_eq!(rows[1].origin, SeatPackOrigin::Checkout);
    assert_eq!(
        rows[1].pack_ref, None,
        "no repository vouches for a checkout"
    );
    assert_staged_under(&rows[1], &staging, "local-");
}

#[test]
fn an_installed_pack_is_installed_unless_it_is_the_shipped_bytes() {
    let staging = staging();
    let tmp = tempfile::tempdir().expect("temp dir");
    let shipped = tmp.path().join("shipped");
    write_role_pack(&shipped, "lead", "lead", &[], &[]);
    let elsewhere = tmp.path().join("elsewhere");
    let verifier_pack = write_role_pack(&elsewhere, "verifier", "verifier", &[], &[]);
    let records = vec![
        installed_role_agent("verifier", Some(&verifier_pack)),
        // The lead's pack *is* the shipped directory: recognised, named.
        installed_role_agent("lead", Some(&shipped.join("lead"))),
    ];

    let rows = walk_role_pack_ladder(&ladder(
        &staging,
        ProjectRung::Absent,
        None,
        &records,
        Some(&shipped),
    ));

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].role, "lead");
    assert_eq!(rows[0].origin, SeatPackOrigin::Shipped);
    assert_eq!(rows[0].pack_ref, Some(shipped_ref("lead")));
    assert_eq!(rows[1].role, "verifier");
    assert_eq!(rows[1].origin, SeatPackOrigin::Installed);
    assert_eq!(
        rows[1].pack_ref, None,
        "nothing on the wire names a local pack"
    );
    assert_staged_under(&rows[1], &staging, "local-");
    assert_staged_under(&rows[0], &staging, "app-");
}

#[test]
fn an_agent_with_a_home_role_and_no_pack_contributes_no_row() {
    let staging = staging();
    let tmp = tempfile::tempdir().expect("temp dir");
    let shipped = tmp.path().join("shipped");
    write_role_pack(&shipped, "lead", "lead", &[], &[]);
    let records = vec![installed_role_agent("poker", None)];

    let rows = walk_role_pack_ladder(&ladder(
        &staging,
        ProjectRung::Absent,
        None,
        &records,
        Some(&shipped),
    ));

    let roles: Vec<&str> = rows.iter().map(|row| row.role.as_str()).collect();
    assert_eq!(roles, vec!["lead"], "a home role is not a pack");
}

#[test]
fn a_directory_named_for_a_role_its_persona_does_not_declare_is_not_that_role() {
    let staging = staging();
    let tmp = tempfile::tempdir().expect("temp dir");
    let shipped = tmp.path().join("shipped");
    // `runner/` whose persona says it is the lead.
    write_role_pack(&shipped, "runner", "lead", &[], &[]);

    let rows = walk_role_pack_ladder(&ladder(
        &staging,
        ProjectRung::Absent,
        None,
        &[],
        Some(&shipped),
    ));

    assert!(rows.is_empty(), "{rows:?}");
}

#[test]
fn the_project_rung_wins_and_roles_it_lacks_carry_the_refusal() {
    let staging = staging();
    let tmp = tempfile::tempdir().expect("temp dir");
    let shipped = tmp.path().join("shipped");
    write_role_pack(&shipped, "lead", "lead", &[], &[]);
    write_role_pack(&shipped, "builder", "builder", &[], &[]);
    let repo_checkout = tmp.path().join("packs-cache").join("deadbeef-packs");
    let path = packs_cache::DEFAULT_PACK_PATH.to_string();
    write_role_pack(&repo_checkout.join(&path), "lead", "lead", &[], &[]);
    let sha = "0123456789abcdef0123456789abcdef01234567".to_string();
    let repo =
        "30617:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:packs".to_string();

    let rows = walk_role_pack_ladder(&ladder(
        &staging,
        ProjectRung::Synced {
            repo: repo.clone(),
            checkout: repo_checkout.clone(),
            path: path.clone(),
            sha: sha.clone(),
        },
        None,
        &[],
        Some(&shipped),
    ));

    assert_eq!(rows.len(), 2);
    let builder = &rows[0];
    assert_eq!(builder.role, "builder");
    assert_eq!(
        builder.origin,
        SeatPackOrigin::Shipped,
        "found here, refused there"
    );
    let refusal = builder.refusal.as_deref().expect("a refusal");
    assert!(
        refusal.starts_with(packs_cache::HIRE_PACK_UNAVAILABLE),
        "{refusal}"
    );
    assert_eq!(
        refusal,
        project_refusal(&packs_cache::missing_role_pack_reason(
            &sha, &path, "builder"
        ))
    );
    let lead = &rows[1];
    assert_eq!(lead.role, "lead");
    assert_eq!(lead.origin, SeatPackOrigin::Project);
    assert_eq!(lead.refusal, None);
    assert_eq!(
        lead.pack_ref,
        Some(PackRef {
            repo,
            sha,
            role: "lead".into(),
            path: format!("{path}/lead"),
        })
    );
    // The row's directory is the composed, staged copy under the packs root
    // keyed by the repository and commit — never the checkout the sync
    // moves (spec § 4.5).
    assert_staged_under(lead, &staging, "deadbeef-packs-");
    assert!(
        !lead
            .pack_dir
            .starts_with(&repo_checkout.to_string_lossy().into_owned()),
        "{}",
        lead.pack_dir
    );
}

#[test]
fn an_unavailable_project_source_refuses_every_role() {
    let staging = staging();
    let tmp = tempfile::tempdir().expect("temp dir");
    let shipped = tmp.path().join("shipped");
    write_role_pack(&shipped, "lead", "lead", &[], &[]);
    write_role_pack(&shipped, "builder", "builder", &[], &[]);

    let rows = walk_role_pack_ladder(&ladder(
        &staging,
        ProjectRung::Unavailable {
            reason: "the packs repository does not contain commit abc".into(),
        },
        None,
        &[],
        Some(&shipped),
    ));

    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(
            row.refusal.as_deref(),
            Some(
                format!(
                    "{} (the packs repository does not contain commit abc)",
                    packs_cache::HIRE_PACK_UNAVAILABLE
                )
                .as_str()
            ),
            "{}",
            row.role
        );
        assert_eq!(row.origin, SeatPackOrigin::Shipped);
    }
}

#[test]
fn an_unavailable_source_with_nothing_local_is_an_error_not_an_empty_catalog() {
    let staging = staging();
    let err = rows_or_refusal(&ladder(
        &staging,
        ProjectRung::Unavailable {
            reason: "the packs repository does not contain commit abc".into(),
        },
        None,
        &[],
        None,
    ))
    .expect_err("nothing to show and a reason to give");
    assert!(err.starts_with(packs_cache::HIRE_PACK_UNAVAILABLE), "{err}");
    assert!(
        err.contains("the packs repository does not contain commit abc"),
        "{err}"
    );
    // With something local the rows carry the refusal and the call is Ok.
    let tmp = tempfile::tempdir().expect("temp dir");
    let shipped = tmp.path().join("shipped");
    write_role_pack(&shipped, "lead", "lead", &[], &[]);
    let rows = rows_or_refusal(&ladder(
        &staging,
        ProjectRung::Unavailable {
            reason: "boom".into(),
        },
        None,
        &[],
        Some(&shipped),
    ))
    .expect("rows with refusals");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].refusal.as_deref(),
        Some(project_refusal("boom").as_str())
    );
}

#[test]
fn the_wire_shape_is_camel_case_with_exactly_these_keys() {
    let row = RolePackSummary {
        role: "lead".into(),
        display_name: "The lead".into(),
        description: "Leads.".into(),
        summary: "You lead.".into(),
        version: Some("0.9.0".into()),
        origin: SeatPackOrigin::Shipped,
        pack_dir: "/packs/lead".into(),
        pack_ref: Some(shipped_ref("lead")),
        skills: vec![RolePackSkill {
            name: "write-brief".into(),
            description: "Write a brief.".into(),
            shared: false,
        }],
        refusal: None,
        warnings: vec!["beekeeper/memory@^1.0.0 resolved to 1.0.0, which is deprecated".into()],
        compose_digest: Some("sha256:abc".into()),
        agents_repo: packs_cache::AgentsRepoAccess::None,
        archived: false,
    };
    let json = serde_json::to_value(&row).expect("serializes");
    let keys = |value: &serde_json::Value| -> Vec<String> {
        value
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect()
    };
    let mut top = keys(&json);
    top.sort();
    assert_eq!(
        top,
        [
            "agentsRepo",
            "archived",
            "composeDigest",
            "description",
            "displayName",
            "origin",
            "packDir",
            "packRef",
            "refusal",
            "role",
            "skills",
            "summary",
            "version",
            "warnings"
        ]
    );
    assert_eq!(json["origin"], "shipped");
    assert_eq!(json["refusal"], serde_json::Value::Null);
    let mut skill = keys(&json["skills"][0]);
    skill.sort();
    assert_eq!(skill, ["description", "name", "shared"]);
    let mut pack_ref = keys(&json["packRef"]);
    pack_ref.sort();
    assert_eq!(pack_ref, ["path", "repo", "role", "sha"]);
    let origins: Vec<serde_json::Value> = [
        SeatPackOrigin::Project,
        SeatPackOrigin::Checkout,
        SeatPackOrigin::Installed,
        SeatPackOrigin::Shipped,
    ]
    .iter()
    .map(|origin| serde_json::to_value(origin).expect("serializes"))
    .collect();
    assert_eq!(origins, ["project", "checkout", "installed", "shipped"]);
}

#[test]
fn no_rungs_means_no_rows() {
    let staging = staging();
    let rows = walk_role_pack_ladder(&ladder(&staging, ProjectRung::Absent, None, &[], None));
    assert!(rows.is_empty());
}

#[test]
fn summarize_prompt_takes_the_first_paragraph() {
    assert_eq!(
        summarize_prompt("\n\nYou are the lead.\nFive verbs.\n\nSecond paragraph.\n"),
        "You are the lead. Five verbs."
    );
    assert_eq!(summarize_prompt(""), "");
    assert_eq!(summarize_prompt("\n  \n"), "");
}

#[test]
fn summarize_prompt_cuts_at_the_ceiling() {
    let long = "x".repeat(SUMMARY_MAX_CHARS + 50);
    let summary = summarize_prompt(&long);
    assert_eq!(summary.chars().count(), SUMMARY_MAX_CHARS);
    assert!(summary.ends_with('…'));
    let exact = "y".repeat(SUMMARY_MAX_CHARS);
    assert_eq!(summarize_prompt(&exact), exact);
}

#[test]
fn newest_project_pack_source_picks_the_latest_valid_record() {
    use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
    let keys = Keys::generate();
    let project = "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:demo";
    let repo_old = "30617:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:old";
    let repo_new = "30617:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:new";
    let build = |repo: &str, at: u64, pin: (&str, &str)| {
        EventBuilder::new(
            Kind::Custom(beekeeper_core_pkg::kind::KIND_PROJECT_PACK_SOURCE as u16),
            r#"{"schema":"buzz-project-pack-source/v1"}"#,
        )
        .tags(vec![
            Tag::parse(vec!["d".to_string(), project.to_string()]).expect("d"),
            Tag::parse(vec!["repo".to_string(), repo.to_string()]).expect("repo"),
            Tag::parse(vec![pin.0.to_string(), pin.1.to_string()]).expect("pin"),
        ])
        .custom_created_at(Timestamp::from(at))
        .sign_with_keys(&keys)
        .expect("signed")
    };
    let malformed = EventBuilder::new(
        Kind::Custom(beekeeper_core_pkg::kind::KIND_PROJECT_PACK_SOURCE as u16),
        "not this schema",
    )
    .custom_created_at(Timestamp::from(9_999))
    .sign_with_keys(&keys)
    .expect("signed");
    let events = vec![
        build(repo_old, 100, ("ref", "refs/heads/main")),
        build(
            repo_new,
            200,
            ("sha", "0123456789abcdef0123456789abcdef01234567"),
        ),
        malformed,
    ];

    let source = newest_project_pack_source(&events).expect("a source");
    assert_eq!(source.repo, repo_new);
    assert_eq!(source.git_ref, None);
    assert_eq!(
        source.sha.as_deref(),
        Some("0123456789abcdef0123456789abcdef01234567")
    );
    assert_eq!(source.path, packs_cache::DEFAULT_PACK_PATH);
    assert_eq!(newest_project_pack_source(&[]), None);
}
