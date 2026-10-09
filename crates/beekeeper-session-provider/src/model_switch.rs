//! SV-35: mid-session model and effort switching (`thread.model.set`).
//!
//! The provider half. A switch is admitted through the `thread.turn.start`
//! chain without spending a turn ([`decide`]), handed to the execution's
//! mailbox behind anything already there, applied by the actor at the next
//! boundary (`session::model_switch`), and answered here
//! ([`Provider::fold_model_switch`]): on success the record, a `model_switched`
//! transcript item, the `model_applied` receipt and the same generation's
//! 44223 — in that order. The wire contract is NIP-CSC / NIP-CSL / NIP-CST.
//!
//! What 44223 `model` then says is the adapter's acknowledgement, never the
//! request. Effort in particular has no turn-level evidence at all: no runtime
//! reports the effort a turn ran at, so the `[effort]` in `model` is the
//! adapter's `currentValue` answer and nothing more.

use std::collections::HashMap;

use beekeeper_core::coding_session_command::CodingSessionTarget;
use uuid::Uuid;

use crate::commands::{CommandContext, TurnDecision};
use crate::payload::{
    self, LifecycleReceipt, SessionStatus, MODEL_NOT_OFFERED, MODEL_SWITCH_UNSUPPORTED,
    NO_LIVE_EXECUTION, QUEUE_FULL,
};
use crate::publish::Priority;
use crate::session::{DeliverError, SessionCommand};
use crate::state::SessionRecord;
use crate::{Provider, TurnDisposition};

/// The emission switch's build default: whether this provider advertises
/// `capabilities.modelSwitch` and accepts `thread.model.set` when the host
/// does not say otherwise.
///
/// Off, no execution's 44223 carries `modelSwitch` and every switch is refused
/// `MODEL_SWITCH_UNSUPPORTED` at admission. It ships **off**: older exact-key
/// readers (installed desktops before SV-35, existing mobile builds) reject an
/// unseen capability key and would drop the whole 44223. A canary host turns
/// it on with `BEEKEEPER_CSP_MODEL_SWITCH=1`, read once at provider start
/// ([`crate::config::Config::model_switch`]); flipping this default is the
/// release action once the installed readers accept the key.
pub const MODEL_SWITCH_ENABLED: bool = false;

/// What became of one `thread.model.set` at the execution's boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSwitchOutcome {
    /// The adapter accepted the switch.
    Applied {
        /// The selection as sent.
        requested: String,
        /// What the adapter acknowledged — the value 44223 `model` publishes.
        applied: String,
    },
    /// Refused; the execution keeps its model. Answered `turn_refused`.
    Refused {
        /// Receipt error code.
        code: &'static str,
        /// Operator-facing detail.
        message: String,
    },
    /// Never applied because it could not be queued. Answered `turn_dropped`.
    Dropped {
        /// Receipt error code.
        code: &'static str,
        /// Operator-facing detail.
        message: String,
    },
}

/// The base of `selection` that is one of `allowed`: the whole selection, or
/// the longest remainder left by peeling trailing `[token]` groups. An id may
/// itself carry brackets (`opus[1m]`), which is why this is a prefix search
/// against the list rather than a split at the first `[`.
pub fn offered_base<'a>(selection: &'a str, allowed: &[String]) -> Option<&'a str> {
    let listed = |candidate: &str| allowed.iter().any(|model| model == candidate);
    if listed(selection) {
        return Some(selection);
    }
    beekeeper_acp::model_options::peel_bracket_suffixes(selection)
        .into_iter()
        .map(|(remainder, _)| remainder)
        .find(|remainder| listed(remainder))
}

/// Whether an adapter's `session/new` result offered a model control at all:
/// a `configOptions` entry of category `model`, or `models.availableModels`.
pub fn offers_model_control(raw: &serde_json::Value) -> bool {
    !beekeeper_acp::acp::extract_model_config_options(raw).is_empty()
        || beekeeper_acp::acp::extract_model_state(raw)
            .is_some_and(|models| models.get("availableModels").is_some())
}

/// The switch-specific half of [`crate::commands::decide_turn_command`],
/// reached only once the shared authority, fence and closure checks admitted
/// the command. No budget is consulted and no operation is fenced: nothing is
/// prompted.
pub(crate) fn decide(
    context: &CommandContext<'_>,
    record: &SessionRecord,
    command_id: String,
    target: CodingSessionTarget,
    selection: String,
) -> TurnDecision {
    let allowed: Vec<String> = context
        .runtimes
        .iter()
        .find(|descriptor| descriptor.instance_ref == record.provider_instance_ref)
        .map(|descriptor| {
            std::iter::once(descriptor.default_model.clone())
                .chain(descriptor.allowed_models.iter().cloned())
                .collect()
        })
        .unwrap_or_default();
    match admission_refusal(context.model_switch_enabled, &selection, &allowed) {
        Some((code, message)) => TurnDecision::Fail {
            command_id,
            target,
            code,
            message,
        },
        None => TurnDecision::SetModel {
            command_id,
            target,
            selection,
        },
    }
}

