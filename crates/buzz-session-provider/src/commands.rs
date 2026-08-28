//! Decoding, addressing, fencing, and freshness for the two operator-authored
//! command kinds (44220 turn, 44221 lifecycle).
//!
//! Everything in this module is a pure decision over already-fetched state: it
//! returns *what to do*, never does it. That is what makes the fencing rules —
//! the parts that must never regress — cheap to test exhaustively.
//!
//! Both 44220 actions (`thread.turn.start`, `thread.turn.interrupt`) decode
//! through the shared `buzz_core::coding_session_command` type, so the
//! provider and the relay can never disagree about what is valid.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use uuid::Uuid;

use buzz_core::coding_session_command::{
    CodingSessionAction, CodingSessionCommandPayload, CodingSessionDelivery, CodingSessionTarget,
};
use buzz_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
};
use buzz_core::coding_session_runtime::RuntimeDescriptor;

use crate::payload::{
    ACTOR_UNAVAILABLE, BUDGET_EXHAUSTED, PROJECT_CWD_UNRESOLVED, PROVIDER_UNAVAILABLE,
    SESSION_CLOSED, SESSION_LIMIT, STALE_GENERATION, UNAUTHORIZED_OPERATOR, UNKNOWN_TARGET,
};
use crate::state::StateStore;

/// Why a command produced no side effect.
///
/// The last three variants carry the command and target they refused. They are
/// reached only *after* the addressing check, so the provider both knows the
/// command was meant for it and can name what the operator addressed — which
/// is exactly the difference between an ignore that owes the operator a
/// visible refusal and one that must stay silent. `NotAddressed`,
/// `AlreadyConsumed`, `AlreadyRefused`, `AlreadyAccepted`, `PastHorizon`, and
/// `Malformed` carry nothing because
/// answering them would be cross-provider chatter, a duplicate answer, or a
/// claim about a command this provider cannot read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ignored {
    /// The command names a different provider authority or instance.
    NotAddressed,
    /// This `commandId` was already consumed.
    AlreadyConsumed,
    /// This `commandId` was already answered with a refusal, durably. Silent
    /// for the same reason as `AlreadyConsumed`: a second byte-identical
    /// refusal is a stutter, not information.
    AlreadyRefused,
    /// This `commandId` is already in this process's mailbox: a turn accepted
    /// and not yet started, or a cancel already handed to an actor and not yet
    /// answered durably. Silent: a relay redelivery must not queue the turn
    /// twice, nor cancel a second time work nobody asked to stop.
    AlreadyAccepted,
    /// Older than the freshness horizon and never seen — history, not intent.
    PastHorizon,
    /// Content did not decode against the contract.
    Malformed(String),
    /// No live session matches the addressed target.
    UnknownTarget {
        /// The command being ignored.
        command_id: String,
        /// What it addressed.
        target: CodingSessionTarget,
    },
    /// The addressed generation is not the one this provider is running.
    StaleGeneration {
        /// The command being ignored.
        command_id: String,
        /// What it addressed.
        target: CodingSessionTarget,
    },
    /// The addressed session has been retired.
    SessionClosed {
        /// The command being ignored.
        command_id: String,
        /// What it addressed.
        target: CodingSessionTarget,
    },
}

/// A visible refusal owed to the operator for an ignored turn command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnRefusal<'a> {
    /// The 44220 being answered.
    pub command_id: &'a str,
    /// The target the command addressed.
    pub target: &'a CodingSessionTarget,
    /// Stable receipt error code.
    pub code: &'static str,
    /// Operator-facing detail.
    pub message: &'static str,
}

impl Ignored {
    /// The refusal this ignore owes the operator, or `None` when silence is
    /// the correct answer.
    ///
    /// Silence is correct for a command addressed to another provider, one
    /// already answered, one older than the freshness horizon, and one whose
    /// content did not decode — in every case a receipt would either be
    /// chatter about someone else's command or a second answer to a command
    /// already answered.
    pub fn refusal(&self) -> Option<TurnRefusal<'_>> {
        match self {
            Self::NotAddressed
            | Self::AlreadyConsumed
            | Self::AlreadyRefused
            | Self::AlreadyAccepted
            | Self::PastHorizon
            | Self::Malformed(_) => None,
            Self::UnknownTarget { command_id, target } => Some(TurnRefusal {
                command_id,
                target,
                code: UNKNOWN_TARGET,
                message: "this provider has no execution matching the addressed target",
            }),
            Self::StaleGeneration { command_id, target } => Some(TurnRefusal {
                command_id,
                target,
                code: STALE_GENERATION,
                message: "the addressed generation has been superseded",
            }),
            Self::SessionClosed { command_id, target } => Some(TurnRefusal {
                command_id,
                target,
                code: SESSION_CLOSED,
                message: "the addressed execution was durably stopped",
            }),
        }
    }
}

/// What to do about one 44221 lifecycle command.
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleDecision {
    /// Do nothing.
    Ignore(Ignored),
    /// Publish a `failed` receipt with this code.
    Fail {
        /// The command being answered.
        command_id: String,
        /// Stable receipt error code.
        code: &'static str,
        /// Operator-facing detail.
        message: String,
    },
    /// Create a session.
    Create(Box<CreatePlan>),
    /// Reattach a disconnected session as a new generation.
    Resume(ResumePlan),
    /// Durably stop a session.
    Stop(StopPlan),
}

/// A validated, resolved create request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePlan {
    /// The create command being answered.
    pub command_id: String,
    /// The matched runtime's `instance_ref` — which descriptor serves this
    /// create.
    pub runtime_instance_ref: String,
    /// Channel every event for the new session is published into.
    pub channel_id: Uuid,
    /// Host-local working directory. Never appears in signed content.
    pub cwd: PathBuf,
    /// NIP-MP project coordinate, or `None` for a standalone session.
    pub project_ref: Option<String>,
    /// Repository coordinate, or `None`.
    pub repo_ref: Option<String>,
    /// Umbrella session reference from the create, or `None` when unclaimed.
    pub session_ref: Option<String>,
    /// Explicit genesis event id to resolve before execution starts.
    pub genesis_ref: Option<String>,
    /// Locally witnessed create signer, replaced by the explicitly referenced
    /// genesis signer for authority-aware creates.
    pub founder_pubkey: String,
    /// Requested model, or `None`.
    pub model: Option<String>,
    /// Operator-facing title, or `None`.
    pub title: Option<String>,
    /// First turn to deliver after creation, or `None`.
    pub initial_turn: Option<String>,
    /// The agent seat this execution runs as, or `None` for a human-created
    /// execution.
    ///
    /// The pubkey only. Its key material is fetched from host-local custody at
    /// spawn time and never enters this plan — a `CreatePlan` is `Debug` and
    /// travels through the create path, so it is exactly the wrong place for a
    /// signing key.
    pub actor: Option<String>,
    /// The role slug that seat holds. Set exactly when `actor` is.
    pub role: Option<String>,
}

/// A validated request to reattach one exact prior generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumePlan {
    /// Lifecycle command being answered.
    pub command_id: String,
    /// Channel the execution belongs to.
    pub channel_id: Uuid,
    /// Exact disconnected generation the operator observed.
    pub target: CodingSessionTarget,
}

/// A validated request to durably stop one exact current generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopPlan {
    /// Lifecycle command being answered.
    pub command_id: String,
    /// Channel the execution belongs to.
    pub channel_id: Uuid,
    /// Exact current generation the operator observed.
    pub target: CodingSessionTarget,
}

/// What to do about one 44220 turn command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnDecision {
    /// Do nothing.
    Ignore(Ignored),
    /// Refuse visibly: the signer lacks session authority, or the umbrella
    /// has spent its turn budget.
    Fail {
        /// The command being answered.
        command_id: String,
        /// The target the command addressed, so the refusal names the
        /// execution the operator tried to steer.
        target: CodingSessionTarget,
        /// Stable receipt error code. Carried rather than assumed by the
        /// publisher: two different refusals reach this variant now, and a
        /// hard-coded `UNAUTHORIZED_OPERATOR` at the publish site would label
        /// a spent budget as an authority failure — a false statement about
        /// the sender.
        code: &'static str,
        /// Operator-facing detail.
        message: String,
    },
    /// Deliver a prompt to a live session.
    Start {
        /// The command being answered.
        command_id: String,
        /// The fenced target.
        target: CodingSessionTarget,
        /// Operator-entered turn text.
        text: String,
        /// The delivery class the sender asked for. Authority for it has
        /// already been checked here: an `interrupt` that reaches this variant
        /// was signed by the founder.
        deliver: CodingSessionDelivery,
    },
    /// Cancel the in-flight turn of a live session.
    Interrupt {
        /// The command being answered.
        command_id: String,
        /// The fenced target.
        target: CodingSessionTarget,
    },
}

/// Read-only view of everything a decision depends on.
pub struct CommandContext<'a> {
    /// This provider's signing pubkey, lowercase hex.
    pub provider_pubkey: &'a str,
    /// Pubkey that signed the command currently being decided.
    pub operator_pubkey: &'a str,
    /// Every runtime this provider offers.
    pub runtimes: &'a [RuntimeDescriptor],
    /// Instance id in every `cs-target` this provider mints.
    pub instance_id: &'a str,
    /// Current wall clock, seconds since the Unix epoch.
    pub now_secs: u64,
    /// Freshness horizon in seconds.
    pub horizon_secs: u64,
    /// Ceiling on concurrently live sessions.
    pub max_sessions: usize,
    /// Ceiling on turns started under one umbrella, or
    /// [`crate::config::UNLIMITED_TURN_BUDGET`] for no ceiling (D9).
    pub turn_budget: u64,
    /// Number of adapter actors currently attached in this process.
    pub active_session_count: usize,
    /// Durable state: dedupe ledger and session records.
    pub state: &'a StateStore,
    /// Host-local working-directory map, freshly read.
    pub projects: &'a ProjectsFile,
    /// Host-local agent-seat custody, freshly read.
    ///
    /// Consulted for presence and identity only — a decision never carries key
    /// material out of it.
    pub actor_seats: &'a crate::actor_seats::ActorSeatsFile,
    /// `commandId`s this process has accepted into a mailbox and not yet run.
    ///
    /// Consumption now happens when a turn *starts*, so the durable ledger
    /// cannot answer "is this already on its way" for the window between
    /// accept and start. This set can, and it is deliberately in-memory: after
    /// a restart the mailbox is empty, and every unstarted command genuinely
    /// does need re-delivering.
    pub in_flight: &'a HashMap<String, crate::InFlightTurn>,
    /// `commandId`s of cancels this process has already handed to an actor's
    /// mailbox and has not yet answered durably.
    ///
    /// An interrupt is not a turn, so it never appears in `in_flight`, and the
    /// ledger append that would durably answer it happens *after* the receipt
    /// that answers the operator and can fail — rolling its own in-memory entry
    /// back. Without this queue a redelivery of such a command reaches the
    /// actor a second time and cancels an unrelated running turn. In-memory for
    /// the same reason as `in_flight`, and bounded: see
    /// `crate::DELIVERED_CANCEL_FENCE_CAPACITY`.
    pub delivered_cancels: &'a VecDeque<String>,
}

