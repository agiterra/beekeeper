//! The role install pins the definition it binds (C4 run 4, ledger 399).

use super::*;
use crate::managed_agents::persona_events::{
    persona_content_hash, persona_d_tag, persona_event_content,
};

/// A role agent's id (`crew-role:<slug>`) is not its NIP-AP coordinate, so the
/// private-persona guard admits it only with an affirmative pin
/// (`host_private_persona.rs`). An unpinned role agent can never be enrolled or
/// reconfigured, and readiness reports its pack `SourceUnknown`. The install
/// is the binding, so it pins the content it writes; a refresh re-pins an
/// identity installed before this fix without minting or moving it.
#[test]
fn an_install_pins_the_role_definition_it_binds() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "builder", "builder", Some("builder"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");
    let pinned = |result: &CrewRoleInstall| {
        let agent = &result.agents[0];
        let definition = result
            .definitions
            .iter()
            .find(|def| Some(def.id.as_str()) == agent.persona_id.as_deref())
            .expect("the agent's definition");
        assert_ne!(persona_d_tag(definition), definition.id);
        let hash = persona_content_hash(&persona_event_content(definition));
        assert_eq!(agent.persona_source_version.as_deref(), Some(hash.as_str()));
        assert_eq!(hash, role_pack_source_version(&scan.packs[0], &agent.name));
        (agent.pubkey.clone(), hash)
    };

    let first = install(&scan, Vec::new(), Vec::new(), &[]);
    let (pubkey, hash) = pinned(&first);

    let mut legacy = first.agents.clone();
    legacy[0].persona_source_version = None;
    legacy[0].runtime = Some("claude".into());
    let refreshed = install(
        &scan,
        first.definitions.clone(),
        legacy,
        std::slice::from_ref(&first.team),
    );
    assert_eq!(pinned(&refreshed), (pubkey, hash));
    assert_eq!(minted_count(&refreshed), 0);
    assert_eq!(refreshed.agents[0].runtime.as_deref(), Some("claude"));
}
