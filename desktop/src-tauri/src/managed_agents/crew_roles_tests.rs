//! Tests for the crew-role installer.
//!
//! Every one of these pins a claim the front-door spec makes: the scan is one
//! level deep, a pack with no role is reported rather than dropped, a second
//! run refreshes instead of duplicating, the minted agents actually resolve a
//! seat pack (the bug this lane exists for), the team never points at the
//! operator's checkout, and the crew is in launch order with the lead first.

use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::managed_agents::types::{AgentDefinition, ManagedAgentRecord};

const NOW: &str = "2026-08-27T00:00:00Z";

/// Write a minimal pack: manifest, one persona, and (optionally) a role.
fn write_pack(root: &Path, dir_name: &str, persona: &str, role: Option<&str>) -> PathBuf {
    let dir = root.join(dir_name);
    fs::create_dir_all(dir.join(".plugin")).expect("pack .plugin dir");
    fs::create_dir_all(dir.join("personas")).expect("pack personas dir");
    fs::write(
        dir.join(".plugin").join("plugin.json"),
        format!(
            r#"{{"id":"com.test.{persona}","name":"{persona}","version":"0.1.0","personas":["personas/{persona}.persona.md"]}}"#
        ),
    )
    .expect("plugin.json");
    let role_line = match role {
        Some(role) => format!("role: {role}\n"),
        None => String::new(),
    };
    fs::write(
        dir.join("personas").join(format!("{persona}.persona.md")),
        format!(
            "---\nname: {persona}\n{role_line}display_name: \"{persona}\"\ndescription: \"the {persona} persona\"\n---\n\nYou are {persona}.\n"
        ),
    )
    .expect("persona file");
    dir
}

/// A helper building the role→name map the installer takes.
fn names(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
    pairs
        .iter()
        .map(|(role, name)| ((*role).to_string(), (*name).to_string()))
        .collect()
}

/// A mint that hands out deterministic, distinguishable identities.
fn counting_mint(minted: &mut usize) -> impl FnMut() -> Result<MintedCrewIdentity, String> + '_ {
    move || {
        *minted += 1;
        let seed = *minted;
        Ok(MintedCrewIdentity {
            pubkey: format!("{seed:0>64}"),
            private_key_nsec: format!("nsec1test{seed}"),
            auth_tag: Some(format!("[\"auth\",\"{seed}\"]")),
        })
    }
}

/// How many rows this install minted rather than refreshed.
fn minted_count(result: &CrewRoleInstall) -> usize {
    result.installed.iter().filter(|row| !row.refreshed).count()
}

fn install(
    scan: &RolePackScan,
    definitions: Vec<AgentDefinition>,
    agents: Vec<ManagedAgentRecord>,
    teams: &[crate::managed_agents::TeamRecord],
) -> CrewRoleInstall {
    let mut minted = 0usize;
    let mut mint = counting_mint(&mut minted);
    install_role_packs(
        scan,
        definitions,
        agents,
        teams,
        NOW,
        &names(&[]),
        &mut mint,
    )
    .expect("install succeeds")
}

/// Depth 1 only, and a child that is not a role pack is reported — never
/// silently dropped, which would leave the operator guessing why a folder they
/// pointed at produced fewer agents than it holds directories.
#[test]
fn install_scans_only_immediate_children_and_skips_packs_with_no_role() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
    write_pack(root.path(), "plain", "plain", None);
    let nested = root.path().join("nested");
    fs::create_dir_all(&nested).expect("nested dir");
    write_pack(&nested, "builder", "builder", Some("builder"));
    fs::write(root.path().join("notes.txt"), "not a pack").expect("stray file");

    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let roles: Vec<&str> = scan.packs.iter().map(|p| p.role.as_str()).collect();
    assert_eq!(roles, vec!["lead"], "only the depth-1 role pack installs");

    let skipped: Vec<&str> = scan.skipped.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(skipped.len(), 2, "both non-pack directories are reported");
    assert!(skipped.iter().any(|path| path.ends_with("plain")));
    assert!(skipped.iter().any(|path| path.ends_with("nested")));
    assert!(scan.skipped.iter().all(|s| s.reason == NO_ROLE_SKIP_REASON));
    assert!(
        !skipped.iter().any(|path| path.ends_with("notes.txt")),
        "a file cannot hold a .plugin/plugin.json and is not reported as a pack"
    );
}