impl CommandContext<'_> {
    fn past_horizon(&self, created_at: u64) -> bool {
        created_at + self.horizon_secs < self.now_secs
    }
}

/// Decide what a 44221 lifecycle command means for this provider.
pub fn decide_lifecycle(
    context: &CommandContext<'_>,
    channel_id: Uuid,
    created_at: u64,
    content: &str,
) -> LifecycleDecision {
    let payload = match decode_coding_session_lifecycle_command(content) {
        Ok(payload) => payload,
        Err(error) => return LifecycleDecision::Ignore(Ignored::Malformed(error)),
    };
    let provider_authority_pubkey = match &payload.action {
        CodingSessionLifecycleAction::SessionCreate {
            provider_authority_pubkey,
            ..
        }
        | CodingSessionLifecycleAction::SessionResume {
            provider_authority_pubkey,
            ..
        }
        | CodingSessionLifecycleAction::SessionStop {
            provider_authority_pubkey,
            ..
        } => provider_authority_pubkey,
    };

    // Addressing before dedupe: a command for another adapter must not consume
    // an id in *this* adapter's ledger, or a later legitimate reuse would be
    // silently dropped.
    if !provider_authority_pubkey.eq_ignore_ascii_case(context.provider_pubkey) {
        return LifecycleDecision::Ignore(Ignored::NotAddressed);
    }
    if context.state.is_command_consumed(&payload.command_id) {
        return LifecycleDecision::Ignore(Ignored::AlreadyConsumed);
    }
    if context.past_horizon(created_at) {
        return LifecycleDecision::Ignore(Ignored::PastHorizon);
    }

    if let CodingSessionLifecycleAction::SessionResume { session, .. }
    | CodingSessionLifecycleAction::SessionStop { session, .. } = &payload.action
    {
        if session.instance_id != context.instance_id {
            return LifecycleDecision::Ignore(Ignored::NotAddressed);
        }
        let Some(record) = context.state.session(&session.session_id) else {
            return LifecycleDecision::Fail {
                command_id: payload.command_id,
                code: UNKNOWN_TARGET,
                message: "this provider has no record of the addressed execution".into(),
            };
        };
        if record.channel_id != channel_id || record.driver != session.driver {
            return LifecycleDecision::Fail {
                command_id: payload.command_id,
                code: UNKNOWN_TARGET,
                message:
                    "the addressed execution does not belong to this channel and provider runtime"
                        .into(),
            };
        }
        if record.generation != session.generation {
            return LifecycleDecision::Fail {
                command_id: payload.command_id,
                code: STALE_GENERATION,
                message: format!(
                    "the addressed execution generation {} is stale; the current generation is {}",
                    session.generation, record.generation
                ),
            };
        }
        if !operator_owns_session(record, context.operator_pubkey) {
            return LifecycleDecision::Fail {
                command_id: payload.command_id,
                code: UNAUTHORIZED_OPERATOR,
                message: "only the session founder may stop or resume this execution".into(),
            };
        }

        return match &payload.action {
            CodingSessionLifecycleAction::SessionResume { .. } if record.closed => {
                LifecycleDecision::Fail {
                    command_id: payload.command_id,
                    code: SESSION_CLOSED,
                    message: "the addressed execution was already durably stopped".into(),
                }
            }
            CodingSessionLifecycleAction::SessionResume { .. } => {
                LifecycleDecision::Resume(ResumePlan {
                    command_id: payload.command_id,
                    channel_id,
                    target: session.clone(),
                })
            }
            CodingSessionLifecycleAction::SessionStop { .. } => LifecycleDecision::Stop(StopPlan {
                command_id: payload.command_id,
                channel_id,
                target: session.clone(),
            }),
            CodingSessionLifecycleAction::SessionCreate { .. } => unreachable!(),
        };
    }

    let CodingSessionLifecycleAction::SessionCreate {
        project_ref,
        repo_ref,
        session_ref,
        genesis_ref,
        provider_instance_ref,
        provider_authority_pubkey: _,
        model,
        title,
        initial_turn,
        actor,
        role,
    } = &payload.action
    else {
        unreachable!()
    };

    // The command is addressed to *this* signer, so no other process will ever
    // answer it. A ref naming no descriptor therefore fails loudly — silence
    // would strand the consumer's durable create forever.
    if !context
        .runtimes
        .iter()
        .any(|descriptor| &descriptor.instance_ref == provider_instance_ref)
    {
        let mut offered: Vec<&str> = context
            .runtimes
            .iter()
            .map(|descriptor| descriptor.instance_ref.as_str())
            .collect();
        offered.sort_unstable();
        return LifecycleDecision::Fail {
            command_id: payload.command_id.clone(),
            code: PROVIDER_UNAVAILABLE,
            message: format!(
                "unknown providerInstanceRef {provider_instance_ref:?}; this provider offers: {}",
                offered.join(", ")
            ),
        };
    }

    if context.max_sessions != crate::config::UNLIMITED_MAX_SESSIONS
        && context.active_session_count >= context.max_sessions
    {
        // The count is live adapter *processes* on this one provider, not
        // durable sessions and nothing to do with the model vendor's own
        // limits — a distinction the old sentence left to the reader, who
        // reasonably read "maximum of 4 session(s)" as an account limit
        // (reported 2026-08-24). Say whose cap it is and what clears it.
        return LifecycleDecision::Fail {
            command_id: payload.command_id.clone(),
            code: SESSION_LIMIT,
            message: format!(
                "this provider already holds its maximum of {} running agent process(es); stop an execution you are finished with to free a slot, or set BUZZ_CSP_MAX_SESSIONS to raise the cap",
                context.max_sessions
            ),
        };
    }

    // Custody before anything expensive: a seat this host does not hold is a
    // refusal, never an execution that is labelled as an agent and cannot act
    // as one. The check reads presence and identity out of the custody file
    // and nothing else — the key itself is fetched at spawn time.
    if let Some(actor) = actor {
        match context.actor_seats.seat(&payload.command_id) {
            Some(seat) if seat.pubkey == *actor => {}
            Some(_) => {
                return LifecycleDecision::Fail {
                    command_id: payload.command_id.clone(),
                    code: ACTOR_UNAVAILABLE,
                    message: "the agent seat this host holds for this create names a different \
                              pubkey than the create's actor"
                        .into(),
                };
            }
            None => {
                return LifecycleDecision::Fail {
                    command_id: payload.command_id.clone(),
                    code: ACTOR_UNAVAILABLE,
                    message: format!(
                        "this host holds no key material for agent {actor}; an agent seat's key \
                         never travels on the wire, so the create must be published from the host \
                         that holds it"
                    ),
                };
            }
        }
    }

    let Some(cwd) =
        context
            .projects
            .resolve(&payload.command_id, project_ref.as_deref(), channel_id)
    else {
        return LifecycleDecision::Fail {
            command_id: payload.command_id.clone(),
            code: PROJECT_CWD_UNRESOLVED,
            message: match project_ref {
                Some(project_ref) => format!(
                    "no working directory is configured for project {project_ref} or channel {channel_id}"
                ),
                None => format!("no working directory is configured for channel {channel_id}"),
            },
        };
    };

    LifecycleDecision::Create(Box::new(CreatePlan {
        command_id: payload.command_id.clone(),
        runtime_instance_ref: provider_instance_ref.clone(),
        channel_id,
        cwd,
        project_ref: project_ref.clone(),
        repo_ref: repo_ref.clone(),
        session_ref: session_ref.clone(),
        genesis_ref: genesis_ref.clone(),
        founder_pubkey: context.operator_pubkey.to_owned(),
        model: model.clone(),
        title: title.clone(),
        initial_turn: initial_turn.clone(),
        actor: actor.clone(),
        role: role.clone(),
    }))
}

/// Decide what a 44220 turn command means for this provider.
pub fn decide_turn(context: &CommandContext<'_>, created_at: u64, content: &str) -> TurnDecision {
    let command = match decode_turn_command(content) {
        Ok(command) => command,
        Err(error) => return TurnDecision::Ignore(Ignored::Malformed(error)),
    };

    // Any driver this provider's runtimes mint is acceptable; the session id
    // (a UUID) plus generation fence everything downstream, so two runtimes
    // sharing a driver slug cannot misroute a turn.
    let known_driver = context
        .runtimes
        .iter()
        .any(|descriptor| descriptor.driver == command.target.driver);
    if !known_driver || command.target.instance_id != context.instance_id {
        return TurnDecision::Ignore(Ignored::NotAddressed);
    }
    if context.state.is_command_consumed(&command.command_id) {
        return TurnDecision::Ignore(Ignored::AlreadyConsumed);
    }
    if context.state.is_command_refused(&command.command_id) {
        return TurnDecision::Ignore(Ignored::AlreadyRefused);
    }
    if context.in_flight.contains_key(&command.command_id) {
        return TurnDecision::Ignore(Ignored::AlreadyAccepted);
    }
    // Same fence, other shape: a cancel this process already put in an actor's
    // mailbox and could not durably record. Issuing it twice destroys work
    // nobody asked to stop.
    if context.delivered_cancels.contains(&command.command_id) {
        return TurnDecision::Ignore(Ignored::AlreadyAccepted);
    }
    if context.past_horizon(created_at) {
        return TurnDecision::Ignore(Ignored::PastHorizon);
    }

    let Some(record) = context.state.session(&command.target.session_id) else {
        return TurnDecision::Ignore(Ignored::UnknownTarget {
            command_id: command.command_id,
            target: command.target,
        });
    };
    if record.generation != command.target.generation {
        return TurnDecision::Ignore(Ignored::StaleGeneration {
            command_id: command.command_id,
            target: command.target,
        });
    }
    if !operator_may_steer(record, context.operator_pubkey) {
        return TurnDecision::Fail {
            command_id: command.command_id,
            target: command.target,
            code: UNAUTHORIZED_OPERATOR,
            message: "only the session founder or a granted operator may steer this execution"
                .into(),
        };
    }
    if record.closed {
        return TurnDecision::Ignore(Ignored::SessionClosed {
            command_id: command.command_id,
            target: command.target,
        });
    }
    // D9: the umbrella's allowance, checked only for a turn that would *start*
    // work. An interrupt is deliberately outside the gate — cancelling spends
    // nothing, and a crew that could not stop its own runaway turn once the
    // budget ran out would be strictly worse off for having a budget.
    if matches!(command.action, TurnAction::Start { .. }) {
        if let Some((used, limit)) = exhausted_turn_budget(context, record) {
            return TurnDecision::Fail {
                command_id: command.command_id,
                target: command.target,
                code: BUDGET_EXHAUSTED,
                message: format!(
                    "this team session has started {used} of its {limit} allowed turns; the \
                     session founder can still send turns, and raising \"Turns per team \
                     session\" takes effect the next time the provider starts"
                ),
            };
        }
    }

    match command.action {
        // Interrupt-class delivery cancels work someone else may be watching,
        // so it stays with the founder in this slice. A granted operator can
        // still steer and still queue; what it cannot do is stop a running
        // turn by sending a new one. Refusing out loud (rather than silently
        // downgrading to boundary) is the point: a control that quietly does
        // something milder than it says is the class of lie this project
        // treats as a bug.
        //
        // Narrow, and the message says so. This gate covers the *class* on a
        // start; a plain `TurnAction::Interrupt` goes through
        // `operator_may_steer` above, so a granted operator reaches the same
        // effect in two commands. Making the command founder-only too would
        // withdraw a capability grantees have today, which is authority work
        // (plan D7), not delivery work — so the refusal points at the route
        // that exists instead of pretending it does not.
        TurnAction::Start {
            deliver: CodingSessionDelivery::Interrupt,
            ..
        } if !operator_may_interrupt(context, record) => TurnDecision::Fail {
            command_id: command.command_id,
            target: command.target,
            code: UNAUTHORIZED_OPERATOR,
            message: "only the session founder, or an operator holding the umbrella's lead seat, \
                      may send an interrupt-class turn; send it as boundary or steer, or send a \
                      separate thread.turn.interrupt to cancel the running turn first"
                .into(),
        },
        TurnAction::Start { text, deliver } => TurnDecision::Start {
            command_id: command.command_id,
            target: command.target,
            text,
            deliver,
        },
        TurnAction::Interrupt => TurnDecision::Interrupt {
            command_id: command.command_id,
            target: command.target,
        },
    }
}

