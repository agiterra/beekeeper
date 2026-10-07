use super::*;
use std::collections::BTreeMap;

const RELAY: &str = "wss://garden.example";

fn new_owner() -> String {
    nostr::Keys::generate().public_key().to_hex()
}

fn write_draft(root: &Path, owner: &str, relay: &str, slug: &str) -> ProjectTeamSetupDraft {
    super::super::super::tests::write_bound_draft(root, owner, relay, slug)
}

/// Every path under `root` with its modification time and file bytes.
fn fingerprint(root: &Path) -> BTreeMap<PathBuf, (std::time::SystemTime, Option<Vec<u8>>)> {
    let mut seen = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = std::fs::symlink_metadata(&path).expect("metadata");
        let bytes = if metadata.is_dir() {
            for entry in std::fs::read_dir(&path).expect("read dir") {
                pending.push(entry.expect("entry").path());
            }
            None
        } else {
            Some(std::fs::read(&path).expect("bytes"))
        };
        seen.insert(path, (metadata.modified().expect("mtime"), bytes));
    }
    seen
}

fn signed_placeholder() -> Event {
    EventBuilder::new(Kind::TextNote, "channel create")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign")
}

#[test]
fn listing_returns_bound_installations_and_skips_foreign_uninstalled_and_malformed() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("project-team-setup");
    let owner = new_owner();
    let commit = "c".repeat(40);
    let lead = "d".repeat(64);
    let builder = "e".repeat(64);

    let garden = write_draft(&root, &owner, RELAY, "garden");
    let garden_journal = super::super::tests::installed_journal(
        &garden,
        &commit,
        &[("lead", &lead), ("builder", &builder)],
        None,
    );
    save_journal(&garden, &garden_journal).expect("garden journal");

    // Same project, another owner; and this owner on another relay.
    let foreign = owner_scoped_installed(&root, &new_owner(), RELAY, "garden", &commit);
    let other_relay =
        owner_scoped_installed(&root, &owner, "wss://orchard.example", "garden", &commit);
    // A journal that never installed, and a draft with no journal at all.
    let orchard = write_draft(&root, &owner, RELAY, "orchard");
    let mut uninstalled = super::super::tests::installed_journal(&orchard, &commit, &[], None);
    uninstalled.installation = None;
    save_journal(&orchard, &uninstalled).expect("uninstalled journal");
    write_draft(&root, &owner, RELAY, "meadow");
    // A malformed journal and an in-flight preparation directory.
    let broken = write_draft(&root, &owner, RELAY, "broken");
    let broken_path = journal_path(&broken).expect("path");
    std::fs::write(&broken_path, b"{ not json").expect("broken journal");
    std::fs::create_dir_all(root.join(".preparing-00000000")).expect("preparing");
    std::fs::create_dir_all(root.join("not-a-scope")).expect("stray");

    let before = fingerprint(&root);
    let canonical_relay = crate::session_provider::canonical_relay_key(RELAY);
    let listed = list_installed_roles(&root, &owner, &canonical_relay).expect("listing");
    assert_eq!(before, fingerprint(&root), "listing must not write");

    assert_eq!(listed.len(), 1, "{listed:?}");
    let entry = &listed[0];
    assert_eq!(entry.project_ref, garden.project_ref);
    assert_eq!(entry.setup_id, garden.setup_id);
    assert_eq!(entry.publication_id, garden_journal.publication_id);
    assert_ne!(entry.setup_id, foreign.setup_id);
    assert_ne!(entry.setup_id, other_relay.setup_id);
    let json = serde_json::to_value(entry).expect("json");
    assert_eq!(
        json,
        serde_json::json!({
            "projectRef": garden.project_ref,
            "setupId": garden.setup_id,
            "publicationId": garden_journal.publication_id,
            "teamId": garden_journal.installation.as_ref().expect("installation").team_id,
            "source": {
                "repoRef": format!("30617:{owner}:garden-packs"),
                "sha": commit,
                "packPath": DEFAULT_PACK_PATH,
            },
            "leadChannelId": null,
            "roles": [
                {
                    "role": "lead",
                    "agentPubkey": lead,
                    "packRef": serde_json::to_value(&garden_journal.installation.as_ref().expect("installation").roles[0].pack_ref).expect("pack ref"),
                },
                {
                    "role": "builder",
                    "agentPubkey": builder,
                    "packRef": serde_json::to_value(&garden_journal.installation.as_ref().expect("installation").roles[1].pack_ref).expect("pack ref"),
                },
            ],
        })
    );
}

fn owner_scoped_installed(
    root: &Path,
    owner: &str,
    relay: &str,
    slug: &str,
    commit: &str,
) -> ProjectTeamSetupDraft {
    let draft = write_draft(root, owner, relay, slug);
    let journal =
        super::super::tests::installed_journal(&draft, commit, &[("lead", &"d".repeat(64))], None);
    save_journal(&draft, &journal).expect("journal");
    draft
}

