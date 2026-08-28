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
    install_role_packs, role_name_choices, role_profile_publishes, scan_role_packs,
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