/// Owner-only authority: stop/resume/end. Checks only authority facts
/// persisted when this provider witnessed the create. Old no-genesis records
/// predate that field and remain ungoverned; genesis-bearing records can
/// never fall open when their founder is absent. `grant-operator` never moves
/// ownership, so the granted-operator set is deliberately not consulted here.
fn operator_owns_session(record: &crate::state::SessionRecord, operator_pubkey: &str) -> bool {
    match record.founder_pubkey.as_deref() {
        Some(founder) => founder == operator_pubkey,
        None => record.genesis_ref.is_none(),
    }
}

/// Steering authority: turn start/interrupt. The owner always may; beyond
/// that, only a genesis-bearing session consults its verified
/// granted-operator cache (each entry applied from a relay-signed acceptance
/// receipt plus the resolved accepted transition — see [`crate::authority`]).
/// Legacy no-genesis sessions never gain operators this way (R20): umbrella
/// authority for them arrives by adoption, not provider inference.
fn operator_may_steer(record: &crate::state::SessionRecord, operator_pubkey: &str) -> bool {
    if operator_owns_session(record, operator_pubkey) {
        return true;
    }
    record.genesis_ref.is_some() && record.granted_operators.contains(operator_pubkey)
}

/// The umbrella allowance this turn would exceed, as `(used, limit)`, or
/// `None` when the turn is within budget or outside the budget's reach.
///
/// A thin wrapper over [`exhausted_umbrella_budget`] so the decision path and
/// the create path answer the same question from the same facts.
fn exhausted_turn_budget(
    context: &CommandContext<'_>,
    record: &crate::state::SessionRecord,
) -> Option<(u64, u64)> {
    exhausted_umbrella_budget(
        context.state,
        context.turn_budget,
        record.session_ref.as_deref(),
        context.operator_pubkey,
    )
}

/// The umbrella allowance a turn by `operator_pubkey` would exceed, as
/// `(used, limit)`, or `None` when it is within budget or outside the
/// budget's reach.
///
/// Four ways a turn is outside its reach, and each is a fact rather than a
/// tolerance: the host set no budget (`UNLIMITED_TURN_BUDGET`); the execution
/// claimed no umbrella, so there is no crew to bound; the signer founded the
/// *umbrella*, because a budget bounds delegated work and the founder is who
/// it was protecting; or the umbrella has simply not spent its allowance yet.
///
/// The exemption is deliberately the umbrella's founder
/// ([`crate::state::StateStore::umbrella_founder`]) and not each execution's
/// own. A delegated seat may create a session, which makes it that session's
/// founder; exempting an execution's own founder would let one signed create
/// buy an unbounded allowance while still charging the umbrella. Nor is it
/// keyed to a genesis: the desktop mints a `sessionRef` for every 1:1 coding
/// session with no genesis at all, and those founders must stay exempt.
pub fn exhausted_umbrella_budget(
    state: &StateStore,
    limit: u64,
    session_ref: Option<&str>,
    operator_pubkey: &str,
) -> Option<(u64, u64)> {
    if limit == crate::config::UNLIMITED_TURN_BUDGET {
        return None;
    }
    let session_ref = session_ref?;
    if state.umbrella_founder(session_ref).as_deref() == Some(operator_pubkey) {
        return None;
    }
    let used = state.turns_used(session_ref);
    (used >= limit).then_some((used, limit))
}

/// The role slug that carries interrupt authority within an umbrella (D7).
pub const LEAD_ROLE: &str = "lead";

/// Interrupt-class authority: the founder always, and — since the agent-seat
/// amendment — a granted operator who *is* the umbrella's `lead` seat.
///
/// Two facts, both already verified locally, and neither of them claimed by
/// the sender:
///
/// 1. `record.granted_operators` contains the signer. Each entry was applied
///    from a relay-signed 40099 acceptance receipt plus the resolved
///    transition ([`crate::authority`]) — the same chain `operator_may_steer`
///    consults, so this never widens *who* may steer, only what a steer may
///    do.
/// 2. Some execution **this provider owns**, under the same umbrella
///    (`session_ref`), is seated on that same pubkey with `role == "lead"`
///    and is *still live*. The role is a fact this provider witnessed on a
///    create it accepted and persisted itself, not something the interrupting
///    command asserts — and [`crate::state::SessionStore::sessions`] yields
///    every record ever minted, so the liveness filter is what makes stopping
///    a lead seat actually withdraw its authority instead of leaving a
///    retired pubkey able to cancel a live sibling's turn.
///
/// The second fact is deliberately provider-local. A lead seated on *another*
/// host is not recognized here, which is a real limit and the honest one: this
/// provider can verify a role it minted, and cannot verify a 44223 claim
/// signed by a provider it does not trust for authority. Founder authority is
/// untouched either way — stop, resume, and end never move to a seat.
fn operator_may_interrupt(
    context: &CommandContext<'_>,
    record: &crate::state::SessionRecord,
) -> bool {
    if operator_owns_session(record, context.operator_pubkey) {
        return true;
    }
    if record.genesis_ref.is_none() || !record.granted_operators.contains(context.operator_pubkey) {
        return false;
    }
    let Some(session_ref) = record.session_ref.as_deref() else {
        // No umbrella means no siblings, so there is no lead seat to hold.
        return false;
    };
    context.state.sessions().any(|sibling| {
        !sibling.closed
            && sibling.session_ref.as_deref() == Some(session_ref)
            && sibling.actor.as_deref() == Some(context.operator_pubkey)
            && sibling.role.as_deref() == Some(LEAD_ROLE)
    })
}

/// A decoded 44220 payload, covering both donor actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnCommand {
    /// Client-generated idempotency key.
    pub command_id: String,
    /// The exact generation addressed.
    pub target: CodingSessionTarget,
    /// The requested action.
    pub action: TurnAction,
}

/// The two turn actions the donor contract defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnAction {
    /// Start a turn with operator-entered text.
    Start {
        /// The prompt.
        text: String,
        /// How the sender asked for it to be delivered.
        deliver: CodingSessionDelivery,
    },
    /// Cancel the in-flight turn.
    Interrupt,
}

/// Strictly decode a 44220 payload of either action.
pub fn decode_turn_command(content: &str) -> Result<TurnCommand, String> {
    let payload: CodingSessionCommandPayload = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session command payload: {error}"))?;
    payload.validate()?;
    let action = match payload.action {
        CodingSessionAction::ThreadTurnStart { text, deliver } => {
            TurnAction::Start { text, deliver }
        }
        CodingSessionAction::ThreadTurnInterrupt => TurnAction::Interrupt,
    };
    Ok(TurnCommand {
        command_id: payload.command_id,
        target: payload.target,
        action,
    })
}

/// Host-local map from session coordinates to working directories.
///
/// This file is the entire seam between signed intent and the machine a session
/// actually runs on. It is written by the desktop host, re-read on every
/// lifecycle command (so an operator can fix a missing entry and republish
/// without restarting the provider), and never travels anywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectsFile {
    /// Format version. Reserved; unknown versions are still read best-effort.
    pub version: u32,
    /// One-shot hint keyed by `commandId`, for a directory chosen at create time.
    pub pending: BTreeMap<String, PathBuf>,
    /// Directory per NIP-MP project coordinate.
    pub projects: BTreeMap<String, PathBuf>,
    /// Fallback directory per channel.
    pub channels: BTreeMap<Uuid, PathBuf>,
}

impl ProjectsFile {
    /// Read the file, treating every failure as "no entries".
    ///
    /// A missing or malformed projects file must not take the provider down: it
    /// degrades every affected create into a `PROJECT_CWD_UNRESOLVED` receipt,
    /// which is a message the operator can act on, unlike a dead process.
    pub fn load(path: Option<&Path>) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        let body = match std::fs::read_to_string(path) {
            Ok(body) => body,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(error) => {
                tracing::warn!(target: "csp::projects", "cannot read {}: {error}", path.display());
                return Self::default();
            }
        };
        match serde_json::from_str(&body) {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(target: "csp::projects", "cannot parse {}: {error}", path.display());
                Self::default()
            }
        }
    }

    /// Resolve a working directory: pending hint, then project, then channel.
    ///
    /// A configured path that is not an existing absolute directory resolves to
    /// `None` — the same outcome as no entry at all — because handing a relative
    /// or missing path to `session/new` fails later and less legibly.
    pub fn resolve(
        &self,
        command_id: &str,
        project_ref: Option<&str>,
        channel_id: Uuid,
    ) -> Option<PathBuf> {
        let candidates = [
            self.pending.get(command_id),
            project_ref.and_then(|project_ref| self.projects.get(project_ref)),
            self.channels.get(&channel_id),
        ];
        candidates
            .into_iter()
            .flatten()
            .find(|path| usable_directory(path))
            .cloned()
    }

    /// Project coordinates advertised in the catalog's optional `projects[]`.
    pub fn project_refs(&self) -> impl Iterator<Item = &str> {
        self.projects.keys().map(String::as_str)
    }
}