/// The admission refusal a switch earns before any mailbox, if any: the
/// build does not switch (`enabled` is `Config::model_switch`), or the
/// selection's base is not in `allowed`.
pub fn admission_refusal(
    enabled: bool,
    selection: &str,
    allowed: &[String],
) -> Option<(&'static str, String)> {
    if !enabled {
        return Some((
            MODEL_SWITCH_UNSUPPORTED,
            "this provider build does not change models mid-session; the execution keeps its \
             model"
                .into(),
        ));
    }
    offered_base(selection, allowed).is_none().then(|| {
        (
            MODEL_NOT_OFFERED,
            format!(
                "{selection} is not one of the models this provider instance offers \
                 (allowedModels); the execution keeps its model"
            ),
        )
    })
}

/// Whether an execution's 44223 says `modelSwitch: true`: the build switches
/// (`enabled` is `Config::model_switch`) and its live process offered a
/// model control.
pub fn advertised(enabled: bool, witnessed: bool) -> bool {
    enabled && witnessed
}

/// One switch handed to a mailbox and not yet answered.
#[derive(Debug, Clone)]
struct PendingSwitch {
    channel_id: Uuid,
    session_id: String,
    /// The command event's `created_at`: the replay floor may not pass it.
    created_at: u64,
}

/// Process-local switch facts. In memory for the reason `in_flight` is: after
/// a restart there is no mailbox, so nothing pending in one could still be
/// true, and the floor this holds makes the relay redeliver it.
#[derive(Debug, Default)]
pub(crate) struct ModelSwitchState {
    /// Whether each live execution's `session/new` offered a model control.
    witnessed: HashMap<String, bool>,
    /// `commandId → switch` for every switch in a mailbox.
    pending: HashMap<String, PendingSwitch>,
}

impl ModelSwitchState {
    /// Record what the process behind `session_id` offered at open.
    pub(crate) fn witness(&mut self, session_id: &str, offered: bool) {
        self.witnessed.insert(session_id.to_owned(), offered);
    }

    /// `created_at` of every switch on `channel_id` still owed an answer.
    pub(crate) fn floor_for(&self, channel_id: Uuid) -> impl Iterator<Item = u64> + '_ {
        self.pending
            .values()
            .filter(move |switch| switch.channel_id == channel_id)
            .map(|switch| switch.created_at)
    }
}

impl Provider {
    /// Whether `session_id`'s 44223 may say `modelSwitch: true`: this build
    /// switches, and the live process behind it offered a model control.
    pub(crate) fn model_switch_capable(&self, session_id: &str) -> bool {
        advertised(
            self.config.model_switch,
            self.model_switch
                .witnessed
                .get(session_id)
                .copied()
                .unwrap_or(false),
        )
    }

    /// Hand an admitted switch to its execution's mailbox, or answer it.
    pub(crate) fn deliver_model_switch(
        &mut self,
        channel_id: Uuid,
        created_at: u64,
        command_id: String,
        target: CodingSessionTarget,
        selection: String,
    ) -> anyhow::Result<TurnDisposition> {
        // Already in a mailbox: the answer is owed by the actor.
        if self.model_switch.pending.contains_key(&command_id) {
            return Ok(TurnDisposition::Silent);
        }
        let session_id = target.session_id.clone();
        if self.sessions.handle(&session_id).is_none() {
            return self.drop_model_switch(channel_id, &command_id, &target, NO_LIVE_EXECUTION);
        }
        if !self.model_switch_capable(&session_id) {
            let receipt = LifecycleReceipt::turn_refused(
                &command_id,
                &target,
                MODEL_SWITCH_UNSUPPORTED,
                "this execution's adapter offered no model control when it opened, so it cannot \
                 change models mid-session; it keeps its model",
            );
            self.refuse_with_staged_answer(channel_id, &command_id, &receipt, None)?;
            return Ok(TurnDisposition::Answered(
                MODEL_SWITCH_UNSUPPORTED.to_owned(),
            ));
        }
        let delivered = match self.sessions.handle(&session_id) {
            Some(handle) => handle.deliver(SessionCommand::SetModel {
                command_id: command_id.clone(),
                selection,
            }),
            None => Err(DeliverError::Gone),
        };
        match delivered {
            Ok(()) => {
                self.model_switch.pending.insert(
                    command_id,
                    PendingSwitch {
                        channel_id,
                        session_id,
                        created_at,
                    },
                );
                Ok(TurnDisposition::Delivered)
            }
            Err(DeliverError::QueueFull) => {
                self.drop_model_switch(channel_id, &command_id, &target, QUEUE_FULL)
            }
            Err(DeliverError::Gone) => {
                self.drop_model_switch(channel_id, &command_id, &target, NO_LIVE_EXECUTION)
            }
        }
    }

