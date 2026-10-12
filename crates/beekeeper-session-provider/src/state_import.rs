//! Signal-free readback of the existing provider admission journals.
use super::*;

fn restricted_read(path: &Path, required: bool) -> Result<Option<Vec<u8>>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound && !required => return Ok(None),
        Err(_) => return Err("import admission evidence is unavailable".into()),
    };
    if !metadata.file_type().is_file() {
        return Err("import admission evidence is not a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("import admission evidence is not restricted".into());
        }
    }
    fs::read(path)
        .map(Some)
        .map_err(|_| "cannot read import admission evidence".into())
}

impl StateStore {
    /// Persist the existing in-memory authoritative snapshot at worker bootstrap.
    /// The read-only import guard never creates a default snapshot itself.
    pub(crate) fn establish_import_readback(&self) -> std::io::Result<()> {
        self.persist()
    }

    /// Affirmatively check existing protected provider admission provenance.
    ///
    /// Callers hold the same actor/community admission lock used before create
    /// reservation and runtime startup. This is read-only: missing/unreadable
    /// authoritative snapshots refuse, never open a new default store. Optional
    /// append ledgers may not exist before their first write; a protected
    /// versioned snapshot is mandatory even in that empty steady state.
    pub fn require_unadmitted_import(dir: &Path, actor: &str, command: &str) -> Result<(), String> {
        let actor_key = nostr::PublicKey::from_hex(actor)
            .map_err(|_| "invalid import actor")?
            .to_hex();
        let actor = actor_key.as_str();
        if command.is_empty() {
            return Err("import operation is missing".into());
        }
        let bytes =
            restricted_read(&dir.join(STATE_FILE), true)?.ok_or("provider snapshot missing")?;
        let fields: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "provider snapshot malformed")?;
        if !fields
            .get("sessions")
            .is_some_and(serde_json::Value::is_object)
            || !fields
                .get("terminalDispositions")
                .is_some_and(serde_json::Value::is_object)
        {
            return Err("provider snapshot lacks authoritative admission provenance".into());
        }
        let snapshot: Snapshot =
            serde_json::from_slice(&bytes).map_err(|_| "provider snapshot malformed")?;
        if snapshot.version != STATE_VERSION {
            return Err("provider snapshot version unsupported".into());
        }
        if snapshot.sessions.values().any(|record| {
            record
                .actor
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case(actor))
                || record.command_id == command
                || record.generation_command_id.as_deref() == Some(command)
        }) || snapshot.terminal_dispositions.iter().any(|(id, value)| {
            id == command
                || value.lifecycle_start.as_ref().is_some_and(|intent| {
                    intent
                        .actor
                        .as_deref()
                        .is_some_and(|value| value.eq_ignore_ascii_case(actor))
                })
        }) {
            return Err(
                "import already has retained execution or lifecycle admission provenance".into(),
            );
        }
        for file in [COMMANDS_FILE, REFUSALS_FILE] {
            if let Some(body) = restricted_read(&dir.join(file), false)? {
                let text = std::str::from_utf8(&body)
                    .map_err(|_| "provider admission ledger malformed")?;
                for line in text.lines().filter(|line| !line.trim().is_empty()) {
                    let row: CommandRecord = serde_json::from_str(line)
                        .map_err(|_| "provider admission ledger malformed")?;
                    if row.command_id == command {
                        return Err(
                            "import operation was consumed or answered; preserved for repair"
                                .into(),
                        );
                    }
                }
            }
        }
        let enrollment = dir.join("actors").join(actor).join("enrollment.json");
        if restricted_read(&enrollment, false)?.is_some() {
            return Err("import has a retained actor enrollment request".into());
        }
        if let Some(bytes) =
            restricted_read(&dir.join(crate::config::ACTOR_SEATS_FILE_NAME), false)?
        {
            let seats: crate::actor_seats::ActorSeatsFile =
                serde_json::from_slice(&bytes).map_err(|_| "actor admission custody malformed")?;
            if seats.version != 1 {
                return Err("actor admission custody version unsupported".into());
            }
            if seats
                .pending
                .iter()
                .any(|(id, seat)| id == command || seat.pubkey.eq_ignore_ascii_case(actor))
            {
                return Err("import has retained pending actor admission custody".into());
            }
        }
        if let Some(bytes) = restricted_read(
            &dir.join(beekeeper_host_core::env::PROJECTS_FILE_NAME),
            false,
        )? {
            let fields: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|_| "pending admission project map malformed")?;
            if !matches!(
                fields.get("version").and_then(serde_json::Value::as_u64),
                Some(0 | 1)
            ) {
                return Err("pending admission project map version unsupported".into());
            }
            let projects: beekeeper_host_core::project::ProjectsFile =
                serde_json::from_slice(&bytes)
                    .map_err(|_| "pending admission project map malformed")?;
            if !matches!(projects.version, 0 | 1) {
                return Err("pending admission project map version unsupported".into());
            }
            if projects.pending.contains_key(command) {
                return Err("import operation has retained admission workspace".into());
            }
        }
        if restricted_read(
            &dir.join(beekeeper_host_core::project::PENDING_HINTS_DIR)
                .join(format!("{command}.json")),
            false,
        )?
        .is_some()
        {
            return Err("import operation has retained admission workspace hint".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(dir: &Path) -> StateStore {
        let store = StateStore::open(dir, 3600).expect("fixture store");
        store
            .establish_import_readback()
            .expect("protected authoritative bootstrap snapshot");
        store
    }
    #[test]
    fn missing_authoritative_state_refuses_without_creating_or_mutating_files() {
        let dir = tempfile::tempdir().expect("state");
        let actor = "ab".repeat(32);
        assert!(StateStore::require_unadmitted_import(dir.path(), &actor, "op").is_err());
        assert_eq!(fs::read_dir(dir.path()).expect("directory").count(), 0);
        let _store = snapshot(dir.path());
        let before = fs::read(dir.path().join(STATE_FILE)).expect("snapshot");
        StateStore::require_unadmitted_import(dir.path(), &actor, "op")
            .expect("affirmative empty snapshot");
        assert_eq!(
            fs::read(dir.path().join(STATE_FILE)).expect("unchanged"),
            before
        );
    }
    #[test]
    fn every_reserved_start_phase_blocks_a_targetless_quiesced_import() {
        for phase in [
            LifecycleStartPhase::BeforeEffects,
            LifecycleStartPhase::EffectsStarted,
            LifecycleStartPhase::RuntimeStarting,
        ] {
            let dir = tempfile::tempdir().expect("state");
            let mut store = snapshot(dir.path());
            let actor = "ab".repeat(32);
            let mut expected = beekeeper_host_core::ownership::Transfer {
                operation_id: "import-one".into(),
                actor_pubkey: actor.clone(),
                relay_url: "wss://isolated.example".into(),
                legacy_nonce: "old-generation".into(),
                definition_event_id: "cd".repeat(32),
                target: None,
                phase: beekeeper_host_core::ownership::Phase::Prepared,
            };
            beekeeper_host_core::ownership::advance(
                dir.path(),
                beekeeper_host_core::layout::Instance::Dev,
                expected.clone(),
            )
            .expect("prepared");
            expected.phase = beekeeper_host_core::ownership::Phase::Quiesced;
            beekeeper_host_core::ownership::advance(
                dir.path(),
                beekeeper_host_core::layout::Instance::Dev,
                expected.clone(),
            )
            .expect("quiesced without accepted target");
            let _pair = beekeeper_host_core::ownership::admission_lock(
                dir.path(),
                beekeeper_host_core::layout::Instance::Dev,
                &actor,
                &expected.relay_url,
            )
            .expect("pair fence");
            let channel = Uuid::new_v4();
            let target = CodingSessionTarget {
                driver: "claude-agent-acp".into(),
                instance_id: "provider".into(),
                session_id: Uuid::new_v4().to_string(),
                generation: 1,
            };
            let receipt = beekeeper_core::coding_session_payload::LifecycleReceipt::failed(
                "op",
                beekeeper_core::coding_session_payload::CREATE_START_OUTCOME_UNCONFIRMED,
                "uncertain",
            );
            let event = nostr::EventBuilder::new(
                nostr::Kind::Custom(44224),
                serde_json::to_string(&receipt).expect("receipt"),
            )
            .sign_with_keys(&nostr::Keys::generate())
            .expect("signed receipt");
            store
                .stage_terminal_disposition(
                    "op",
                    TerminalDisposition {
                        semantic_key: "receipt:op".into(),
                        event,
                        lifecycle_start: Some(LifecycleStartIntent {
                            phase,
                            fingerprint: "request".into(),
                            target,
                            actor: Some(actor.clone()),
                            channel,
                            provider_ref: "runtime".into(),
                            runtime: "claude".into(),
                            initial_turn: false,
                        }),
                    },
                )
                .expect("actual durable reservation");
            assert!(store.sessions().next().is_none());
            assert!(
                StateStore::require_unadmitted_import(
                    dir.path(),
                    &actor.to_uppercase(),
                    "foreign-client-operation"
                )
                .is_err(),
                "canonical identity cannot bypass reserved admission"
            );
            let before = fs::read(dir.path().join(STATE_FILE)).expect("snapshot");
            let guarded = StateStore::require_unadmitted_import(
                dir.path(),
                &actor,
                "different-client-operation",
            )
            .and_then(|_| {
                beekeeper_host_core::ownership::revise_quiesced_import_definition(
                    dir.path(),
                    beekeeper_host_core::layout::Instance::Dev,
                    &expected,
                    &"ef".repeat(32),
                )
            });
            assert!(
                guarded.is_err(),
                "target absence never bypasses a reserved start"
            );
            assert_eq!(
                beekeeper_host_core::ownership::for_pair(
                    dir.path(),
                    beekeeper_host_core::layout::Instance::Dev,
                    &actor,
                    &expected.relay_url
                )
                .expect("journal")
                .expect("transfer")
                .definition_event_id,
                expected.definition_event_id
            );
            assert_eq!(
                fs::read(dir.path().join(STATE_FILE)).expect("unchanged"),
                before
            );
        }
    }
    #[test]
    fn unsupported_or_incomplete_provenance_never_reads_as_empty() {
        let dir = tempfile::tempdir().expect("state");
        let store = snapshot(dir.path());
        let actor = "ab".repeat(32);
        for file in [
            crate::config::ACTOR_SEATS_FILE_NAME,
            beekeeper_host_core::env::PROJECTS_FILE_NAME,
        ] {
            beekeeper_host_core::atomic_write::atomic_write_json_restricted(
                &dir.path().join(file),
                b"{\"version\":99,\"pending\":{}}",
            )
            .expect("unsupported protected version fixture");
            assert!(StateStore::require_unadmitted_import(dir.path(), &actor, "op").is_err());
            fs::remove_file(dir.path().join(file)).expect("only fixture cleanup");
        }
        beekeeper_host_core::atomic_write::atomic_write_json_restricted(
            &dir.path().join(STATE_FILE),
            b"{\"version\":1}",
        )
        .expect("old incomplete provenance");
        assert!(StateStore::require_unadmitted_import(dir.path(), &actor, "op").is_err());
        store
            .establish_import_readback()
            .expect("real existing bootstrap establishes full snapshot");
        StateStore::require_unadmitted_import(dir.path(), &actor, "op")
            .expect("affirmative full snapshot");
    }

    #[test]
    fn consumed_and_refused_operations_remain_blocked_without_a_session() {
        for consumed in [true, false] {
            let dir = tempfile::tempdir().expect("state");
            let mut store = snapshot(dir.path());
            if consumed {
                store.consume_command("op", 1).expect("consumed");
            } else {
                store.record_refusal("op", 1).expect("answered");
            }
            assert!(
                StateStore::require_unadmitted_import(dir.path(), &"ab".repeat(32), "op").is_err()
            );
        }
    }
}
