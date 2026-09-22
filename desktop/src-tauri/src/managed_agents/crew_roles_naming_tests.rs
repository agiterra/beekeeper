//! Naming every identity the installer writes, not only the lead.
//!
//! Ledger 84: the dialog asked for one name — the lead's — so the designer
//! identity a person addresses as "Banksy" was `designer` on this computer and
//! `designer` on the relay. These pin the role→name map, D11's "minted once"
//! rule when a name is typed over an identity that already exists, and the
//! kind:0 publish a rename owes (ledger 80 (e)).
//!
//! A sibling of `crew_roles_tests.rs` rather than more of it: that file was at
//! the repository's 1000-line ceiling. Helpers come from the parent test
//! module.

use super::*;
use crate::managed_agents::crew_roles::{
    install_role_packs, install_role_packs_in_named_team, role_name_choices,
    role_profile_publishes, scan_role_packs, NameScope,
};

// ── Naming every identity, not just the lead (ledger 84) ─────────────────────
//
// The installer asked for one name — the lead's — so the designer identity a
// person addresses as "Banksy" was called `designer` on this computer and
// `designer` on the relay. The dialog now asks a name per role pack it found,
// and the install carries a role→name map.

/// Every named role is minted under the name it was given; a role with no
/// entry in the map keeps its pack's own name.
#[test]
fn every_named_role_is_minted_under_the_name_the_operator_gave() {
    let root = tempfile::tempdir().expect("temp dir");
    for role in ["lead", "designer", "builder"] {
        write_pack(root.path(), role, role, Some(role));
    }
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let mut minted = 0usize;
    let mut mint = counting_mint(&mut minted);
    let result = install_role_packs(
        &scan,
        Vec::new(),
        Vec::new(),
        &[],
        NOW,
        &names(&[("lead", "Keystone"), ("designer", "Banksy")]),
        &mut mint,
    )
    .expect("install succeeds");

    let named = |role: &str| {
        result
            .installed
            .iter()
            .find(|row| row.role == role)
            .unwrap_or_else(|| panic!("{role} installed"))
            .agent_name
            .clone()
    };
    assert_eq!(named("lead"), "Keystone");
    assert_eq!(named("designer"), "Banksy");
    assert_eq!(
        named("builder"),
        "builder",
        "a role the operator left alone keeps its pack's name"
    );
    assert!(
        result.agents.iter().any(|agent| agent.name == "Banksy"),
        "the minted record carries the given name"
    );
    assert!(
        result.installed.iter().all(|row| !row.renamed),
        "a freshly minted identity was named, not renamed"
    );
}

/// D11's "minted once" rule: a name typed over an already-installed identity
/// renames *that* identity — its record and its persona card — and never mints
/// a second one beside it.
#[test]
fn a_new_name_renames_the_installed_identity_in_place_and_mints_nothing() {
    let root = tempfile::tempdir().expect("temp dir");
    for role in ["lead", "designer"] {
        write_pack(root.path(), role, role, Some(role));
    }
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let mut minted = 0usize;
    let first = {
        let mut mint = counting_mint(&mut minted);
        install_role_packs(
            &scan,
            Vec::new(),
            Vec::new(),
            &[],
            NOW,
            &names(&[]),
            &mut mint,
        )
        .expect("install succeeds")
    };
    let designer_pubkey = first
        .installed
        .iter()
        .find(|row| row.role == "designer")
        .expect("designer installed")
        .agent_pubkey
        .clone();
    assert_eq!(minted, 2, "the first run mints one identity per pack");

    let mut mint = counting_mint(&mut minted);
    let second = install_role_packs(
        &scan,
        first.definitions.clone(),
        first.agents.clone(),
        std::slice::from_ref(&first.team),
        NOW,
        &names(&[("designer", "Banksy")]),
        &mut mint,
    )
    .expect("install succeeds");
    drop(mint);

    assert_eq!(minted, 2, "a rename mints no key");
    assert_eq!(second.agents.len(), 2, "no second designer appeared");
    let designer = second
        .installed
        .iter()
        .find(|row| row.role == "designer")
        .expect("designer installed");
    assert_eq!(designer.agent_pubkey, designer_pubkey, "same identity");
    assert!(designer.refreshed);
    assert!(designer.renamed, "the row has to say it was renamed");
    let record = second
        .agents
        .iter()
        .find(|agent| agent.pubkey == designer_pubkey)
        .expect("the designer record");
    assert_eq!(record.name, "Banksy");
    let card = second
        .definitions
        .iter()
        .find(|def| Some(def.id.as_str()) == record.persona_id.as_deref())
        .expect("the designer's persona card");
    assert_eq!(
        card.display_name, "Banksy",
        "the persona card is renamed with the identity, not left on the old name"
    );

    let lead = second
        .installed
        .iter()
        .find(|row| row.role == "lead")
        .expect("lead installed");
    assert!(
        !lead.renamed,
        "an identity nobody renamed is not reported as renamed"
    );
}