#[test]
fn listing_lead_channel_prefers_lead_journal_then_reservation_and_hides_unadopted_source() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("project-team-setup");
    let owner = new_owner();
    let relay = crate::session_provider::canonical_relay_key(RELAY);
    let commit = "c".repeat(40);
    let lead = "d".repeat(64);
    let draft = write_draft(&root, &owner, RELAY, "garden");
    let mut journal =
        super::super::tests::installed_journal(&draft, &commit, &[("lead", &lead)], None);
    let reserved = uuid::Uuid::new_v4().to_string();
    if let Some(installation) = journal.installation.as_mut() {
        installation.channel = Some(activation::ActivationChannelJournal {
            channel_id: reserved.clone(),
            create_event: signed_placeholder(),
        });
    }
    save_journal(&draft, &journal).expect("reserved journal");
    let listed = list_installed_roles(&root, &owner, &relay).expect("listing");
    assert_eq!(
        listed[0].lead_channel_id.as_deref(),
        Some(reserved.as_str())
    );

    let chosen = uuid::Uuid::new_v4().to_string();
    journal.lead = Some(super::super::tests::ready_lead(&chosen, &lead));
    journal.status = PublicationStatus::Superseded;
    save_journal(&draft, &journal).expect("lead journal");
    let listed = list_installed_roles(&root, &owner, &relay).expect("listing");
    assert_eq!(listed[0].lead_channel_id.as_deref(), Some(chosen.as_str()));
    assert_eq!(listed[0].source, None);
    assert_eq!(listed[0].roles.len(), 1);
}

#[test]
fn listing_without_storage_is_empty_and_creates_nothing() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("project-team-setup");
    assert!(list_installed_roles(&root, &new_owner(), RELAY)
        .expect("listing")
        .is_empty());
    assert!(!root.exists());
}

fn journal_agent(pubkey: &str, home_role: &str) -> crate::managed_agents::ManagedAgentRecord {
    let mut record: crate::managed_agents::ManagedAgentRecord =
        serde_json::from_value(serde_json::json!({
            "pubkey": pubkey, "name": home_role, "relay_url": RELAY,
            "acp_command": "beekeeper-acp", "agent_command": "goose", "agent_args": [],
            "mcp_command": "", "turn_timeout_seconds": 320, "system_prompt": null,
            "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z",
            "last_started_at": null, "last_stopped_at": null,
            "last_exit_code": null, "last_error": null
        }))
        .expect("record");
    record.home_role = Some(home_role.to_string());
    record
}

/// The journal backfill reads this owner's installations on this relay and
/// associates only records that exist, are unassociated and hold the role;
/// running it twice changes nothing and a conflict is left untouched.
#[test]
fn journal_backfill_associates_installed_agents_idempotently() {
    use crate::managed_agents::project_agent_association::backfill_agents_from_journal_root;
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("project-team-setup");
    let owner = new_owner();
    let relay = crate::session_provider::canonical_relay_key(RELAY);
    let commit = "c".repeat(40);
    let (lead, builder, verifier, taken) = (
        "1".repeat(64),
        "2".repeat(64),
        "3".repeat(64),
        "4".repeat(64),
    );
    let garden = write_draft(&root, &owner, RELAY, "garden");
    let roles = [
        ("lead", lead.as_str()),
        ("builder", builder.as_str()),
        ("verifier", verifier.as_str()),
        ("runner", taken.as_str()),
        ("designer", &"5".repeat(64)),
    ];
    let journal = super::super::tests::installed_journal(&garden, &commit, &roles, None);
    save_journal(&garden, &journal).expect("journal");
    // Another owner's journal on the same relay is never read.
    owner_scoped_installed(&root, &new_owner(), RELAY, "elsewhere", &commit);

    let elsewhere = format!("30621:{}:elsewhere", "f".repeat(64));
    let mut taken_record = journal_agent(&taken, "runner");
    taken_record.project_ref = Some(elsewhere.clone());
    let bob = journal_agent(&"6".repeat(64), "builder");
    let mut agents = vec![
        journal_agent(&lead, "lead"),
        journal_agent(&builder, "builder"),
        journal_agent(&verifier, "builder"), // home role mismatch
        taken_record,
        bob,
    ];

    let first =
        backfill_agents_from_journal_root(&root, &owner, &relay, &mut agents).expect("backfill");
    assert_eq!(first.associated, vec![lead.clone(), builder.clone()]);
    assert_eq!(first.conflicts.len(), 1);
    assert_eq!(agents.len(), 5, "missing designer record is not created");
    assert_eq!(
        agents[0].project_ref.as_deref(),
        Some(garden.project_ref.as_str())
    );
    assert_eq!(
        agents[1].project_ref.as_deref(),
        Some(garden.project_ref.as_str())
    );
    assert_eq!(agents[2].project_ref, None, "home role mismatch is ignored");
    assert_eq!(agents[3].project_ref.as_deref(), Some(elsewhere.as_str()));
    assert_eq!(agents[4].project_ref, None, "Bob stays unassociated");

    let snapshot = agents.clone();
    let second =
        backfill_agents_from_journal_root(&root, &owner, &relay, &mut agents).expect("again");
    assert!(second.associated.is_empty());
    assert_eq!(agents, snapshot, "a second backfill is the same result");
}