/// A second run over the same folder refreshes the same agents. Duplicating
/// them would mint a second key per role and leave two agents fighting over one
/// seat.
#[test]
fn install_is_idempotent_and_refreshes_rather_than_duplicating() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
    write_pack(root.path(), "builder", "builder", Some("builder"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let first = install(&scan, Vec::new(), Vec::new(), &[]);
    assert_eq!(first.agents.len(), 2);
    assert_eq!(
        minted_count(&first),
        2,
        "both agents are minted on a first run"
    );
    assert!(first.installed.iter().all(|row| !row.refreshed));

    let second = install(
        &scan,
        first.definitions.clone(),
        first.agents.clone(),
        std::slice::from_ref(&first.team),
    );

    assert_eq!(second.agents.len(), 2, "no duplicate agents");
    assert_eq!(second.definitions.len(), 2, "no duplicate definitions");
    assert_eq!(minted_count(&second), 0, "a refresh mints no keys");
    assert!(second.installed.iter().all(|row| row.refreshed));
    assert_eq!(second.team.id, first.team.id, "the same team is updated");
    assert_eq!(
        second.team.crew, first.team.crew,
        "the same crew comes back"
    );
    let before: Vec<&str> = first.agents.iter().map(|a| a.pubkey.as_str()).collect();
    let after: Vec<&str> = second.agents.iter().map(|a| a.pubkey.as_str()).collect();
    assert_eq!(before, after, "the same identities survive");
}

/// The bug this lane exists for: before it, `resolve_seat_pack` returned `None`
/// for every agent on this computer, so every seat staged without its pack.
#[test]
fn installed_agents_resolve_a_seat_pack() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
    write_pack(root.path(), "verifier", "verifier", Some("verifier"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let result = install(&scan, Vec::new(), Vec::new(), &[]);
    let teams = vec![result.team.clone()];

    assert_eq!(result.agents.len(), 2);
    for agent in &result.agents {
        let pack = crate::managed_agents::actor_seats::resolve_seat_pack(agent, &teams);
        assert!(
            pack.is_some(),
            "agent {} stages no role pack — this is the bug",
            agent.name
        );
        assert!(
            agent.home_role.is_some(),
            "and it carries the role the pack declared"
        );
    }
}

/// `delete_team_with_cascade` does `fs::remove_dir_all(source_dir)`. A crew
/// roles team pointed at `personas/roles` would delete the operator's checkout
/// on "Delete team".
#[test]
fn the_crew_roles_team_has_no_source_dir_so_delete_cannot_remove_the_packs() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let result = install(&scan, Vec::new(), Vec::new(), &[]);

    assert_eq!(result.team.source_dir, None);
    assert!(!result.team.is_symlink);
    assert_eq!(result.team.symlink_target, None);
    // The pack link lives on the agent instead.
    assert_eq!(
        result.agents[0].persona_team_dir.as_deref(),
        Some(scan.packs[0].dir.as_path())
    );
}

/// Launch order is the roster's order, not the order the folder happened to
/// list, and the first turn goes to the lead. Poker and designer install as
/// agents but take no seat.
#[test]
fn the_installed_team_carries_a_crew_in_launch_order_with_the_lead_primary() {
    let root = tempfile::tempdir().expect("temp dir");
    for role in [
        "verifier",
        "designer",
        "builder",
        "lead",
        "runner",
        "poker",
        "architect",
    ] {
        write_pack(root.path(), role, role, Some(role));
    }
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let result = install(&scan, Vec::new(), Vec::new(), &[]);
    let crew = result.team.crew.expect("the team is a crew");

    let roles: Vec<&str> = crew.seats.iter().map(|seat| seat.role.as_str()).collect();
    assert_eq!(roles, vec!["lead", "architect", "builder", "runner"]);
    let lead = result
        .installed
        .iter()
        .find(|row| row.role == "lead")
        .expect("lead installed");
    assert_eq!(crew.primary, lead.persona_id);
    assert_eq!(result.agents.len(), 7, "poker and designer install too");
    for row in &result.installed {
        assert_eq!(
            row.seated,
            !matches!(row.role.as_str(), "poker" | "designer" | "verifier")
        );
    }
}

/// A roster role with no pack is dropped from the seats. Seating it with a
/// persona id that does not exist would make every launch of this crew fail on
/// a seat nobody can fill.
#[test]
fn a_roster_role_with_no_pack_is_dropped_from_the_seats_not_seated_with_a_missing_persona() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
    write_pack(root.path(), "builder", "builder", Some("builder"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let result = install(&scan, Vec::new(), Vec::new(), &[]);
    let crew = result.team.crew.expect("the team is a crew");

    let roles: Vec<&str> = crew.seats.iter().map(|seat| seat.role.as_str()).collect();
    assert_eq!(roles, vec!["lead", "builder"]);
    let known: Vec<&str> = result
        .definitions
        .iter()
        .map(|def| def.id.as_str())
        .collect();
    for seat in &crew.seats {
        assert!(
            known.contains(&seat.persona_id.as_str()),
            "seat {} names a persona that does not exist",
            seat.role
        );
    }
}

/// A folder that cannot be read is a disclosure, not an empty result.
#[test]
fn an_unreadable_folder_is_an_error_not_an_empty_scan() {
    let root = tempfile::tempdir().expect("temp dir");
    let missing = root.path().join("does-not-exist");
    assert!(scan_role_packs(&missing).is_err());
}

/// The repo's own `personas/roles` — the folder an operator actually points
/// the installer at — installs seven agents, seats four of them in launch
/// order, and every minted agent resolves its pack.
///
/// This is the acceptance case run against the real packs rather than a
/// fixture: a scan that passes on hand-written fixtures and fails on the
/// shipped packs would be a green test over a broken front door.
#[test]
fn the_repo_role_packs_install_seven_agents_and_seat_four() {
    let roles_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("personas")
        .join("roles");
    let Ok(roles_dir) = std::fs::canonicalize(&roles_dir) else {
        // A checkout without the packs is not a failure of this code.
        return;
    };

    let scan = scan_role_packs(&roles_dir).expect("the repo's role packs scan");
    let roles: Vec<&str> = scan.packs.iter().map(|p| p.role.as_str()).collect();
    assert_eq!(
        roles,
        vec![
            "lead",
            "architect",
            "builder",
            "verifier",
            "runner",
            "poker",
            "designer"
        ],
        "install order is the roster, then the two unseated roles"
    );
    assert!(scan.skipped.is_empty(), "no shipped pack is skipped");

    let result = install(&scan, Vec::new(), Vec::new(), &[]);
    assert_eq!(result.agents.len(), 7);
    let teams = vec![result.team.clone()];
    for agent in &result.agents {
        assert!(
            crate::managed_agents::actor_seats::resolve_seat_pack(agent, &teams).is_some(),
            "agent {} would stage no role pack",
            agent.name
        );
    }

    let crew = result.team.crew.clone().expect("the team is a crew");
    assert_eq!(
        crew.seats.len(),
        4,
        "four seats: poker, designer and verifier install unseated"
    );
    assert_eq!(
        crew.seats
            .iter()
            .map(|seat| seat.role.as_str())
            .collect::<Vec<_>>(),
        vec!["lead", "architect", "builder", "runner"]
    );
    let lead = result
        .installed
        .iter()
        .find(|row| row.role == "lead")
        .expect("the lead pack installed");
    assert_eq!(
        crew.primary, lead.persona_id,
        "the lead takes the first turn"
    );

    // And every seat names a persona this install actually minted.
    for seat in &crew.seats {
        assert!(
            result
                .agents
                .iter()
                .any(|agent| agent.persona_id.as_deref() == Some(seat.persona_id.as_str())),
            "seat {} names a persona no agent fills",
            seat.role
        );
    }
}

/// A partial install has to report the seats it actually wrote **and** the
/// roster roles it dropped.
///
/// Without both lists the dialog can only print a constant roster, which is
/// what it did: "Seated by default: lead, architect, builder, verifier,
/// runner." under a result list holding no verifier (SESSION_STATE item 76,
/// poke finding F2).
#[test]
fn an_install_reports_the_seats_it_wrote_and_the_roster_roles_it_dropped() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
    write_pack(root.path(), "builder", "builder", Some("builder"));
    write_pack(root.path(), "poker", "poker", Some("poker"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let result = install(&scan, Vec::new(), Vec::new(), &[]);

    assert_eq!(
        result.seated,
        vec!["lead".to_string(), "builder".to_string()],
        "seated names the seats the crew actually holds, in seat order"
    );
    assert_eq!(
        result.dropped,
        vec!["architect".to_string(), "runner".to_string()],
        "a roster role with no installed pack is reported as dropped"
    );
    let poker = result
        .installed
        .iter()
        .find(|row| row.role == "poker")
        .expect("the poker pack installed");
    assert!(!poker.seated, "poker installs but is never seated");
}

/// A full roster drops nothing, and the unseated packs are not "dropped" —
/// they were never on the roster to begin with.
#[test]
fn a_full_roster_drops_nothing() {
    let root = tempfile::tempdir().expect("temp dir");
    for role in CREW_SEAT_ROSTER {
        write_pack(root.path(), role, role, Some(role));
    }
    write_pack(root.path(), "designer", "designer", Some("designer"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let result = install(&scan, Vec::new(), Vec::new(), &[]);

    assert_eq!(result.seated, CREW_SEAT_ROSTER.map(str::to_string).to_vec());
    assert!(result.dropped.is_empty());
}

/// A mint that fails is a **key** failure, not a folder one.
///
/// The dialog wrapped every failure in "That folder could not be read:", so an
/// operator with a locked keychain was sent to look at their folder
/// (SESSION_STATE item 76, poke finding F3). The stage has to travel with the
/// error for the dialog to say anything else.
#[test]
fn a_mint_failure_is_reported_as_a_key_failure_not_a_folder_one() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let mut mint = || Err("the keychain is locked".to_string());
    let error = install_role_packs(
        &scan,
        Vec::new(),
        Vec::new(),
        &[],
        NOW,
        &names(&[]),
        &mut mint,
    )
    .expect_err("a mint failure aborts the install");

    assert_eq!(error.failure, CrewRoleInstallFailure::Keys);
    assert_eq!(error.detail, "the keychain is locked");
    assert!(
        !error.detail.contains("folder"),
        "the detail must not mention the folder"
    );
}

/// The failure stage is on the wire as a plain lowercase word the dialog
/// switches on — not a sentence it has to pattern-match.
#[test]
fn the_failure_stage_serialises_as_a_word() {
    let error = CrewRoleInstallError {
        failure: CrewRoleInstallFailure::Store,
        detail: "disk full".to_string(),
    };
    let json = serde_json::to_value(&error).expect("serialises");
    assert_eq!(json["failure"], "store");
    assert_eq!(json["detail"], "disk full");
}

/// F7 (SESSION_STATE item 77): every seat the installer wrote read "vendor not
/// declared" in the Team tab, and the D8 family rule refused the launch on it.
/// The seat now carries the runtime a launch from this computer runs it on, and
/// the vendor that runtime can only be.
#[test]
fn every_seat_declares_the_runtime_and_vendor_it_will_launch_on() {
    let root = tempfile::tempdir().expect("temp dir");
    for role in ["lead", "builder", "verifier"] {
        write_pack(root.path(), role, role, Some(role));
    }
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let result = install(&scan, Vec::new(), Vec::new(), &[]);
    let crew = result.team.crew.expect("the team is a crew");

    assert!(!crew.seats.is_empty());
    for seat in &crew.seats {
        assert_eq!(
            seat.driver.as_deref(),
            Some(DEFAULT_CREW_SEAT_DRIVER),
            "seat {} names no runtime",
            seat.role
        );
        assert_eq!(
            seat.vendor.as_deref(),
            Some("anthropic"),
            "seat {} declares no vendor, so the family rule cannot read it",
            seat.role
        );
    }
}

/// D11: a lead is an identity a person names once. The installer used to name
/// every identity after its role, so the lead was always "Lead".
#[test]
fn the_lead_is_minted_under_the_name_the_operator_gave() {
    let root = tempfile::tempdir().expect("temp dir");
    for role in ["lead", "builder"] {
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
        &names(&[("lead", "Keystone")]),
        &mut mint,
    )
    .expect("install succeeds");

    let lead = result
        .installed
        .iter()
        .find(|row| row.role == "lead")
        .expect("lead installed");
    assert_eq!(lead.agent_name, "Keystone");
    assert!(
        result.agents.iter().any(|agent| agent.name == "Keystone"),
        "the minted record carries the given name"
    );
    let builder = result
        .installed
        .iter()
        .find(|row| row.role == "builder")
        .expect("builder installed");
    assert_eq!(
        builder.agent_name, "builder",
        "only the lead is renamed; the rest keep their role names"
    );
}

/// The default roster seats no verifier.
///
/// Every seat of one launch is created against the one `providerInstanceRef`
/// the dialog selected, so every seat runs on that runtime's vendor — and D8
/// refuses a verifier sharing a builder's vendor. A seated verifier therefore
/// made the installed roster unlaunchable by construction (F7). The pack still
/// installs, unseated, for a roster launched across two providers.
#[test]
fn the_default_roster_installs_a_verifier_without_seating_it() {
    let root = tempfile::tempdir().expect("temp dir");
    for role in ["lead", "architect", "builder", "verifier", "runner"] {
        write_pack(root.path(), role, role, Some(role));
    }
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let result = install(&scan, Vec::new(), Vec::new(), &[]);

    assert_eq!(
        result.seated,
        vec![
            "lead".to_string(),
            "architect".to_string(),
            "builder".to_string(),
            "runner".to_string()
        ]
    );
    assert!(
        result.dropped.is_empty(),
        "nothing was dropped for want of a pack"
    );
    let verifier = result
        .installed
        .iter()
        .find(|row| row.role == "verifier")
        .expect("the verifier pack installs");
    assert!(!verifier.seated);
}

/// A team minted before the rename is renamed in place, not duplicated.
#[test]
fn a_team_installed_under_the_old_name_is_updated_rather_than_duplicated() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");

    let first = install(&scan, Vec::new(), Vec::new(), &[]);
    let mut legacy = first.team.clone();
    legacy.name = LEGACY_CREW_ROLES_TEAM_NAME.to_string();

    let second = install(
        &scan,
        first.definitions.clone(),
        first.agents.clone(),
        std::slice::from_ref(&legacy),
    );

    assert_eq!(second.team.id, legacy.id, "the same team is updated");
    assert_eq!(second.team.name, CREW_ROLES_TEAM_NAME);
}

/// Item 79(a): the Agents grid read `Lead` over the identity the operator
/// named `Keystone`, because the installer reused Fizz's persona card and left
/// its display name alone. The card an install writes is the identity's card;
/// it carries the identity's name or the grid tells the operator about an
/// agent that does not exist.
#[test]
fn the_persona_card_an_install_writes_carries_the_identity_name() {
    let root = tempfile::tempdir().expect("temp dir");
    for role in ["lead", "builder"] {
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
        &names(&[("lead", "Keystone")]),
        &mut mint,
    )
    .expect("install succeeds");

    let lead = result
        .installed
        .iter()
        .find(|row| row.role == "lead")
        .expect("lead installed");
    let card = result
        .definitions
        .iter()
        .find(|def| def.id == lead.persona_id)
        .expect("the lead's persona card");
    assert_eq!(
        card.display_name, "Keystone",
        "the card is titled after the identity the install minted"
    );
    assert!(
        !result
            .definitions
            .iter()
            .any(|def| def.display_name == "lead"),
        "no card is left titled after the role pack the lead came from"
    );
    // Every other role keeps its pack's name, and its card agrees with it.
    let builder = result
        .installed
        .iter()
        .find(|row| row.role == "builder")
        .expect("builder installed");
    let builder_card = result
        .definitions
        .iter()
        .find(|def| def.id == builder.persona_id)
        .expect("the builder's persona card");
    assert_eq!(builder_card.display_name, builder.agent_name);
}

/// A second install over an already-named lead renames the card it reuses.
#[test]
fn a_refresh_renames_the_card_it_reuses_after_the_identity() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
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
    assert_eq!(first.definitions[0].display_name, "lead");

    let mut mint = counting_mint(&mut minted);
    let second = install_role_packs(
        &scan,
        first.definitions.clone(),
        first.agents.clone(),
        std::slice::from_ref(&first.team),
        NOW,
        &names(&[("lead", "Keystone")]),
        &mut mint,
    )
    .expect("install succeeds");
    assert_eq!(second.definitions.len(), 1, "the card is reused, not added");
    assert_eq!(second.definitions[0].display_name, "Keystone");
    assert_eq!(second.agents[0].name, "Keystone");
}

/// Ledger 80 (e): the session header read "Fizz · Lead" over an identity the
/// installer had just renamed, because the seat's name comes from the
/// identity's kind:0 relay profile and the install never republished it. A
/// rename must produce a profile publish carrying the new name.
#[test]
fn renaming_the_lead_owes_a_profile_publish_with_the_new_name() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
    write_pack(root.path(), "builder", "builder", Some("builder"));
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
            &names(&[("lead", "Fizz")]),
            &mut mint,
        )
        .expect("install succeeds")
    };
    // A first install owes a publish for every identity it minted: none of
    // them has a profile on the relay yet.
    let first_publishes = role_profile_publishes(&[], &first);
    assert_eq!(first_publishes.len(), first.installed.len());
    assert!(
        first_publishes
            .iter()
            .all(|publish| publish.previous_name.is_none()),
        "a minted identity has no previous name"
    );

    let mut mint = counting_mint(&mut minted);
    let second = install_role_packs(
        &scan,
        first.definitions.clone(),
        first.agents.clone(),
        std::slice::from_ref(&first.team),
        NOW,
        &names(&[("lead", "Keystone")]),
        &mut mint,
    )
    .expect("install succeeds");

    let publishes = role_profile_publishes(&first.agents, &second);
    let lead_pubkey = second
        .installed
        .iter()
        .find(|row| row.role == "lead")
        .expect("lead installed")
        .agent_pubkey
        .clone();
    let lead = publishes
        .iter()
        .find(|publish| publish.pubkey == lead_pubkey)
        .expect("the renamed lead owes a profile publish");
    assert_eq!(
        lead.display_name, "Keystone",
        "the publish must carry the name the operator gave, not the pack's"
    );
    assert_eq!(lead.previous_name.as_deref(), Some("Fizz"));

    // The seats that were not renamed are still republished — a refreshed
    // identity may carry a profile from an install whose publish failed — and
    // they carry their own unchanged name, never the lead's.
    let builder_pubkey = second
        .installed
        .iter()
        .find(|row| row.role == "builder")
        .expect("builder installed")
        .agent_pubkey
        .clone();
    let builder = publishes
        .iter()
        .find(|publish| publish.pubkey == builder_pubkey)
        .expect("the builder owes a profile publish too");
    assert_eq!(builder.display_name, "builder");
    assert_eq!(builder.previous_name.as_deref(), Some("builder"));
}