/// Ledger 80 (e) for every seat, not only the lead: renaming the designer to
/// "Banksy" owes a kind:0 publish carrying that name, or the relay — and every
/// session header reading it — still says `designer`.
#[test]
fn renaming_the_designer_owes_a_profile_publish_with_the_new_name() {
    let root = tempfile::tempdir().expect("temp dir");
    for role in ["lead", "designer"] {
        write_pack(root.path(), role, role, Some(role));
    }
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let mut minted = 0usize;
    let first = {
        let mut mint = counting_mint(&mut minted);
        install_role_packs(
            &scan,
            Vec::new(),
            Vec::new(),
            &[],
            NOW,
            &names(&[("lead", "Keystone")]),
            &mut mint,
        )
        .expect("install succeeds")
    };

    let mut mint = counting_mint(&mut minted);
    let second = install_role_packs(
        &scan,
        first.definitions.clone(),
        first.agents.clone(),
        std::slice::from_ref(&first.team),
        NOW,
        &names(&[("lead", "Keystone"), ("designer", "Banksy")]),
        &mut mint,
    )
    .expect("install succeeds");

    let publishes = role_profile_publishes(&first.agents, &second);
    let designer_pubkey = second
        .installed
        .iter()
        .find(|row| row.role == "designer")
        .expect("designer installed")
        .agent_pubkey
        .clone();
    let designer = publishes
        .iter()
        .find(|publish| publish.pubkey == designer_pubkey)
        .expect("the renamed designer owes a profile publish");
    assert_eq!(designer.display_name, "Banksy");
    assert_eq!(
        designer.previous_name.as_deref(),
        Some("designer"),
        "the publish knows the name the relay currently carries"
    );
}

/// The field list the dialog renders comes off the scan: one row per pack, the
/// lead first, defaulting to the name that identity already carries here — so
/// re-running the installer over a named team offers "Keystone", not "lead",
/// and leaving every field alone renames nobody.
#[test]
fn the_name_fields_come_off_the_scan_with_the_lead_first() {
    let root = tempfile::tempdir().expect("temp dir");
    for role in ["designer", "lead", "builder"] {
        write_pack(root.path(), role, role, Some(role));
    }
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let fresh = role_name_choices(&scan, &[]);
    assert_eq!(
        fresh
            .iter()
            .map(|choice| choice.role.as_str())
            .collect::<Vec<_>>(),
        vec!["lead", "builder", "designer"],
        "the lead's row is first, then the roster, then the unseated roles"
    );
    assert!(
        fresh.iter().all(|choice| !choice.installed),
        "nothing is installed yet"
    );
    assert_eq!(fresh[0].default_name, "lead");

    let mut minted = 0usize;
    let mut mint = counting_mint(&mut minted);
    let installed = install_role_packs(
        &scan,
        Vec::new(),
        Vec::new(),
        &[],
        NOW,
        &names(&[("lead", "Keystone")]),
        &mut mint,
    )
    .expect("install succeeds");

    let again = role_name_choices(&scan, &installed.agents);
    assert_eq!(again[0].role, "lead");
    assert_eq!(
        again[0].default_name, "Keystone",
        "the field offers the name this identity already carries"
    );
    assert!(again[0].installed);
    assert_eq!(again[0].persona_name, "lead");
    assert!(
        again.iter().all(|choice| !choice.pack_dir.is_empty()),
        "each row names the pack it came from"
    );
}

// ── Names are unique per project, not per computer (ledger 207 → 246) ────────
//
// Until 2026-09-22 `mint_agent_name` scanned every managed agent on the
// computer, so the second project to install a `builder` got "Builder 2" and
// the third "Builder 3". Those numbers recorded nothing but the order in which
// projects happened to be created on one laptop. A project is the namespace
// now; the reserved every-project namespace is an explicit flag on a record and
// nothing else.

