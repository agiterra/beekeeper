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
    install_role_packs(scan, definitions, agents, teams, NOW, None, &mut mint)
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
    let error = install_role_packs(&scan, Vec::new(), Vec::new(), &[], NOW, None, &mut mint)
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
        Some("Keystone"),
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