// ── The project's own role packs (ledger 85) ─────────────────────────────────
//
// The installer opened on "No folder chosen", so a new operator had to know
// that a project's packs live in `<checkout>/personas/roles`. These pin the
// read-only look the dialog takes before anybody clicks anything: where it
// looks, and the three answers it can come back with — a folder that is not
// there, a folder holding nothing, and a folder that scans to exactly the
// fields the picker path would have rendered.

/// A project's role packs live in `personas/roles` under its checkout.
#[test]
fn a_projects_role_packs_live_under_personas_roles_in_its_checkout() {
    let checkout = tempfile::tempdir().expect("temp dir");
    assert_eq!(
        project_role_packs_dir(checkout.path()),
        checkout.path().join("personas").join("roles"),
    );
}

/// A checkout without that folder is a fact the dialog states, not an error
/// it blames the operator for — and the answer still names the folder it
/// looked in, so the sentence on screen can say where.
#[test]
fn a_checkout_with_no_personas_roles_folder_is_reported_missing_not_failed() {
    let checkout = tempfile::tempdir().expect("temp dir");

    let scan =
        scan_project_role_packs(checkout.path(), &[]).expect("a missing folder is not a failure");

    assert!(
        !scan.exists,
        "the folder is not there, and the scan says so"
    );
    assert!(scan.packs.is_empty());
    assert!(scan.skipped.is_empty());
    assert_eq!(
        scan.directory,
        project_role_packs_dir(checkout.path())
            .display()
            .to_string(),
    );
}