/// A well-formed project coordinate for `slug`.
fn project(slug: &str) -> String {
    format!("30621:{}:{slug}", "ab".repeat(32))
}

/// What `associate_installation` writes after the installer returns.
fn associated(records: &[ManagedAgentRecord], project_ref: &str) -> Vec<ManagedAgentRecord> {
    records
        .iter()
        .cloned()
        .map(|mut record| {
            record.project_ref = Some(project_ref.to_string());
            record
        })
        .collect()
}

fn install_builder_for(
    scan: &crate::managed_agents::crew_roles::RolePackScan,
    definitions: Vec<AgentDefinition>,
    agents: Vec<ManagedAgentRecord>,
    slug: &str,
    minted: &mut usize,
) -> crate::managed_agents::crew_roles::CrewRoleInstall {
    let mut mint = counting_mint(minted);
    install_role_packs_in_named_team(
        scan,
        definitions,
        agents,
        &[],
        NOW,
        &names(&[("builder", "Builder")]),
        &mut mint,
        format!("team-{slug}"),
        &format!("Project team {}", project(slug)),
        NameScope::Project(project(slug)),
    )
    .expect("install succeeds")
}

/// The headline: two projects may each have a plain "Builder".
#[test]
fn a_second_project_gets_a_builder_not_a_builder_2() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "builder", "builder", Some("builder"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let mut minted = 0usize;
    let alpha = install_builder_for(&scan, Vec::new(), Vec::new(), "alpha", &mut minted);
    assert_eq!(alpha.agents[0].name, "Builder");

    let beta = install_builder_for(
        &scan,
        alpha.definitions.clone(),
        associated(&alpha.agents, &project("alpha")),
        "beta",
        &mut minted,
    );
    assert_eq!(minted, 2, "the second project mints its own identity");

    let beta_pubkey = &beta
        .installed
        .iter()
        .find(|row| row.role == "builder")
        .expect("builder installed")
        .agent_pubkey;
    let record = beta
        .agents
        .iter()
        .find(|agent| &agent.pubkey == beta_pubkey)
        .expect("beta's builder record");
    assert_eq!(
        record.name, "Builder",
        "alpha's Builder is in another project's namespace and does not push beta's to `Builder 2`"
    );
}

/// The rule still holds inside one project: a role a second identity already
/// fills does get a suffix, because there the name really would be ambiguous.
#[test]
fn a_second_identity_in_the_same_project_still_gets_a_suffix() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "builder", "builder", Some("builder"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let mut minted = 0usize;
    let first = install_builder_for(&scan, Vec::new(), Vec::new(), "alpha", &mut minted);

    // A second builder in the *same* project, minted from somewhere else, so
    // the installer cannot adopt it and has to name beside it.
    let mut agents = associated(&first.agents, &project("alpha"));
    let mut twin = agents[0].clone();
    twin.pubkey = "f".repeat(64);
    // A different team, so `existing_agent_for_team` cannot adopt it and the
    // install has to mint beside it — but the same project, so it is in the
    // namespace the new name must be unique inside.
    twin.team_id = Some("team-elsewhere".to_string());
    twin.persona_team_dir = Some(std::path::PathBuf::from("/somewhere/else"));
    twin.persona_name_in_team = Some("elsewhere".to_string());
    twin.persona_id = Some("elsewhere".to_string());
    agents.push(twin);
    // Drop the adoptable original so this install must mint rather than refresh.
    agents.remove(0);

    let second = install_builder_for(
        &scan,
        first.definitions.clone(),
        agents,
        "alpha",
        &mut minted,
    );
    let fresh = second
        .installed
        .iter()
        .find(|row| row.role == "builder")
        .expect("builder installed");
    assert_eq!(
        fresh.agent_name, "Builder 2",
        "one project holding two builders is exactly when a number means something"
    );
}

