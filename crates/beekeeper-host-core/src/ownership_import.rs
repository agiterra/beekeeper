//! Compare-and-swap for one verified legacy backfill representation change.
use super::*;

/// Rebind an unadmitted, quiesced legacy import to its proven owner definition head.
///
/// The caller must hold the pair admission lock, verify the current signed owner
/// head and the exact B5 semantic conversion, and establish that no shared create
/// operation has been journaled and no provider reserved/consumed/uncertain
/// admission or retained actor request exists. A protected provider snapshot is
/// required for that affirmative readback; target absence is never sufficient. This function grants no relay authority and
/// performs no process or runtime work. It accepts only the exact staged import
/// (operation, actor, canonical relay, legacy nonce and old revision), preserves
/// history, and supports an identical retry after its atomic write.
pub fn revise_quiesced_import_definition(
    home: &Path,
    instance: Instance,
    expected: &Transfer,
    verified_revision: &str,
) -> Result<(), String> {
    nostr::PublicKey::from_hex(&expected.actor_pubkey).map_err(|_| "invalid import actor")?;
    nostr::EventId::from_hex(&expected.definition_event_id)
        .map_err(|_| "invalid staged import revision")?;
    nostr::EventId::from_hex(verified_revision).map_err(|_| "invalid verified import revision")?;
    if expected.phase != Phase::Quiesced
        || expected.target.is_some()
        || expected.operation_id.trim().is_empty()
        || expected.legacy_nonce.trim().is_empty()
        || expected.legacy_nonce.starts_with("fresh-identity:")
    {
        return Err("definition rebind requires an unadmitted quiesced legacy import".into());
    }
    let mut expected = expected.clone();
    expected.actor_pubkey.make_ascii_lowercase();
    expected.relay_url = canonical_relay_key(&expected.relay_url);
    url::Url::parse(&expected.relay_url).map_err(|_| "invalid import relay")?;
    let _journal_lock = journal_lock(home, instance)?;
    let mut journal = read(home, instance)?;
    let index = journal
        .transfers
        .iter()
        .position(|current| {
            current.actor_pubkey == expected.actor_pubkey && current.relay_url == expected.relay_url
        })
        .ok_or_else(|| "staged legacy import is missing".to_owned())?;
    let current = &journal.transfers[index];
    let mut next = expected.clone();
    next.definition_event_id = verified_revision.to_owned();
    // Idempotence is exact, including nonce, phase and target; it cannot turn a
    // different import or already accepted runtime into a retry of this CAS.
    if *current == next {
        return Ok(());
    }
    if *current != expected {
        return Err(
            "quiesced import definition conflicts with preserved custody or revision".into(),
        );
    }
    journal.history.push(current.clone());
    journal.transfers[index] = next;
    atomic_write::atomic_write_json_restricted(
        &layout::host_dir(home, instance).join("managed-ownership.json"),
        &serde_json::to_vec_pretty(&journal).map_err(|error| error.to_string())?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn staged(home: &Path) -> Transfer {
        let mut transfer = Transfer {
            operation_id: "import-one".into(),
            actor_pubkey: "ab".repeat(32),
            relay_url: "wss://one.example/".into(),
            legacy_nonce: "legacy-generation-one".into(),
            definition_event_id: "cd".repeat(32),
            target: None,
            phase: Phase::Prepared,
        };
        advance(home, Instance::Dev, transfer.clone()).expect("prepared");
        transfer.phase = Phase::Quiesced;
        advance(home, Instance::Dev, transfer.clone()).expect("quiesced");
        transfer
    }
    #[test]
    fn exact_unadmitted_legacy_rebind_is_atomic_and_idempotent() {
        let home = tempfile::tempdir().expect("home");
        let expected = staged(home.path());
        let _pair = admission_lock(
            home.path(),
            Instance::Dev,
            &expected.actor_pubkey,
            &expected.relay_url,
        )
        .expect("pair fence");
        let revision = "ef".repeat(32);
        revise_quiesced_import_definition(home.path(), Instance::Dev, &expected, &revision)
            .expect("verified conversion");
        revise_quiesced_import_definition(home.path(), Instance::Dev, &expected, &revision)
            .expect("exact retry");
        let mut next = expected.clone();
        next.relay_url = canonical_relay_key(&next.relay_url);
        next.definition_event_id = revision;
        assert_eq!(
            for_pair(
                home.path(),
                Instance::Dev,
                &expected.actor_pubkey,
                &expected.relay_url
            )
            .expect("read"),
            Some(next)
        );
        let journal = read(home.path(), Instance::Dev).expect("journal");
        assert_eq!(journal.history.len(), 1);
        assert_eq!(
            journal.history[0].definition_event_id,
            expected.definition_event_id
        );
    }
    #[test]
    fn foreign_or_stale_conversion_never_mutates_quiesced_custody() {
        let home = tempfile::tempdir().expect("home");
        let expected = staged(home.path());
        let before = std::fs::read(
            layout::host_dir(home.path(), Instance::Dev).join("managed-ownership.json"),
        )
        .expect("bytes");
        let mut cases = Vec::new();
        let mut other = expected.clone();
        other.operation_id = "foreign-op".into();
        cases.push(other);
        let mut other = expected.clone();
        other.legacy_nonce = "foreign-generation".into();
        cases.push(other);
        let mut other = expected.clone();
        other.actor_pubkey = "aa".repeat(32);
        cases.push(other);
        let mut other = expected.clone();
        other.relay_url = "wss://other.example".into();
        cases.push(other);
        let mut other = expected.clone();
        other.definition_event_id = "11".repeat(32);
        cases.push(other);
        let mut other = expected.clone();
        other.legacy_nonce = "fresh-identity:import-one".into();
        cases.push(other);
        let mut other = expected.clone();
        other.phase = Phase::Prepared;
        cases.push(other);
        let mut other = expected.clone();
        other.target = Some(CodingSessionTarget {
            driver: "claude-agent-acp".into(),
            instance_id: "provider-one".into(),
            session_id: uuid::Uuid::new_v4().to_string(),
            generation: 1,
        });
        cases.push(other);
        for other in cases {
            assert!(revise_quiesced_import_definition(
                home.path(),
                Instance::Dev,
                &other,
                &"ef".repeat(32)
            )
            .is_err());
            assert_eq!(
                std::fs::read(
                    layout::host_dir(home.path(), Instance::Dev).join("managed-ownership.json")
                )
                .expect("unchanged"),
                before
            );
        }
        assert!(revise_quiesced_import_definition(
            home.path(),
            Instance::Dev,
            &expected,
            "malformed"
        )
        .is_err());
    }
    #[test]
    fn adopted_or_paused_import_cannot_be_rebound_even_on_exact_new_head() {
        for adopted in [false, true] {
            let home = tempfile::tempdir().expect("home");
            let expected = staged(home.path());
            let mut terminal = expected.clone();
            terminal.phase = if adopted {
                Phase::Adopted
            } else {
                Phase::HostStopped
            };
            if adopted {
                terminal.target = Some(CodingSessionTarget {
                    driver: "claude-agent-acp".into(),
                    instance_id: "provider-one".into(),
                    session_id: uuid::Uuid::new_v4().to_string(),
                    generation: 1,
                });
            }
            advance(home.path(), Instance::Dev, terminal.clone()).expect("verified boundary");
            assert!(revise_quiesced_import_definition(
                home.path(),
                Instance::Dev,
                &expected,
                &expected.definition_event_id
            )
            .is_err());
            assert_eq!(
                for_pair(
                    home.path(),
                    Instance::Dev,
                    &expected.actor_pubkey,
                    &expected.relay_url
                )
                .expect("read")
                .expect("transfer")
                .phase,
                terminal.phase
            );
        }
    }
}