/// A folder that is there and holds nothing is a different fact from a folder
/// that is not there — the dialog says which, so `exists` cannot be inferred
/// from an empty pack list.
#[test]
fn an_empty_personas_roles_folder_exists_and_holds_no_packs() {
    let checkout = tempfile::tempdir().expect("temp dir");
    fs::create_dir_all(project_role_packs_dir(checkout.path())).expect("roles dir");

    let scan = scan_project_role_packs(checkout.path(), &[]).expect("scan succeeds");

    assert!(scan.exists, "the folder is there");
    assert!(scan.packs.is_empty(), "and it holds nothing");
}

/// The pre-chosen folder is the *same* scan the picker runs: same rows, same
/// order, same defaults, same skip list. A second code path here would let the
/// two disagree about what the folder holds.
#[test]
fn a_projects_roles_folder_scans_to_exactly_what_the_picker_would_have_shown() {
    let checkout = tempfile::tempdir().expect("temp dir");
    let roles = project_role_packs_dir(checkout.path());
    fs::create_dir_all(&roles).expect("roles dir");
    write_pack(&roles, "lead", "lead", Some("lead"));
    write_pack(&roles, "designer", "designer", Some("designer"));
    write_pack(&roles, "notes", "notes", None);
    // The names an operator already gave, read back the way the dialog reads
    // them: off this computer's agent list, not off the packs.
    let installed = {
        let mut minted = 0usize;
        let mut mint = counting_mint(&mut minted);
        install_role_packs(
            &scan_role_packs(&roles).expect("first scan"),
            Vec::new(),
            Vec::new(),
            &[],
            NOW,
            &names(&[("lead", "Keystone")]),
            &mut mint,
        )
        .expect("install succeeds")
    };
    let agents = installed.agents.clone();

    let scan = scan_project_role_packs(checkout.path(), &agents).expect("scan succeeds");

    assert!(scan.exists);
    let picker = role_name_choices(&scan_role_packs(&roles).expect("picker scan"), &agents);
    assert_eq!(scan.packs, picker, "one scan, not two that can disagree");
    assert_eq!(
        scan.packs
            .iter()
            .map(|pack| pack.default_name.as_str())
            .collect::<Vec<_>>(),
        vec!["Keystone", "designer"],
        "a pack already installed here defaults to that identity's name",
    );
    assert_eq!(scan.skipped.len(), 1, "the child with no role is reported");
}