/// `project_ref == None` is the no-project bucket, **not** a claim on every
/// project. A pre-pivot machine is full of unassociated records; if one of them
/// reserved its name, no project could ever mint a `Builder` again.
#[test]
fn an_agent_with_no_project_reserves_nothing_outside_its_own_bucket() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "builder", "builder", Some("builder"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let mut minted = 0usize;
    let legacy = install_builder_for(&scan, Vec::new(), Vec::new(), "alpha", &mut minted);
    // The legacy shape: installed long ago, never associated with a project.
    let unscoped = legacy.agents.clone();
    assert!(unscoped[0].project_ref.is_none());
    assert!(!unscoped[0].reserves_name_globally);

    let alpha = install_builder_for(
        &scan,
        legacy.definitions.clone(),
        unscoped,
        "alpha",
        &mut minted,
    );
    let record = alpha
        .agents
        .iter()
        .find(|agent| agent.name == "Builder")
        .expect("a plain Builder");
    assert_eq!(record.name, "Builder");
}

/// The reserved namespace, which is the whole reason the flag exists. Nothing
/// sets it today; this is what it will do when something does.
#[test]
fn a_globally_reserved_name_is_refused_to_every_project() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "builder", "builder", Some("builder"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let mut minted = 0usize;
    let seeded = install_builder_for(&scan, Vec::new(), Vec::new(), "alpha", &mut minted);
    let mut reserved = seeded.agents.clone();
    reserved[0].project_ref = None;
    reserved[0].reserves_name_globally = true;

    let beta = install_builder_for(
        &scan,
        seeded.definitions.clone(),
        reserved,
        "beta",
        &mut minted,
    );
    let fresh = beta
        .installed
        .iter()
        .find(|row| row.role == "builder")
        .expect("builder installed");
    assert_eq!(
        fresh.agent_name, "Builder 2",
        "a reserved name is taken in every namespace, whatever project asks for it"
    );
}

/// Case is folded on both sides. Every other name resolver in this app folds
/// it, so `builder` and `Builder` are one name and not two.
#[test]
fn a_name_differing_only_in_case_is_the_same_name() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "builder", "builder", Some("builder"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let mut minted = 0usize;
    let first = install_builder_for(&scan, Vec::new(), Vec::new(), "alpha", &mut minted);
    let mut agents = associated(&first.agents, &project("alpha"));
    agents[0].name = "builder".to_string();
    let mut twin = agents[0].clone();
    twin.pubkey = "f".repeat(64);
    // A different team, so `existing_agent_for_team` cannot adopt it and the
    // install has to mint beside it — but the same project, so it is in the
    // namespace the new name must be unique inside.
    twin.team_id = Some("team-elsewhere".to_string());
    twin.persona_team_dir = Some(std::path::PathBuf::from("/somewhere/else"));
    twin.persona_name_in_team = Some("elsewhere".to_string());
    twin.persona_id = Some("elsewhere".to_string());
    agents.push(twin);
    agents.remove(0);

    let second = install_builder_for(
        &scan,
        first.definitions.clone(),
        agents,
        "alpha",
        &mut minted,
    );
    let fresh = second
        .installed
        .iter()
        .find(|row| row.role == "builder")
        .expect("builder installed");
    assert_eq!(fresh.agent_name, "Builder 2");
}

/// Two roles of one project asked for the same name in the same run.
///
/// The record minted a moment ago has no `project_ref` yet —
/// `associate_installation` writes it after the installer returns — so without
/// `minted_here` the second role would read the first as belonging to no
/// project, find its name free, and both would be installed as "Twin".
#[test]
fn two_roles_of_one_project_named_alike_in_one_run_do_not_collide() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "builder", "builder", Some("builder"));
    write_pack(root.path(), "runner", "runner", Some("runner"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let mut minted = 0usize;
    let mut mint = counting_mint(&mut minted);
    let install = install_role_packs_in_named_team(
        &scan,
        Vec::new(),
        Vec::new(),
        &[],
        NOW,
        &names(&[("builder", "Twin"), ("runner", "Twin")]),
        &mut mint,
        "team-alpha".to_string(),
        &format!("Project team {}", project("alpha")),
        NameScope::Project(project("alpha")),
    )
    .expect("install succeeds");
    drop(mint);

    let mut given: Vec<&str> = install
        .installed
        .iter()
        .map(|row| row.agent_name.as_str())
        .collect();
    given.sort_unstable();
    assert_eq!(
        given,
        vec!["Twin", "Twin 2"],
        "the second identity of the same project has to be told apart from the first"
    );
}