    fn drop_model_switch(
        &mut self,
        channel_id: Uuid,
        command_id: &str,
        target: &CodingSessionTarget,
        code: &'static str,
    ) -> anyhow::Result<TurnDisposition> {
        let receipt =
            LifecycleReceipt::turn_dropped(command_id, target, code, &dropped_message(code));
        self.enqueue_terminal_receipt(channel_id, command_id, &receipt)?;
        Ok(TurnDisposition::Answered(code.to_owned()))
    }

    /// Answer one switch the actor reached. On success, in order: the record,
    /// the `model_switched` item, the `model_applied` receipt, and the same
    /// generation's 44223 — all at one priority, so they drain in that order.
    pub(crate) fn fold_model_switch(
        &mut self,
        session_id: &str,
        command_id: &str,
        outcome: ModelSwitchOutcome,
    ) -> anyhow::Result<()> {
        self.model_switch.pending.remove(command_id);
        let Some((channel_id, target)) = self.locate(session_id) else {
            return Ok(());
        };
        match outcome {
            ModelSwitchOutcome::Applied { requested, applied } => {
                tracing::info!(
                    target: "csp",
                    %session_id,
                    %command_id,
                    %requested,
                    %applied,
                    "model switch applied at the boundary"
                );
                // `model_effective` is left to the next turn's own report:
                // an acknowledgement is not a turn that ran on the model.
                self.state.update_session(session_id, |record| {
                    record.model = Some(applied.clone());
                    record.model_requested = Some(requested.clone());
                })?;
                self.enqueue_transcript(
                    channel_id,
                    &target,
                    None,
                    payload::model_switched_item(&applied, &requested, command_id),
                    Priority::Live,
                )?;
                let receipt = LifecycleReceipt::model_applied(command_id, &target);
                self.enqueue_terminal_receipt(channel_id, command_id, &receipt)?;
                let status = self
                    .last_metadata
                    .get(session_id)
                    .map_or(SessionStatus::Idle, |published| published.status);
                self.publish_metadata_at_priority(channel_id, &target, status, Priority::Live)?;
            }
            ModelSwitchOutcome::Refused { code, message } => {
                tracing::warn!(target: "csp", %session_id, %command_id, code, "model switch refused: {message}");
                let receipt = LifecycleReceipt::turn_refused(command_id, &target, code, &message);
                self.refuse_with_staged_answer(channel_id, command_id, &receipt, None)?;
            }
            ModelSwitchOutcome::Dropped { code, message } => {
                let receipt = LifecycleReceipt::turn_dropped(command_id, &target, code, &message);
                self.enqueue_terminal_receipt(channel_id, command_id, &receipt)?;
            }
        }
        Ok(())
    }

    /// The process behind `session_id` exited: every switch still in its
    /// mailbox is answered `turn_dropped`/`NO_LIVE_EXECUTION`, and its
    /// witnessed controls are forgotten.
    pub(crate) fn report_lost_model_switches(&mut self, session_id: &str) -> anyhow::Result<()> {
        self.model_switch.witnessed.remove(session_id);
        let lost: Vec<String> = self
            .model_switch
            .pending
            .iter()
            .filter(|(_, switch)| switch.session_id == session_id)
            .map(|(command_id, _)| command_id.clone())
            .collect();
        for command_id in lost {
            let Some(switch) = self.model_switch.pending.remove(&command_id) else {
                continue;
            };
            let Some((_, target)) = self.locate(session_id) else {
                continue;
            };
            self.drop_model_switch(switch.channel_id, &command_id, &target, NO_LIVE_EXECUTION)?;
        }
        Ok(())
    }
}

fn dropped_message(code: &str) -> String {
    if code == QUEUE_FULL {
        "the execution's queue is full, so the model switch was not delivered; it keeps its model"
            .into()
    } else {
        "this execution has no live process, so the model switch was not applied and will not \
         be retried; it keeps its recorded model — resume it and send the switch again"
            .into()
    }
}