fn usable_directory(path: &Path) -> bool {
    if !path.is_absolute() {
        tracing::warn!(target: "csp::projects", "ignoring relative working directory {}", path.display());
        return false;
    }
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => true,
        Ok(_) => {
            tracing::warn!(target: "csp::projects", "ignoring non-directory working directory {}", path.display());
            false
        }
        Err(_) => {
            tracing::warn!(target: "csp::projects", "ignoring missing working directory {}", path.display());
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{SessionRecord, StateStore};

    const AUTHORITY: &str = "ab00000000000000000000000000000000000000000000000000000000000000";

    fn store(dir: &Path) -> StateStore {
        StateStore::open(dir, 86_400).expect("state")
    }

    fn runtime(instance_ref: &str, driver: &str, runtime: &str) -> RuntimeDescriptor {
        RuntimeDescriptor {
            instance_ref: instance_ref.to_owned(),
            driver: driver.to_owned(),
            runtime: runtime.to_owned(),
            agent_command: driver.to_owned(),
            agent_args: Vec::new(),
            cli_env: None,
            default_model: "default".into(),
            allowed_models: vec!["default".into()],
            discover_models: false,
            capabilities: None,
        }
    }

    fn runtimes() -> &'static [RuntimeDescriptor] {
        static RUNTIMES: std::sync::OnceLock<Vec<RuntimeDescriptor>> = std::sync::OnceLock::new();
        RUNTIMES.get_or_init(|| {
            vec![
                runtime("claude-primary", "claude-agent-acp", "claude"),
                runtime("codex-primary", "codex-acp", "codex"),
            ]
        })
    }

    /// No agent seats: the shape of every test that predates crew seats.
    fn no_actor_seats() -> &'static crate::actor_seats::ActorSeatsFile {
        static EMPTY: std::sync::OnceLock<crate::actor_seats::ActorSeatsFile> =
            std::sync::OnceLock::new();
        EMPTY.get_or_init(crate::actor_seats::ActorSeatsFile::default)
    }

    fn ctx<'a>(
        state: &'a StateStore,
        projects: &'a ProjectsFile,
        now_secs: u64,
    ) -> CommandContext<'a> {
        ctx_as(state, projects, now_secs, AUTHORITY)
    }

    fn ctx_as<'a>(
        state: &'a StateStore,
        projects: &'a ProjectsFile,
        now_secs: u64,
        operator_pubkey: &'a str,
    ) -> CommandContext<'a> {
        ctx_seated(state, projects, no_actor_seats(), now_secs, operator_pubkey)
    }

    fn ctx_seated<'a>(
        state: &'a StateStore,
        projects: &'a ProjectsFile,
        actor_seats: &'a crate::actor_seats::ActorSeatsFile,
        now_secs: u64,
        operator_pubkey: &'a str,
    ) -> CommandContext<'a> {
        CommandContext {
            provider_pubkey: AUTHORITY,
            operator_pubkey,
            runtimes: runtimes(),
            instance_id: "instance-1",
            now_secs,
            horizon_secs: 86_400,
            max_sessions: 4,
            // Unbudgeted by default: every decision test that predates D9
            // describes a provider with no crew budget configured.
            turn_budget: crate::config::UNLIMITED_TURN_BUDGET,
            active_session_count: state.live_session_count(),
            state,
            projects,
            actor_seats,
            in_flight: no_commands_in_flight(),
            delivered_cancels: no_delivered_cancels(),
        }
    }

    /// The empty accepted-not-started set, leaked once so every decision test
    /// can borrow it for `'a` without threading a local through each call.
    fn no_commands_in_flight() -> &'static HashMap<String, crate::InFlightTurn> {
        static EMPTY: std::sync::OnceLock<HashMap<String, crate::InFlightTurn>> =
            std::sync::OnceLock::new();
        EMPTY.get_or_init(HashMap::new)
    }

    /// The empty delivered-cancel queue, leaked once for the same reason.
    fn no_delivered_cancels() -> &'static VecDeque<String> {
        static EMPTY: std::sync::OnceLock<VecDeque<String>> = std::sync::OnceLock::new();
        EMPTY.get_or_init(VecDeque::new)
    }

    fn create_content(command_id: &str, project_ref: &str, authority: &str) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"{command_id}","action":{{"type":"session.create","projectRef":{project_ref},"repoRef":null,"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{authority}","model":"claude-sonnet-4-6","title":"Ship it","initialTurn":"go"}}}}"#
        )
    }

    fn turn_content(command_id: &str, session_id: &str, generation: u64) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-command/v1","commandId":"{command_id}","target":{{"driver":"claude-agent-acp","instanceId":"instance-1","sessionId":"{session_id}","generation":{generation}}},"action":{{"type":"thread.turn.start","text":"do the thing"}}}}"#
        )
    }

    /// The target a `turn_content` / `interrupt_content` command addresses.
    fn turn_target(session_id: &str, generation: u64) -> CodingSessionTarget {
        CodingSessionTarget {
            driver: "claude-agent-acp".into(),
            instance_id: "instance-1".into(),
            session_id: session_id.into(),
            generation,
        }
    }

    fn interrupt_content(command_id: &str, session_id: &str, generation: u64) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-command/v1","commandId":"{command_id}","target":{{"driver":"claude-agent-acp","instanceId":"instance-1","sessionId":"{session_id}","generation":{generation}}},"action":{{"type":"thread.turn.interrupt"}}}}"#
        )
    }

    fn lifecycle_target_content(
        action: &str,
        command_id: &str,
        session_id: &str,
        generation: u64,
    ) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"{command_id}","action":{{"type":"{action}","session":{{"driver":"claude-agent-acp","instanceId":"instance-1","sessionId":"{session_id}","generation":{generation}}},"providerAuthorityPubkey":"{AUTHORITY}"}}}}"#
        )
    }

    fn session(session_id: &str, cwd: &Path) -> SessionRecord {
        SessionRecord {
            session_id: session_id.to_owned(),
            generation: 1,
            channel_id: Uuid::nil(),
            command_id: "create-1".into(),
            generation_command_id: None,
            provider_instance_ref: "claude-primary".into(),
            runtime: "claude".into(),
            driver: "claude-agent-acp".into(),
            cwd: cwd.to_path_buf(),
            project_ref: None,
            repo_ref: None,
            session_ref: None,
            genesis_ref: None,
            actor: None,
            role: None,
            founder_pubkey: Some(AUTHORITY.into()),
            granted_operators: std::collections::BTreeSet::new(),
            granted_viewers: std::collections::BTreeSet::new(),
            authority_seq: 0,
            model: None,
            resume_cursor: None,
            title: None,
            created_at_ms: 0,
            next_seq: 1,
            next_lease_sequence: 1,
            bootstrap_transport: None,
            open_turn: None,
            closed: false,
        }
    }

    fn projects_with_channel(channel_id: Uuid, cwd: &Path) -> ProjectsFile {
        ProjectsFile {
            version: 1,
            channels: [(channel_id, cwd.to_path_buf())].into_iter().collect(),
            ..ProjectsFile::default()
        }
    }

    #[test]
    fn a_create_for_another_authority_is_ignored_without_consuming_the_command_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        let context = ctx(&state, &projects, 1_000);
        let other = "cd".repeat(32);
        assert_eq!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &create_content("create-1", "null", &other)
            ),
            LifecycleDecision::Ignore(Ignored::NotAddressed)
        );
    }

    /// Authority matched but the ref names no descriptor: nobody else will ever
    /// answer this command, so it fails loudly instead of being ignored.
    #[test]
    fn a_create_for_an_unknown_instance_ref_fails_with_provider_unavailable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        let context = ctx(&state, &projects, 1_000);
        let content = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"providerInstanceRef":"ghost-primary","providerAuthorityPubkey":"{AUTHORITY}","model":null,"title":null,"initialTurn":null}}}}"#
        );
        match decide_lifecycle(&context, Uuid::nil(), 1_000, &content) {
            LifecycleDecision::Fail {
                command_id,
                code,
                message,
            } => {
                assert_eq!(command_id, "create-1");
                assert_eq!(code, PROVIDER_UNAVAILABLE);
                assert_eq!(
                    message,
                    "unknown providerInstanceRef \"ghost-primary\"; this provider offers: \
                     claude-primary, codex-primary"
                );
            }
            other => panic!("expected a failure receipt, got {other:?}"),
        }
    }

    /// Any driver in the runtime set is addressable; the session UUID does the
    /// rest of the routing.
    #[test]
    fn turns_for_any_offered_driver_are_addressed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);

        let codex_turn = r#"{"schema":"buzz-coding-session-command/v1","commandId":"turn-1","target":{"driver":"codex-acp","instanceId":"instance-1","sessionId":"s1","generation":1},"action":{"type":"thread.turn.start","text":"go"}}"#;
        assert!(matches!(
            decide_turn(&context, 1_000, codex_turn),
            TurnDecision::Start { .. }
        ));

        let alien_turn = r#"{"schema":"buzz-coding-session-command/v1","commandId":"turn-2","target":{"driver":"someone-elses-acp","instanceId":"instance-1","sessionId":"s1","generation":1},"action":{"type":"thread.turn.start","text":"go"}}"#;
        assert_eq!(
            decide_turn(&context, 1_000, alien_turn),
            TurnDecision::Ignore(Ignored::NotAddressed)
        );
    }

    #[test]
    fn a_create_resolves_its_working_directory_in_pending_project_channel_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pending_dir = dir.path().join("pending");
        let project_dir = dir.path().join("project");
        let channel_dir = dir.path().join("channel");
        for path in [&pending_dir, &project_dir, &channel_dir] {
            std::fs::create_dir_all(path).expect("mkdir");
        }
        let channel = Uuid::new_v4();
        let project_ref = format!("30621:{}:demo", "cd".repeat(32));
        let state = store(dir.path());

        let all = ProjectsFile {
            version: 1,
            pending: [("create-1".to_owned(), pending_dir.clone())]
                .into_iter()
                .collect(),
            projects: [(project_ref.clone(), project_dir.clone())]
                .into_iter()
                .collect(),
            channels: [(channel, channel_dir.clone())].into_iter().collect(),
        };
        let content = create_content("create-1", &format!("\"{project_ref}\""), AUTHORITY);

        let resolved = |projects: &ProjectsFile| match decide_lifecycle(
            &ctx(&state, projects, 1_000),
            channel,
            1_000,
            &content,
        ) {
            LifecycleDecision::Create(plan) => plan.cwd,
            other => panic!("expected a create plan, got {other:?}"),
        };
        assert_eq!(resolved(&all), pending_dir);

        let without_pending = ProjectsFile {
            pending: BTreeMap::new(),
            ..all.clone()
        };
        assert_eq!(resolved(&without_pending), project_dir);

        let channel_only = ProjectsFile {
            pending: BTreeMap::new(),
            projects: BTreeMap::new(),
            ..all.clone()
        };
        assert_eq!(resolved(&channel_only), channel_dir);
    }

    /// Fork amendment: a create may claim an umbrella via `sessionRef`. The
    /// plan carries it verbatim; the historical 8-key form plans `None` — the
    /// exact behavior of every create before the field existed.
    #[test]
    fn a_create_plans_the_session_ref_it_claimed_and_none_otherwise() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        let context = ctx(&state, &projects, 1_000);

        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let claiming = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"sessionRef":"{umbrella}","providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{AUTHORITY}","model":null,"title":null,"initialTurn":null}}}}"#
        );
        match decide_lifecycle(&context, Uuid::nil(), 1_000, &claiming) {
            LifecycleDecision::Create(plan) => {
                assert_eq!(plan.session_ref.as_deref(), Some(umbrella));
            }
            other => panic!("expected a create plan, got {other:?}"),
        }

        match decide_lifecycle(
            &context,
            Uuid::nil(),
            1_000,
            &create_content("create-2", "null", AUTHORITY),
        ) {
            LifecycleDecision::Create(plan) => assert!(plan.session_ref.is_none()),
            other => panic!("expected a create plan, got {other:?}"),
        }
    }

    #[test]
    fn an_unresolvable_working_directory_fails_the_create_rather_than_guessing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);
        match decide_lifecycle(
            &context,
            Uuid::new_v4(),
            1_000,
            &create_content("create-1", "null", AUTHORITY),
        ) {
            LifecycleDecision::Fail { code, .. } => assert_eq!(code, PROJECT_CWD_UNRESOLVED),
            other => panic!("expected a failure receipt, got {other:?}"),
        }
    }

    /// A relative path, a missing directory, or a file where a directory was
    /// expected are all indistinguishable from "unconfigured" as far as the
    /// operator's next action goes — the create must fail legibly, not later.
    #[test]
    fn unusable_configured_paths_are_treated_as_unconfigured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, b"x").expect("write");
        let channel = Uuid::new_v4();

        for path in [
            PathBuf::from("relative/path"),
            dir.path().join("does-not-exist"),
            file,
        ] {
            let projects = projects_with_channel(channel, &path);
            assert!(projects.resolve("create-1", None, channel).is_none());
        }
    }

    #[test]
    fn a_replayed_create_produces_no_second_side_effect() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state.consume_command("create-1", 1_000).expect("consume");
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        assert_eq!(
            decide_lifecycle(
                &ctx(&state, &projects, 1_000),
                Uuid::nil(),
                1_000,
                &create_content("create-1", "null", AUTHORITY)
            ),
            LifecycleDecision::Ignore(Ignored::AlreadyConsumed)
        );
    }

    #[test]
    fn resume_and_stop_are_fenced_to_the_exact_persisted_generation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);

        assert!(matches!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.resume", "resume-1", "s1", 1),
            ),
            LifecycleDecision::Resume(ResumePlan { target, .. }) if target.generation == 1
        ));
        assert!(matches!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.stop", "stop-1", "s1", 1),
            ),
            LifecycleDecision::Stop(StopPlan { target, .. }) if target.generation == 1
        ));
        for (action, command_id) in [
            ("session.resume", "resume-stale"),
            ("session.stop", "stop-stale"),
        ] {
            match decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content(action, command_id, "s1", 2),
            ) {
                LifecycleDecision::Fail {
                    command_id: actual_command_id,
                    code,
                    message,
                } => {
                    assert_eq!(actual_command_id, command_id);
                    assert_eq!(code, STALE_GENERATION);
                    assert_eq!(
                        message,
                        "the addressed execution generation 2 is stale; the current generation is 1"
                    );
                }
                other => panic!("expected a stale-generation receipt, got {other:?}"),
            }
        }
    }

    #[test]
    fn addressed_unknown_lifecycle_targets_fail_loudly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);

        for (action, command_id) in [
            ("session.resume", "resume-unknown"),
            ("session.stop", "stop-unknown"),
        ] {
            match decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content(action, command_id, "missing", 1),
            ) {
                LifecycleDecision::Fail {
                    command_id: actual_command_id,
                    code,
                    message,
                } => {
                    assert_eq!(actual_command_id, command_id);
                    assert_eq!(code, UNKNOWN_TARGET);
                    assert_eq!(
                        message,
                        "this provider has no record of the addressed execution"
                    );
                }
                other => panic!("expected an unknown-target receipt, got {other:?}"),
            }
        }
    }

    #[test]
    fn target_refusals_do_not_override_addressing_dedupe_or_horizon_silence() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .consume_command("stop-replayed", 900)
            .expect("consume command");
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);

        let other_instance =
            lifecycle_target_content("session.stop", "stop-other-instance", "missing", 1)
                .replace("\"instance-1\"", "\"instance-2\"");
        assert_eq!(
            decide_lifecycle(&context, Uuid::nil(), 1_000, &other_instance),
            LifecycleDecision::Ignore(Ignored::NotAddressed)
        );
        assert_eq!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.stop", "stop-replayed", "missing", 1),
            ),
            LifecycleDecision::Ignore(Ignored::AlreadyConsumed)
        );

        let old_context = ctx(&state, &projects, 100_000);
        assert_eq!(
            decide_lifecycle(
                &old_context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.stop", "stop-old", "missing", 1),
            ),
            LifecycleDecision::Ignore(Ignored::PastHorizon)
        );
    }

    #[test]
    fn a_target_with_the_wrong_channel_or_driver_fails_as_unknown() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        let projects = ProjectsFile::default();
        let wrong_channel = Uuid::new_v4();
        match decide_lifecycle(
            &ctx(&state, &projects, 1_000),
            wrong_channel,
            1_000,
            &lifecycle_target_content("session.stop", "stop-wrong-channel", "s1", 1),
        ) {
            LifecycleDecision::Fail { code, .. } => assert_eq!(code, UNKNOWN_TARGET),
            other => panic!("expected an unknown-target receipt, got {other:?}"),
        }

        let mut wrong_driver = session("s2", dir.path());
        wrong_driver.driver = "codex-acp".into();
        state.insert_session(wrong_driver).expect("insert");
        match decide_lifecycle(
            &ctx(&state, &projects, 1_000),
            Uuid::nil(),
            1_000,
            &lifecycle_target_content("session.resume", "resume-wrong-driver", "s2", 1),
        ) {
            LifecycleDecision::Fail { code, .. } => assert_eq!(code, UNKNOWN_TARGET),
            other => panic!("expected an unknown-target receipt, got {other:?}"),
        }
    }

    #[test]
    fn pre_authority_legacy_records_stay_ungoverned_but_genesis_never_falls_open() {
        let dir = tempfile::tempdir().expect("tempdir");
        let projects = ProjectsFile::default();

        let mut legacy_state = store(&dir.path().join("legacy"));
        let mut legacy = session("legacy", dir.path());
        legacy.founder_pubkey = None;
        legacy_state.insert_session(legacy).expect("insert legacy");
        assert!(matches!(
            decide_turn(
                &ctx(&legacy_state, &projects, 1_000),
                1_000,
                &turn_content("turn-legacy", "legacy", 1),
            ),
            TurnDecision::Start { .. }
        ));
        assert!(matches!(
            decide_lifecycle(
                &ctx(&legacy_state, &projects, 1_000),
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.stop", "stop-legacy", "legacy", 1),
            ),
            LifecycleDecision::Stop(_)
        ));

        let mut genesis_state = store(&dir.path().join("genesis"));
        let mut unresolved = session("genesis", dir.path());
        unresolved.genesis_ref = Some("12".repeat(32));
        unresolved.founder_pubkey = None;
        genesis_state
            .insert_session(unresolved)
            .expect("insert genesis");
        assert!(matches!(
            decide_turn(
                &ctx(&genesis_state, &projects, 1_000),
                1_000,
                &turn_content("turn-genesis", "genesis", 1),
            ),
            TurnDecision::Fail { .. }
        ));
        assert!(matches!(
            decide_lifecycle(
                &ctx(&genesis_state, &projects, 1_000),
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.stop", "stop-genesis", "genesis", 1),
            ),
            LifecycleDecision::Fail {
                code: UNAUTHORIZED_OPERATOR,
                ..
            }
        ));
    }

    /// The A5 authority split: a granted operator may steer (turn start and
    /// interrupt) a genesis-bearing session, but stop/resume stay owner-only —
    /// `grant-operator` never moves ownership.
    #[test]
    fn a_granted_operator_may_steer_but_never_stop_or_resume() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let grantee = "ef".repeat(32);
        let mut governed = session("s1", dir.path());
        governed.genesis_ref = Some("12".repeat(32));
        governed.granted_operators = [grantee.clone()].into_iter().collect();
        governed.authority_seq = 1;
        state.insert_session(governed).expect("insert");
        let projects = ProjectsFile::default();
        let context = ctx_as(&state, &projects, 1_000, &grantee);

        assert!(matches!(
            decide_turn(&context, 1_000, &turn_content("turn-grantee", "s1", 1)),
            TurnDecision::Start { .. }
        ));
        assert!(matches!(
            decide_turn(
                &context,
                1_000,
                &interrupt_content("interrupt-grantee", "s1", 1)
            ),
            TurnDecision::Interrupt { .. }
        ));
        for (action, command_id) in [
            ("session.stop", "stop-grantee"),
            ("session.resume", "resume-grantee"),
        ] {
            assert!(
                matches!(
                    decide_lifecycle(
                        &context,
                        Uuid::nil(),
                        1_000,
                        &lifecycle_target_content(action, command_id, "s1", 1),
                    ),
                    LifecycleDecision::Fail {
                        code: UNAUTHORIZED_OPERATOR,
                        ..
                    }
                ),
                "{action} from a granted operator must stay owner-only"
            );
        }
    }

    /// A channel member who is neither founder nor granted operator is
    /// refused visibly, and a granted-operator set can never open a legacy
    /// no-genesis record (R20: legacy authority is the witnessed creator).
    #[test]
    fn non_granted_members_are_refused_and_grants_never_apply_without_a_genesis() {
        let dir = tempfile::tempdir().expect("tempdir");
        let projects = ProjectsFile::default();
        let stranger = "99".repeat(32);
        let grantee = "ef".repeat(32);

        let mut governed_state = store(&dir.path().join("governed"));
        let mut governed = session("s1", dir.path());
        governed.genesis_ref = Some("12".repeat(32));
        governed.granted_operators = [grantee.clone()].into_iter().collect();
        governed.authority_seq = 1;
        governed_state.insert_session(governed).expect("insert");
        assert!(matches!(
            decide_turn(
                &ctx_as(&governed_state, &projects, 1_000, &stranger),
                1_000,
                &turn_content("turn-stranger", "s1", 1)
            ),
            TurnDecision::Fail { .. }
        ));

        // Defensive: a grant entry on a no-genesis record is inert — legacy
        // sessions acquire umbrella authority by adoption, never inference.
        let mut legacy_state = store(&dir.path().join("legacy"));
        let mut legacy = session("s2", dir.path());
        legacy.granted_operators = [grantee.clone()].into_iter().collect();
        legacy_state.insert_session(legacy).expect("insert");
        assert!(matches!(
            decide_turn(
                &ctx_as(&legacy_state, &projects, 1_000, &grantee),
                1_000,
                &turn_content("turn-inert-grant", "s2", 1)
            ),
            TurnDecision::Fail { .. }
        ));
    }

    #[test]
    fn a_durably_stopped_session_cannot_resume() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let mut stopped = session("s1", dir.path());
        stopped.closed = true;
        state.insert_session(stopped).expect("insert");
        let projects = ProjectsFile::default();
        match decide_lifecycle(
            &ctx(&state, &projects, 1_000),
            Uuid::nil(),
            1_000,
            &lifecycle_target_content("session.resume", "resume-1", "s1", 1),
        ) {
            LifecycleDecision::Fail {
                command_id,
                code,
                message,
            } => {
                assert_eq!(command_id, "resume-1");
                assert_eq!(code, SESSION_CLOSED);
                assert_eq!(
                    message,
                    "the addressed execution was already durably stopped"
                );
            }
            other => panic!("expected a session-closed receipt, got {other:?}"),
        }
    }

    #[test]
    fn a_command_older_than_the_horizon_is_history_not_intent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        let context = ctx(&state, &projects, 1_000_000);
        assert_eq!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &create_content("create-1", "null", AUTHORITY)
            ),
            LifecycleDecision::Ignore(Ignored::PastHorizon)
        );
        // Exactly at the horizon is still fresh.
        let at_horizon = ctx(&state, &projects, 1_000 + 86_400);
        assert!(matches!(
            decide_lifecycle(
                &at_horizon,
                Uuid::nil(),
                1_000,
                &create_content("create-1", "null", AUTHORITY)
            ),
            LifecycleDecision::Create(_)
        ));
    }

    #[test]
    fn the_session_cap_fails_the_create_with_session_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        for index in 0..4 {
            state
                .insert_session(session(&format!("s{index}"), dir.path()))
                .expect("insert");
        }
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        match decide_lifecycle(
            &ctx(&state, &projects, 1_000),
            Uuid::nil(),
            1_000,
            &create_content("create-1", "null", AUTHORITY),
        ) {
            LifecycleDecision::Fail { code, .. } => assert_eq!(code, SESSION_LIMIT),
            other => panic!("expected a failure receipt, got {other:?}"),
        }
    }

    /// Zero is unlimited: the very state that refuses at a ceiling of four
    /// admits when the ceiling is none (asked for 2026-08-24).
    #[test]
    fn an_unlimited_ceiling_never_refuses_for_capacity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        for index in 0..64 {
            state
                .insert_session(session(&format!("s{index}"), dir.path()))
                .expect("insert");
        }
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        let mut context = ctx(&state, &projects, 1_000);
        context.max_sessions = crate::config::UNLIMITED_MAX_SESSIONS;
        assert!(matches!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &create_content("create-1", "null", AUTHORITY),
            ),
            LifecycleDecision::Create(_)
        ));
    }

    /// Generation fencing is the whole safety story for turns: a command that
    /// names a generation this provider is not running must never reach an
    /// agent, because the operator was looking at a session that no longer
    /// exists in this form.
    #[test]
    fn turns_are_fenced_to_the_exact_live_generation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);

        assert!(matches!(
            decide_turn(&context, 1_000, &turn_content("turn-1", "s1", 1)),
            TurnDecision::Start { .. }
        ));
        assert_eq!(
            decide_turn(&context, 1_000, &turn_content("turn-2", "s1", 2)),
            TurnDecision::Ignore(Ignored::StaleGeneration {
                command_id: "turn-2".into(),
                target: turn_target("s1", 2),
            })
        );
        assert_eq!(
            decide_turn(&context, 1_000, &turn_content("turn-3", "ghost", 1)),
            TurnDecision::Ignore(Ignored::UnknownTarget {
                command_id: "turn-3".into(),
                target: turn_target("ghost", 1),
            })
        );
    }

    #[test]
    fn turns_addressed_to_another_instance_are_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        let projects = ProjectsFile::default();
        let context = CommandContext {
            instance_id: "instance-2",
            ..ctx(&state, &projects, 1_000)
        };
        assert_eq!(
            decide_turn(&context, 1_000, &turn_content("turn-1", "s1", 1)),
            TurnDecision::Ignore(Ignored::NotAddressed)
        );
    }

    #[test]
    fn a_closed_session_accepts_no_further_turns() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let mut record = session("s1", dir.path());
        record.closed = true;
        state.insert_session(record).expect("insert");
        let projects = ProjectsFile::default();
        assert_eq!(
            decide_turn(
                &ctx(&state, &projects, 1_000),
                1_000,
                &turn_content("turn-1", "s1", 1)
            ),
            TurnDecision::Ignore(Ignored::SessionClosed {
                command_id: "turn-1".into(),
                target: turn_target("s1", 1),
            })
        );
    }

    #[test]
    fn both_donor_turn_actions_decode() {
        let start = decode_turn_command(&turn_content("turn-1", "s1", 1)).expect("decode start");
        assert_eq!(
            start.action,
            TurnAction::Start {
                text: "do the thing".into(),
                deliver: CodingSessionDelivery::Boundary,
            }
        );
        let interrupt =
            decode_turn_command(&interrupt_content("turn-2", "s1", 1)).expect("decode interrupt");
        assert_eq!(interrupt.action, TurnAction::Interrupt);
        assert_eq!(interrupt.target.session_id, "s1");
    }

    #[test]
    fn malformed_turn_payloads_are_rejected() {
        for content in [
            "{}",
            r#"{"schema":"wrong","commandId":"c","target":{"driver":"d","instanceId":"i","sessionId":"s","generation":1},"action":{"type":"thread.turn.interrupt"}}"#,
            r#"{"schema":"buzz-coding-session-command/v1","commandId":"","target":{"driver":"d","instanceId":"i","sessionId":"s","generation":1},"action":{"type":"thread.turn.interrupt"}}"#,
            r#"{"schema":"buzz-coding-session-command/v1","commandId":"c","target":{"driver":"d","instanceId":"i","sessionId":"s","generation":0},"action":{"type":"thread.turn.interrupt"}}"#,
            r#"{"schema":"buzz-coding-session-command/v1","commandId":"c","target":{"driver":"d","instanceId":"i","sessionId":"s","generation":1},"action":{"type":"thread.turn.steer"}}"#,
            r#"{"schema":"buzz-coding-session-command/v1","commandId":"c","target":{"driver":"d","instanceId":"i","sessionId":"s","generation":1},"action":{"type":"thread.turn.interrupt"},"cwd":"/tmp"}"#,
        ] {
            assert!(
                decode_turn_command(content).is_err(),
                "should reject {content}"
            );
        }
    }

    #[test]
    fn interrupt_decisions_are_fenced_the_same_way_starts_are() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);
        assert!(matches!(
            decide_turn(&context, 1_000, &interrupt_content("turn-1", "s1", 1)),
            TurnDecision::Interrupt { .. }
        ));
        assert_eq!(
            decide_turn(&context, 1_000, &interrupt_content("turn-2", "s1", 9)),
            TurnDecision::Ignore(Ignored::StaleGeneration {
                command_id: "turn-2".into(),
                target: turn_target("s1", 9),
            })
        );
    }

    #[test]
    fn a_missing_or_unreadable_projects_file_degrades_to_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(ProjectsFile::load(None), ProjectsFile::default());
        assert_eq!(
            ProjectsFile::load(Some(&dir.path().join("absent.json"))),
            ProjectsFile::default()
        );
        let broken = dir.path().join("broken.json");
        std::fs::write(&broken, b"{ not json").expect("write");
        assert_eq!(ProjectsFile::load(Some(&broken)), ProjectsFile::default());
    }

    #[test]
    fn the_projects_file_schema_round_trips_from_the_host_format() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel = Uuid::new_v4();
        let path = dir.path().join("projects.json");
        std::fs::write(
            &path,
            format!(
                r#"{{"version":1,"pending":{{"create-1":"{cwd}"}},"projects":{{"30621:{owner}:demo":"{cwd}"}},"channels":{{"{channel}":"{cwd}"}}}}"#,
                cwd = dir.path().display(),
                owner = "cd".repeat(32),
            ),
        )
        .expect("write");
        let file = ProjectsFile::load(Some(&path));
        assert_eq!(file.version, 1);
        assert_eq!(
            file.resolve("create-1", None, Uuid::new_v4()).as_deref(),
            Some(dir.path())
        );
        assert_eq!(file.project_refs().count(), 1);
        assert_eq!(
            file.resolve("other", None, channel).as_deref(),
            Some(dir.path())
        );
    }

    /// `turn_content` with an explicit delivery class.
    fn turn_content_delivering(
        command_id: &str,
        session_id: &str,
        generation: u64,
        deliver: &str,
    ) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-command/v1","commandId":"{command_id}","target":{{"driver":"claude-agent-acp","instanceId":"instance-1","sessionId":"{session_id}","generation":{generation}}},"action":{{"type":"thread.turn.start","text":"do the thing","deliver":"{deliver}"}}}}"#
        )
    }

    /// The class the sender asked for reaches the decision intact, and a
    /// command that names none means `boundary` — the behaviour that existed
    /// before the field did.
    #[test]
    fn a_turns_delivery_class_survives_the_decision() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);

        for (wire, expected) in [
            ("boundary", CodingSessionDelivery::Boundary),
            ("steer", CodingSessionDelivery::Steer),
            ("interrupt", CodingSessionDelivery::Interrupt),
        ] {
            match decide_turn(
                &context,
                1_000,
                &turn_content_delivering(&format!("turn-{wire}"), "s1", 1, wire),
            ) {
                TurnDecision::Start { deliver, .. } => assert_eq!(deliver, expected),
                other => panic!("{wire} was not a start: {other:?}"),
            }
        }

        match decide_turn(&context, 1_000, &turn_content("turn-default", "s1", 1)) {
            TurnDecision::Start { deliver, .. } => {
                assert_eq!(deliver, CodingSessionDelivery::Boundary);
            }
            other => panic!("expected a boundary start, got {other:?}"),
        }

        // A class this build has never heard of is not guessed at.
        assert!(matches!(
            decide_turn(
                &context,
                1_000,
                &turn_content_delivering("turn-unknown-class", "s1", 1, "cancel")
            ),
            TurnDecision::Ignore(Ignored::Malformed(_))
        ));
    }

    /// Interrupt-class delivery stops work someone else may be watching, so a
    /// granted operator with no lead seat is refused out loud rather than
    /// quietly downgraded — a control that silently does something milder than
    /// it says is the kind of lie this project treats as a bug.
    ///
    /// The refusal is narrow, and its words have to be: the *class* needs
    /// founder or lead standing, while a plain `thread.turn.interrupt` is open
    /// to anyone who may steer. Both halves are asserted here. The lead half of
    /// the gate is pinned separately in
    /// `the_umbrellas_lead_seat_may_interrupt_and_a_builder_may_not`.
    #[test]
    fn interrupt_class_delivery_is_refused_to_a_grantee_with_no_lead_seat() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let grantee = "ef".repeat(32);
        let mut governed = session("s1", dir.path());
        governed.genesis_ref = Some("12".repeat(32));
        governed.granted_operators = [grantee.clone()].into_iter().collect();
        governed.authority_seq = 1;
        state.insert_session(governed).expect("insert");
        let projects = ProjectsFile::default();

        let as_grantee = ctx_as(&state, &projects, 1_000, &grantee);
        match decide_turn(
            &as_grantee,
            1_000,
            &turn_content_delivering("turn-grantee-interrupt", "s1", 1, "interrupt"),
        ) {
            TurnDecision::Fail {
                command_id,
                message,
                ..
            } => {
                assert_eq!(command_id, "turn-grantee-interrupt");
                // The refusal must not claim more authority than the gate has.
                // A granted operator *can* cancel a running turn — with a
                // `thread.turn.interrupt`, checked just below — so telling
                // them to ask the founder to interrupt is a comfortable
                // sentence that is not true.
                assert!(
                    !message.contains("ask the founder"),
                    "the refusal must not imply a grantee cannot interrupt: {message}"
                );
            }
            other => panic!("a granted operator must not interrupt: {other:?}"),
        }
        // The gap this refusal must not deny: the class is founder-only, the
        // *command* is not. Recorded here so nobody reads the refusal above as
        // a guarantee it does not make (NIP-CSL, and plan D7 for closing it).
        assert!(matches!(
            decide_turn(
                &as_grantee,
                1_000,
                &interrupt_content("turn-grantee-cancel", "s1", 1)
            ),
            TurnDecision::Interrupt { .. }
        ));
        // Everything else the grantee could already do is untouched.
        for class in ["boundary", "steer"] {
            assert!(
                matches!(
                    decide_turn(
                        &as_grantee,
                        1_000,
                        &turn_content_delivering(&format!("turn-grantee-{class}"), "s1", 1, class)
                    ),
                    TurnDecision::Start { .. }
                ),
                "{class} must still be allowed"
            );
        }

        let as_founder = ctx(&state, &projects, 1_000);
        assert!(matches!(
            decide_turn(
                &as_founder,
                1_000,
                &turn_content_delivering("turn-founder-interrupt", "s1", 1, "interrupt")
            ),
            TurnDecision::Start {
                deliver: CodingSessionDelivery::Interrupt,
                ..
            }
        ));
    }

    /// A command already answered — consumed, refused, or sitting in a mailbox
    /// waiting to run — is never answered a second time.
    #[test]
    fn a_command_already_answered_is_silent_on_redelivery() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        state.consume_command("turn-ran", 1_000).expect("consume");
        state.record_refusal("turn-refused", 1_000).expect("refuse");
        let projects = ProjectsFile::default();
        let mut in_flight = HashMap::new();
        in_flight.insert(
            "turn-waiting".to_owned(),
            crate::InFlightTurn {
                channel_id: Uuid::nil(),
                session_id: "s1".to_owned(),
                target: turn_target("s1", 1),
                created_at: 1_000,
            },
        );
        // A cancel already in an actor's mailbox whose ledger append failed:
        // no durable fence answers it, so this in-memory one has to.
        let delivered_cancels: VecDeque<String> = ["cancel-delivered".to_owned()].into();
        let mut context = ctx(&state, &projects, 1_000);
        context.in_flight = &in_flight;
        context.delivered_cancels = &delivered_cancels;

        for (command_id, expected) in [
            ("turn-ran", Ignored::AlreadyConsumed),
            ("turn-refused", Ignored::AlreadyRefused),
            ("turn-waiting", Ignored::AlreadyAccepted),
            ("cancel-delivered", Ignored::AlreadyAccepted),
        ] {
            let decision = decide_turn(&context, 1_000, &turn_content(command_id, "s1", 1));
            assert_eq!(decision, TurnDecision::Ignore(expected.clone()));
            assert!(
                expected.refusal().is_none(),
                "{command_id} must not be answered twice"
            );
        }
    }

    /// A budgeted context: everything `ctx_as` builds, plus a finite D9
    /// ceiling on turns started under one umbrella.
    fn ctx_budgeted<'a>(
        state: &'a StateStore,
        projects: &'a ProjectsFile,
        operator_pubkey: &'a str,
        turn_budget: u64,
    ) -> CommandContext<'a> {
        CommandContext {
            turn_budget,
            ..ctx_as(state, projects, 1_000, operator_pubkey)
        }
    }

    /// One governed execution under `umbrella`, with `grantee` holding
    /// operator standing on it. The founder is [`AUTHORITY`].
    fn seated_umbrella_session(state: &mut StateStore, dir: &Path, umbrella: &str, grantee: &str) {
        let mut record = session("s1", dir);
        record.genesis_ref = Some("12".repeat(32));
        record.session_ref = Some(umbrella.to_owned());
        record.granted_operators = [grantee.to_owned()].into_iter().collect();
        record.authority_seq = 1;
        state.insert_session(record).expect("insert");
    }

    /// D9's whole point in one test: the allowance binds delegated work and
    /// nothing else. A granted seat is refused with the code the receipt
    /// carries and the two numbers in words; the founder, whom the budget
    /// exists to protect, is not refused at all.
    #[test]
    fn a_spent_umbrella_budget_refuses_a_seat_and_never_the_founder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let seat = "ef".repeat(32);
        seated_umbrella_session(&mut state, dir.path(), umbrella, &seat);
        for _ in 0..3 {
            state.record_turn_spend(umbrella).expect("spend");
        }
        let projects = ProjectsFile::default();

        match decide_turn(
            &ctx_budgeted(&state, &projects, &seat, 3),
            1_000,
            &turn_content("turn-over-budget", "s1", 1),
        ) {
            TurnDecision::Fail {
                command_id,
                code,
                message,
                ..
            } => {
                assert_eq!(command_id, "turn-over-budget");
                assert_eq!(code, BUDGET_EXHAUSTED);
                assert!(
                    message.contains("3 of its 3 allowed turns"),
                    "the refusal must name used and limit, got {message:?}"
                );
            }
            other => panic!("expected a budget refusal, got {other:?}"),
        }

        assert!(
            matches!(
                decide_turn(
                    &ctx_budgeted(&state, &projects, AUTHORITY, 3),
                    1_000,
                    &turn_content("turn-founder-at-limit", "s1", 1),
                ),
                TurnDecision::Start { .. }
            ),
            "the founder is never refused for a budget"
        );
    }

    /// The exemption is the *umbrella's* founder, not each execution's own.
    ///
    /// A delegated seat may create a session; that makes it the founder of
    /// that session. If the budget exempted an execution's own founder, one
    /// signed create would buy the seat an unbounded allowance while still
    /// charging the umbrella — which is exactly the delegated work D9 exists
    /// to bound.
    #[test]
    fn a_seat_that_founds_its_own_execution_is_still_bound_by_the_umbrella() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let seat = "ef".repeat(32);
        // The umbrella was opened by AUTHORITY, on an execution created first.
        seated_umbrella_session(&mut state, dir.path(), umbrella, &seat);
        // The seat then creates its own execution under the same umbrella and
        // is, on that record alone, the founder.
        let mut own = session("s2", dir.path());
        own.session_ref = Some(umbrella.to_owned());
        own.genesis_ref = Some("12".repeat(32));
        own.founder_pubkey = Some(seat.clone());
        own.granted_operators = [AUTHORITY.to_owned()].into_iter().collect();
        own.authority_seq = 1;
        own.created_at_ms = 1_000;
        state.insert_session(own).expect("insert");
        for _ in 0..3 {
            state.record_turn_spend(umbrella).expect("spend");
        }
        let projects = ProjectsFile::default();

        match decide_turn(
            &ctx_budgeted(&state, &projects, &seat, 3),
            1_000,
            &turn_content("turn-self-founded", "s2", 1),
        ) {
            TurnDecision::Fail { code, .. } => assert_eq!(code, BUDGET_EXHAUSTED),
            other => panic!("a self-founded execution must not escape the budget, got {other:?}"),
        }

        assert!(
            matches!(
                decide_turn(
                    &ctx_budgeted(&state, &projects, AUTHORITY, 3),
                    1_000,
                    &turn_content("turn-umbrella-founder", "s2", 1),
                ),
                TurnDecision::Start { .. }
            ),
            "the umbrella's founder stays exempt on every execution under it"
        );
    }

    /// The desktop mints a `sessionRef` for every 1:1 coding session, with no
    /// genesis and no crew. Its founder must stay exempt: resolving the
    /// umbrella's founder may never depend on a genesis those sessions have
    /// never carried.
    #[test]
    fn a_genesisless_solo_umbrella_still_exempts_its_founder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let mut record = session("s1", dir.path());
        record.session_ref = Some(umbrella.to_owned());
        record.genesis_ref = None;
        state.insert_session(record).expect("insert");
        state.record_turn_spend(umbrella).expect("spend");
        let projects = ProjectsFile::default();

        assert!(
            matches!(
                decide_turn(
                    &ctx_budgeted(&state, &projects, AUTHORITY, 1),
                    1_000,
                    &turn_content("turn-solo-founder", "s1", 1),
                ),
                TurnDecision::Start { .. }
            ),
            "a solo session's founder is never refused for a budget"
        );
    }

    /// The three ways a turn sits outside the allowance's reach, each a fact
    /// rather than a tolerance: room left, no umbrella to bound, and a host
    /// that set no budget at all.
    #[test]
    fn the_turn_budget_binds_only_a_budgeted_umbrella_with_no_room_left() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let seat = "ef".repeat(32);
        seated_umbrella_session(&mut state, dir.path(), umbrella, &seat);
        state.record_turn_spend(umbrella).expect("spend");
        let projects = ProjectsFile::default();

        assert!(
            matches!(
                decide_turn(
                    &ctx_budgeted(&state, &projects, &seat, 3),
                    1_000,
                    &turn_content("turn-in-budget", "s1", 1),
                ),
                TurnDecision::Start { .. }
            ),
            "one of three turns spent leaves room"
        );
        assert!(
            matches!(
                decide_turn(
                    &ctx_budgeted(
                        &state,
                        &projects,
                        &seat,
                        crate::config::UNLIMITED_TURN_BUDGET
                    ),
                    1_000,
                    &turn_content("turn-unbudgeted-host", "s1", 1),
                ),
                TurnDecision::Start { .. }
            ),
            "zero is unlimited, so nothing is ever refused for a budget"
        );

        // A session that never claimed an umbrella has no crew to bound, so
        // the same spent counter cannot reach it.
        let mut unclaimed = store(dir.path());
        let mut record = session("s2", dir.path());
        record.genesis_ref = Some("12".repeat(32));
        record.granted_operators = [seat.clone()].into_iter().collect();
        record.authority_seq = 1;
        unclaimed.insert_session(record).expect("insert");
        assert!(
            matches!(
                decide_turn(
                    &ctx_budgeted(&unclaimed, &projects, &seat, 1),
                    1_000,
                    &turn_content("turn-no-umbrella", "s2", 1),
                ),
                TurnDecision::Start { .. }
            ),
            "an execution with no sessionRef is outside the budget's reach"
        );
    }

    /// A cancel spends nothing, so a spent budget must not be able to refuse
    /// one: a crew that could not stop its own runaway turn once the
    /// allowance ran out would be worse off for having an allowance.
    #[test]
    fn a_spent_budget_still_lets_a_seat_cancel_the_running_turn() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let seat = "ef".repeat(32);
        seated_umbrella_session(&mut state, dir.path(), umbrella, &seat);
        state.record_turn_spend(umbrella).expect("spend");
        let projects = ProjectsFile::default();

        assert!(
            matches!(
                decide_turn(
                    &ctx_budgeted(&state, &projects, &seat, 1),
                    1_000,
                    &interrupt_content("cancel-over-budget", "s1", 1),
                ),
                TurnDecision::Interrupt { .. }
            ),
            "an interrupt is not a turn and never spends the allowance"
        );
    }

    /// D7: interrupt-class authority extends to a granted operator who holds
    /// this umbrella's `lead` seat, and to nobody else.
    ///
    /// Both agents in this test are granted operators on the same execution,
    /// so the *only* difference between them is the role on the seat this
    /// provider minted for each. The builder is refused; the lead is not.
    /// Founder authority is asserted unchanged for both: a lead seat is not a
    /// founder, and `session.stop` proves it.
    #[test]
    fn the_umbrellas_lead_seat_may_interrupt_and_a_builder_may_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let lead = "ef".repeat(32);
        let builder = "ab".repeat(31) + "cd";

        // The execution being steered: governed, under the umbrella, with both
        // agents granted operator standing.
        let mut target = session("s1", dir.path());
        target.genesis_ref = Some("12".repeat(32));
        target.session_ref = Some(umbrella.to_owned());
        target.granted_operators = [lead.clone(), builder.clone()].into_iter().collect();
        target.authority_seq = 1;
        state.insert_session(target).expect("insert");

        // The two sibling seats, minted by this provider under the same
        // umbrella. This is where the role comes from — never from the command.
        for (session_id, actor, role) in [
            ("s-lead", &lead, "lead"),
            ("s-builder", &builder, "builder"),
        ] {
            let mut seat = session(session_id, dir.path());
            seat.genesis_ref = Some("12".repeat(32));
            seat.session_ref = Some(umbrella.to_owned());
            seat.actor = Some(actor.clone());
            seat.role = Some(role.to_owned());
            state.insert_session(seat).expect("insert seat");
        }

        let projects = ProjectsFile::default();

        let as_lead = ctx_as(&state, &projects, 1_000, &lead);
        assert!(
            matches!(
                decide_turn(
                    &as_lead,
                    1_000,
                    &turn_content_delivering("turn-lead-interrupt", "s1", 1, "interrupt")
                ),
                TurnDecision::Start {
                    deliver: CodingSessionDelivery::Interrupt,
                    ..
                }
            ),
            "the umbrella's lead seat must be allowed the interrupt class"
        );

        let as_builder = ctx_as(&state, &projects, 1_000, &builder);
        assert!(
            matches!(
                decide_turn(
                    &as_builder,
                    1_000,
                    &turn_content_delivering("turn-builder-interrupt", "s1", 1, "interrupt")
                ),
                TurnDecision::Fail { .. }
            ),
            "a builder seat must not hold interrupt-class authority"
        );

        // Founder authority never moves: the lead may cancel a turn, and may
        // not stop, resume, or end the execution.
        assert!(matches!(
            decide_lifecycle(
                &as_lead,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.stop", "stop-lead", "s1", 1),
            ),
            LifecycleDecision::Fail {
                code: UNAUTHORIZED_OPERATOR,
                ..
            }
        ));

        // And the role has to be *in this umbrella*: the same lead pubkey
        // seated under a different umbrella confers nothing here.
        let mut elsewhere = store(&dir.path().join("elsewhere"));
        let mut other_target = session("s1", dir.path());
        other_target.genesis_ref = Some("12".repeat(32));
        other_target.session_ref = Some("11111111-2222-3333-4444-555555555555".to_owned());
        other_target.granted_operators = [lead.clone()].into_iter().collect();
        other_target.authority_seq = 1;
        elsewhere.insert_session(other_target).expect("insert");
        let mut foreign_lead = session("s-lead", dir.path());
        foreign_lead.genesis_ref = Some("12".repeat(32));
        foreign_lead.session_ref = Some(umbrella.to_owned());
        foreign_lead.actor = Some(lead.clone());
        foreign_lead.role = Some("lead".to_owned());
        elsewhere.insert_session(foreign_lead).expect("insert");
        let cross = ctx_as(&elsewhere, &projects, 1_000, &lead);
        assert!(
            matches!(
                decide_turn(
                    &cross,
                    1_000,
                    &turn_content_delivering("turn-cross-umbrella", "s1", 1, "interrupt")
                ),
                TurnDecision::Fail { .. }
            ),
            "a lead seat in another umbrella must not interrupt this one"
        );
    }

    /// A lead seat that has been stopped keeps no authority over its siblings.
    ///
    /// `state.sessions()` is "every session record this provider has ever
    /// minted and not pruned" (`crate::state::SessionStore::sessions`), so a
    /// seat whose execution was durably ended is still in it, with its `actor`
    /// and `role` intact. Reading that as standing means ending a lead seat
    /// takes nothing away: the pubkey behind a process that no longer exists
    /// can still cancel a live sibling's turn, and the only way to withdraw
    /// the authority would be to revoke the grant — which is not what a
    /// person who stops a seat believes they did.
    #[test]
    fn a_retired_lead_seat_no_longer_holds_interrupt_authority() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let lead = "ef".repeat(32);

        let mut target = session("s1", dir.path());
        target.genesis_ref = Some("12".repeat(32));
        target.session_ref = Some(umbrella.to_owned());
        target.granted_operators = [lead.clone()].into_iter().collect();
        target.authority_seq = 1;
        state.insert_session(target).expect("insert");

        let mut seat = session("s-lead", dir.path());
        seat.genesis_ref = Some("12".repeat(32));
        seat.session_ref = Some(umbrella.to_owned());
        seat.actor = Some(lead.clone());
        seat.role = Some("lead".to_owned());
        seat.closed = true;
        state.insert_session(seat).expect("insert seat");

        let projects = ProjectsFile::default();
        let as_lead = ctx_as(&state, &projects, 1_000, &lead);
        assert!(
            matches!(
                decide_turn(
                    &as_lead,
                    1_000,
                    &turn_content_delivering("turn-retired-lead", "s1", 1, "interrupt")
                ),
                TurnDecision::Fail { .. }
            ),
            "a stopped lead seat must not keep interrupt-class authority"
        );

        // The grant itself is untouched: the retired seat may still steer, it
        // simply may no longer cancel a sibling's running turn.
        assert!(matches!(
            decide_turn(
                &as_lead,
                1_000,
                &turn_content_delivering("turn-retired-lead-boundary", "s1", 1, "boundary")
            ),
            TurnDecision::Start { .. }
        ));
    }

    /// D6: a create that seats an agent this host holds no key for is refused
    /// with `ACTOR_UNAVAILABLE` — never created, never spawned.
    ///
    /// Three cases, because they fail for three different reasons and an
    /// operator has to be able to tell them apart: no custody file at all, a
    /// custody file with no entry for this `commandId`, and an entry for this
    /// `commandId` that names a different pubkey than the create did.
    #[test]
    fn a_create_seating_an_actor_this_host_cannot_resolve_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let channel = Uuid::new_v4();
        let projects = projects_with_channel(channel, dir.path());
        let actor = "cd".repeat(32);

        let decide = |seats: &crate::actor_seats::ActorSeatsFile, actor_hex: &str| {
            let context = ctx_seated(&state, &projects, seats, 1_000, AUTHORITY);
            decide_lifecycle(
                &context,
                channel,
                1_000,
                &seated_create_content("create-1", actor_hex, "lead"),
            )
        };

        let empty = crate::actor_seats::ActorSeatsFile::default();
        assert!(
            matches!(
                decide(&empty, &actor),
                LifecycleDecision::Fail {
                    code: ACTOR_UNAVAILABLE,
                    ..
                }
            ),
            "a host with no custody file must refuse a seated create"
        );

        let seats_path = dir.path().join("actor-seats.json");
        std::fs::write(
            &seats_path,
            format!(
                r#"{{"version":1,"pending":{{"create-other":{{"pubkey":"{actor}","nsec":"nsec1x","authTag":null,"relayUrl":"wss://relay.example"}}}}}}"#
            ),
        )
        .expect("write seats");
        let wrong_command = crate::actor_seats::ActorSeatsFile::load(Some(&seats_path));
        assert!(matches!(
            decide(&wrong_command, &actor),
            LifecycleDecision::Fail {
                code: ACTOR_UNAVAILABLE,
                ..
            }
        ));

        std::fs::write(
            &seats_path,
            format!(
                r#"{{"version":1,"pending":{{"create-1":{{"pubkey":"{}","nsec":"nsec1x","authTag":null,"relayUrl":"wss://relay.example"}}}}}}"#,
                "ab".repeat(32)
            ),
        )
        .expect("write seats");
        let wrong_pubkey = crate::actor_seats::ActorSeatsFile::load(Some(&seats_path));
        match decide(&wrong_pubkey, &actor) {
            LifecycleDecision::Fail { code, message, .. } => {
                assert_eq!(code, ACTOR_UNAVAILABLE);
                assert!(
                    message.contains("different pubkey"),
                    "a mismatched seat must say so: {message}"
                );
            }
            other => panic!("a mismatched seat must be refused: {other:?}"),
        }

        // And the matching entry plans a create that carries the seat.
        std::fs::write(
            &seats_path,
            format!(
                r#"{{"version":1,"pending":{{"create-1":{{"pubkey":"{actor}","nsec":"nsec1x","authTag":null,"relayUrl":"wss://relay.example"}}}}}}"#
            ),
        )
        .expect("write seats");
        let held = crate::actor_seats::ActorSeatsFile::load(Some(&seats_path));
        match decide(&held, &actor) {
            LifecycleDecision::Create(plan) => {
                assert_eq!(plan.actor.as_deref(), Some(actor.as_str()));
                assert_eq!(plan.role.as_deref(), Some("lead"));
                // The plan is `Debug` and travels through the create path, so
                // it must never be able to carry the key.
                let rendered = format!("{plan:?}");
                assert!(!rendered.contains("nsec"), "{rendered}");
            }
            other => panic!("a held seat must plan a create: {other:?}"),
        }
    }

    /// An unseated create is unchanged by all of the above: no custody lookup,
    /// no seat on the plan.
    #[test]
    fn an_unseated_create_never_consults_custody() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let channel = Uuid::new_v4();
        let projects = projects_with_channel(channel, dir.path());
        let seats = crate::actor_seats::ActorSeatsFile::default();
        let context = ctx_seated(&state, &projects, &seats, 1_000, AUTHORITY);
        match decide_lifecycle(
            &context,
            channel,
            1_000,
            &create_content("create-1", "null", AUTHORITY),
        ) {
            LifecycleDecision::Create(plan) => {
                assert!(plan.actor.is_none());
                assert!(plan.role.is_none());
            }
            other => panic!("expected a create: {other:?}"),
        }
    }

    /// A seated create's signed content, as the desktop writes it.
    fn seated_create_content(command_id: &str, actor: &str, role: &str) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"{command_id}","action":{{"type":"session.create","projectRef":null,"repoRef":null,"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{AUTHORITY}","actor":"{actor}","role":"{role}","model":null,"title":null,"initialTurn":null}}}}"#
        )
    }
}
