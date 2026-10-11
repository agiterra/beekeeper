//! Local process-owner fence; relay/session authority remains independent.
use crate::session::CreateFailure;
use beekeeper_host_core::{
    layout::Instance,
    ownership::{self, Phase},
};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub(crate) enum Admission<'a> {
    Create {
        command: &'a str,
        initial_turn: bool,
    },
    Resume,
    Turn,
}

fn refusal() -> CreateFailure {
    crate::execution_scope::refuse("ACTOR_OWNERSHIP_UNAVAILABLE", "this participant is owned by the legacy client or its ownership transfer is incomplete; no work was admitted")
}

/// Holds only the pair admission lock, never the management operation lock.
pub(crate) struct HostAdmission {
    home: PathBuf,
    instance: Instance,
    actor: String,
    relay: String,
    command: Option<String>,
    initial_turn: bool,
    _lock: std::fs::File,
}

impl HostAdmission {
    pub(crate) fn recheck(&self) -> Result<(), CreateFailure> {
        let admission = match self.command.as_deref() {
            Some(command) => Admission::Create {
                command,
                initial_turn: self.initial_turn,
            },
            None => Admission::Resume,
        };
        require(
            &self.home,
            self.instance,
            &self.actor,
            &self.relay,
            admission,
        )
    }
}

fn require(
    home: &Path,
    instance: Instance,
    actor: &str,
    relay: &str,
    admission: Admission<'_>,
) -> Result<(), CreateFailure> {
    let Some(transfer) =
        ownership::for_pair(home, instance, actor, relay).map_err(|_| refusal())?
    else {
        return Ok(());
    };
    let allowed = match transfer.phase {
        Phase::Adopted => true,
        Phase::Quiesced => {
            matches!(admission, Admission::Create { command, initial_turn: false } if command == transfer.operation_id)
        }
        Phase::HostStopped => matches!(
            admission,
            Admission::Create {
                initial_turn: false,
                ..
            }
        ),
        Phase::Prepared | Phase::Restoring | Phase::LegacyRestored => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(refusal())
    }
}

/// Resolve the same explicit host context used by runtime admission.
pub(crate) fn context() -> Result<Option<(PathBuf, Instance)>, CreateFailure> {
    let Some(home) = std::env::var_os(beekeeper_host_core::layout::HOME_VAR).map(PathBuf::from)
    else {
        return Ok(None);
    };
    if !home.is_absolute() {
        return Err(refusal());
    }
    Ok(Some((home, Instance::from_env().map_err(|_| refusal())?)))
}

/// Keep synchronous conversation disposition serialized with the existing
/// ownership transition fence. Runtime startup acquires its own admission.
pub(crate) struct ConversationOwnership {
    pub(crate) legacy_owns: bool,
    pub(crate) busy: bool,
    _lock: Option<std::fs::File>,
}

pub(crate) fn conversation_ownership(
    home: &Path,
    instance: Instance,
    actor: &str,
    relay: &str,
) -> Result<ConversationOwnership, CreateFailure> {
    let lock = ownership::try_admission_lock(home, instance, actor, relay).ok();
    let transfer = ownership::for_pair(home, instance, actor, relay).map_err(|_| refusal())?;
    Ok(ConversationOwnership {
        legacy_owns: transfer
            .is_some_and(|t| matches!(t.phase, Phase::Prepared | Phase::LegacyRestored)),
        busy: lock.is_none(),
        _lock: lock,
    })
}

pub(crate) fn admit(
    config: &crate::config::Config,
    actor: Option<&str>,
    admission: Admission<'_>,
) -> Result<Option<HostAdmission>, CreateFailure> {
    let Some(actor) = actor else {
        return Ok(None);
    };
    let Some((home, instance)) = context()? else {
        return Ok(None);
    };
    require(&home, instance, actor, &config.relay_url, admission)?;
    let lock = ownership::try_admission_lock(&home, instance, actor, &config.relay_url).map_err(|_| crate::execution_scope::refuse("ACTOR_OWNERSHIP_BUSY", "another startup or ownership transition is in progress for this participant; retry at its next boundary"))?;
    require(&home, instance, actor, &config.relay_url, admission)?;
    Ok(Some(HostAdmission {
        home,
        instance,
        actor: actor.into(),
        relay: config.relay_url.clone(),
        command: match admission {
            Admission::Create { command, .. } => Some(command.into()),
            _ => None,
        },
        initial_turn: matches!(
            admission,
            Admission::Create {
                initial_turn: true,
                ..
            }
        ),
        _lock: lock,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_host_core::ownership::Transfer;

    #[test]
    fn host_ownership_fences_each_transfer_boundary_without_blocking_fresh_hosts() {
        let home = tempfile::tempdir().expect("home");
        let actor = "ab".repeat(32);
        let relay = "wss://isolated.example";
        assert!(require(home.path(), Instance::Dev, &actor, relay, Admission::Turn).is_ok());
        let mut transfer = Transfer {
            operation_id: "adopt-1".into(),
            actor_pubkey: actor.clone(),
            relay_url: relay.into(),
            legacy_nonce: "legacy-1".into(),
            definition_event_id: "cd".repeat(32),
            target: None,
            phase: Phase::Prepared,
        };
        ownership::advance(home.path(), Instance::Dev, transfer.clone()).expect("prepared");
        assert!(require(home.path(), Instance::Dev, &actor, relay, Admission::Resume).is_err());
        transfer.phase = Phase::Quiesced;
        ownership::advance(home.path(), Instance::Dev, transfer.clone()).expect("quiesced");
        assert!(require(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            Admission::Create {
                command: "other",
                initial_turn: false
            }
        )
        .is_err());
        assert!(require(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            Admission::Create {
                command: "adopt-1",
                initial_turn: true
            }
        )
        .is_err());
        assert!(require(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            Admission::Create {
                command: "adopt-1",
                initial_turn: false
            }
        )
        .is_ok());
        assert!(require(home.path(), Instance::Dev, &actor, relay, Admission::Turn).is_err());
        transfer.target = Some(
            beekeeper_core::coding_session_command::CodingSessionTarget {
                driver: "codex-acp".into(),
                instance_id: "primary".into(),
                session_id: "00000000-0000-0000-0000-000000000001".into(),
                generation: 1,
            },
        );
        transfer.phase = Phase::Adopted;
        ownership::advance(home.path(), Instance::Dev, transfer.clone()).expect("adopted");
        assert!(require(home.path(), Instance::Dev, &actor, relay, Admission::Turn).is_ok());
        transfer.phase = Phase::HostStopped;
        ownership::advance(home.path(), Instance::Dev, transfer.clone()).expect("host stopped");
        assert!(require(home.path(), Instance::Dev, &actor, relay, Admission::Turn).is_err());
        assert!(require(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            Admission::Create {
                command: "successor",
                initial_turn: false
            }
        )
        .is_ok());
        transfer.phase = Phase::Restoring;
        ownership::advance(home.path(), Instance::Dev, transfer.clone()).expect("restoring");
        assert!(require(home.path(), Instance::Dev, &actor, relay, Admission::Resume).is_err());
        transfer.phase = Phase::LegacyRestored;
        ownership::advance(home.path(), Instance::Dev, transfer).expect("legacy restored");
        assert!(require(home.path(), Instance::Dev, &actor, relay, Admission::Turn).is_err());
    }
}
