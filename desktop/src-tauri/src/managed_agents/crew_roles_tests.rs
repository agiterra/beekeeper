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
    install_role_packs(scan, definitions, agents, teams, NOW, &mut mint).expect("install succeeds")
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
    assert_eq!(
        roles,
        vec!["lead", "architect", "builder", "verifier", "runner"]
    );
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
            !matches!(row.role.as_str(), "poker" | "designer")
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