/// Naming every installed identity, and what a rename owes the relay. Split
/// into its own file only to keep both under the repository file-size gate.
#[path = "crew_roles_naming_tests.rs"]
mod naming;

/// Item 90: model, provider, runtime and avatar are host-owned install facts.
/// A refresh (including one that renames in place) rebuilds the instance off
/// the pack definition, and role packs are model-agnostic by design — so
/// without an explicit carry-over the reinstall wipes whatever the operator
/// seated the identity on.
#[test]
fn a_refresh_preserves_the_host_owned_model_provider_runtime_and_avatar() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
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

    // The host seats the identity on a specific runtime/model and gives it a
    // custom avatar — exactly what happened by hand at 23:17.
    let mut seated = first.agents.clone();
    seated[0].model = Some("gpt-5.6-sol".into());
    seated[0].provider = Some("openai".into());
    seated[0].runtime = Some("codex".into());
    seated[0].avatar_url = Some("https://example.com/banksy.png".into());

    let mut mint = counting_mint(&mut minted);
    let second = install_role_packs(
        &scan,
        first.definitions.clone(),
        seated,
        std::slice::from_ref(&first.team),
        NOW,
        &names(&[("lead", "Keystone")]),
        &mut mint,
    )
    .expect("install succeeds");

    assert_eq!(second.agents[0].name, "Keystone", "the rename still lands");
    assert_eq!(
        second.agents[0].model.as_deref(),
        Some("gpt-5.6-sol"),
        "a reinstall must not wipe the model the host seated"
    );
    assert_eq!(second.agents[0].provider.as_deref(), Some("openai"));
    assert_eq!(
        second.agents[0].runtime.as_deref(),
        Some("codex"),
        "a reinstall must not wipe the runtime the host seated"
    );
    assert_eq!(
        second.agents[0].avatar_url.as_deref(),
        Some("https://example.com/banksy.png"),
        "a reinstall must not wipe the avatar the host chose"
    );
}
