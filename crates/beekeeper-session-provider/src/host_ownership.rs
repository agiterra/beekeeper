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
    if !ownership::is_modern_enrollment(&transfer) {
        return Err(refusal());
    }
    let allowed = match transfer.phase {
        Phase::Adopted => true,
        Phase::HostStopped => match admission {
            Admission::Create {
                command,
                initial_turn: false,
            } => {
                if transfer.target.is_none() && transfer.legacy_nonce.starts_with("fresh-identity:")
                {
                    command == transfer.operation_id
                        && transfer.legacy_nonce == format!("fresh-identity:{command}")
                } else {
                    transfer.target.is_some()
                }
            }
            _ => false,
        },
        Phase::Prepared | Phase::Quiesced | Phase::Restoring | Phase::LegacyRestored => false,
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
    pub(crate) historical_setup: bool,
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
        historical_setup: transfer.is_some_and(|t| {
            !ownership::is_modern_enrollment(&t)
                || matches!(
                    t.phase,
                    Phase::Prepared | Phase::Quiesced | Phase::Restoring | Phase::LegacyRestored
                )
        }),
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
    let activation = match admission {
        Admission::Create {
            command,
            initial_turn: false,
        } => {
            let path = config
                .state_dir
                .join("actors")
                .join(actor)
                .join("enrollment.json");
            let value = crate::managed_configuration::protected(&path)?;
            value.and_then(|value| {
                if value
                    .get("activationOperation")
                    .and_then(serde_json::Value::as_str)
                    == Some(command)
                {
                    value
                        .get("enrolledOperation")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                } else {
                    None
                }
            })
        }
        _ => None,
    };
    let admission = activation
        .as_deref()
        .map_or(admission, |command| Admission::Create {
            command,
            initial_turn: false,
        });
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
    #[test]
    fn historical_states_are_read_only_and_never_admit_execution() {
        let home = tempfile::tempdir().unwrap();
        let actor = "ab".repeat(32);
        let relay = "wss://isolated.example";
        for phase in [
            Phase::Prepared,
            Phase::Quiesced,
            Phase::Restoring,
            Phase::LegacyRestored,
            Phase::Adopted,
            Phase::HostStopped,
        ] {
            let transfer = ownership::Transfer {
                operation_id: "historical".into(),
                actor_pubkey: actor.clone(),
                relay_url: relay.into(),
                legacy_nonce: "old".into(),
                definition_event_id: "cd".repeat(32),
                target: None,
                phase,
            };
            beekeeper_host_core::atomic_write::create_dir_all_restricted(
                &beekeeper_host_core::layout::host_dir(home.path(), Instance::Dev),
            )
            .unwrap();
            beekeeper_host_core::atomic_write::atomic_write_json_restricted(
                &beekeeper_host_core::layout::host_dir(home.path(), Instance::Dev)
                    .join("managed-ownership.json"),
                &serde_json::to_vec(&serde_json::json!({"transfers":[transfer],"history":[]}))
                    .unwrap(),
            )
            .unwrap();
            for admission in [
                Admission::Resume,
                Admission::Turn,
                Admission::Create {
                    command: "historical",
                    initial_turn: false,
                },
            ] {
                assert!(require(home.path(), Instance::Dev, &actor, relay, admission).is_err());
            }
            assert!(ownership::record_host_enrollment(
                home.path(),
                Instance::Dev,
                &actor,
                relay,
                "new",
                &"cd".repeat(32),
                None
            )
            .is_err());
        }
    }
}
