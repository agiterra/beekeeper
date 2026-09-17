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

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use buzz_core::ci_result::{correlation_id, CiResultIdentity};
use buzz_core::coding_session_authority_claim::ClaimState;
use buzz_core::coding_session_command::{
    CodingSessionAction, CodingSessionCommandPayload, CodingSessionDelivery, CodingSessionTarget,
    TurnAttachment,
};
use buzz_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
    CodingSessionLifecycleCommandPayload,
};
use buzz_core::coding_session_routing::RoutingRecord;
use buzz_core::coding_session_runtime::RuntimeDescriptor;

use crate::ci_continuation_store::{AdmitRefusal, Admitted, CiContinuationStore};
use crate::payload::{
    ACTOR_UNAVAILABLE, BUDGET_EXHAUSTED, CI_CONTINUATION_EXPIRED, CI_CONTINUATION_STORE_FULL,
    COMMAND_ID_CONFLICT, DUPLICATE_OPERATION, HANDOVER_FENCED, PROJECT_CWD_UNRESOLVED,
    PROVIDER_UNAVAILABLE, SESSION_CLOSED, SESSION_LIMIT, SESSION_RETIRED, STALE_GENERATION,
    UNAUTHORIZED_OPERATOR, UNKNOWN_TARGET,
};
use crate::state::{SessionRecord, StateStore};

/// The refusal code for a registration whose requested expiry lies beyond the
/// window this provider will hold a command for.
///
/// Provider-local, exactly like [`crate::QUEUE_FULL_TURN_KEPT`], and for the
/// same reason: the receipt code list is open (`docs/nips/NIP-CSL.md`), and
/// this is a fact none of the five wire codes states. Calling it
/// `CI_CONTINUATION_EXPIRED` would be false — nothing expired; the sender
/// asked for a horizon the provider does not offer — and a control that
/// misnames why it refused is the class of lie this project treats as a bug.
pub const CI_CONTINUATION_HORIZON: &str = "CI_CONTINUATION_HORIZON";

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
    /// Detach a live session and reattach it at once as a new generation
    /// with a freshly staged seat (spec § 4.9).
    Restart(RestartPlan),
    /// Durably stop a session.
    Stop(StopPlan),
}

/// A validated, resolved create request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePlan {
    /// The verified signer of the 44221 that asked for this execution.
    ///
    /// Distinct from [`Self::founder_pubkey`], and the distinction is the
    /// whole of what the handover fence needs from a create: for a
    /// genesis-bearing create the founder is the *genesis*'s signer, so a
    /// claimant reconstructing somebody else's session has a founder who is
    /// not them. Attributing their own seeded first turn to that founder made
    /// the fence refuse the claimant's own continuation on the body they
    /// claimed.
    pub created_by: String,
    /// The umbrella's claim as this provider verified it from the accepted
    /// chain, ahead of any adapter start.
    ///
    /// `NoClaim` at decision time and replaced by `on_lifecycle` for a
    /// genesis-bearing create. It is what the new record is seeded with, so an
    /// execution minted on a machine that has never seen this umbrella still
    /// carries the claim its siblings elsewhere carry — without it the record
    /// is born `NoClaim` and every later turn on it walks past the fence.
    pub verified_claim: ClaimState,
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
    /// The host's routing decision, echoed onto the seat's kind:44223.
    pub routing: Option<RoutingRecord>,
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
    /// Signed kind:44221 `session.hire` event id this create answers, or
    /// `None` when the create answers no hire.
    ///
    /// Carried so the create path can read the hire and learn **who asked for
    /// this seat**. Nothing about the reference is trusted here: the event is
    /// fetched by id, its own signature is checked by the relay that stores
    /// it, and its `requestedBy` claim is compared with its signer before a
    /// single byte of attribution is used.
    pub hire_ref: Option<String>,
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

/// A validated request to restart one exact current generation in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPlan {
    /// Lifecycle command being answered.
    pub command_id: String,
    /// Channel the execution belongs to.
    pub channel_id: Uuid,
    /// Exact current generation the operator observed.
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
        /// Images the operator attached, addressed by Blossom hash.
        attachments: Vec<TurnAttachment>,
        /// The delivery class the sender asked for. Authority for it has
        /// already been checked here: an `interrupt` that reaches this variant
        /// was signed by the founder.
        deliver: CodingSessionDelivery,
        /// The resolved operation fence key this turn takes custody of, or
        /// `None` when its prompt is not a pointer. Resolved here rather than
        /// re-derived at delivery so the fence that *admitted* the turn and
        /// the fence the turn *claims* can never be computed two ways.
        operation_key: Option<String>,
    },
    /// Cancel the in-flight turn of a live session.
    Interrupt {
        /// The command being answered.
        command_id: String,
        /// The fenced target.
        target: CodingSessionTarget,
    },
    /// Durably register a turn to be started when one exact CI result lands.
    ///
    /// Not a turn: nothing enters a mailbox, no budget is spent, and the
    /// operation ledger is untouched until the eventual turn actually starts.
    RegisterCiContinuation {
        /// The command being answered; the eventual turn runs under it.
        command_id: String,
        /// The exact generation the eventual turn will address.
        target: CodingSessionTarget,
        /// The exact CI run attempt awaited.
        identity: CiResultIdentity,
        /// `correlation_id(identity)` — validated here so a registration that
        /// could never correlate is refused rather than stored.
        correlation_id: String,
        /// Text to deliver alongside the verified result.
        continuation: String,
        /// Unix seconds after which the registration is refused.
        expires_at: u64,
        /// SHA-256 of the registration event content, the idempotence fence.
        payload_digest: String,
    },
}

/// Read-only view of everything a decision depends on.
pub struct CommandContext<'a> {
    /// This provider's signing pubkey, lowercase hex.
    pub provider_pubkey: &'a str,
    /// Pubkey that signed the command currently being decided.
    pub operator_pubkey: &'a str,
    /// Channel the command arrived on.
    ///
    /// Read only by the CI-continuation store's per-channel cap: one noisy
    /// room must not be able to consume every pending slot the provider has
    /// for every other room.
    pub channel_id: Uuid,
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
    /// Per-umbrella turn ceilings read from published kind-44245 session
    /// policies, keyed by `sessionRef`.
    ///
    /// Owned rather than borrowed because it is a small snapshot taken per
    /// command: the map is provider state that a create or resume rewrites,
    /// and a decision must be made against one consistent reading of it.
    /// An umbrella absent from the map has no policy budget and falls back to
    /// [`turn_budget`](Self::turn_budget).
    pub policy_turn_budgets: HashMap<String, u32>,
    /// Number of adapter actors currently attached in this process.
    pub active_session_count: usize,
    /// Durable state: dedupe ledger and session records.
    pub state: &'a StateStore,
    /// Durable CI-continuation registrations.
    ///
    /// Consulted only by `thread.turn.continue_on_ci`, and only for facts the
    /// file already holds: whether this `commandId` is taken, by what bytes,
    /// and whether there is a pending slot left. The decision writes nothing —
    /// [`CiContinuationStore::insert`] happens in the caller, before the
    /// acknowledgement.
    pub ci_continuations: &'a CiContinuationStore,
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
    /// Genesis refs whose authority chain this process has **not** managed to
    /// re-read since it started.
    ///
    /// Populated by `recover` from every open genesis-bearing record and
    /// emptied one genesis at a time as backfills succeed. Empty for a
    /// provider that has never recovered, so nothing that does not restart
    /// pays for this. See [`AUTHORITY_NOT_REVERIFIED`].
    pub claims_pending_reverification: &'a HashSet<String>,
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
        | CodingSessionLifecycleAction::SessionRestart {
            provider_authority_pubkey,
            ..
        }
        | CodingSessionLifecycleAction::SessionStop {
            provider_authority_pubkey,
            ..
        } => provider_authority_pubkey,
        // A hire is addressed to the umbrella's *host*, not to a provider: it
        // names no provider authority at all, and the seat it asks for reaches
        // this provider later as an ordinary seated create.
        CodingSessionLifecycleAction::SessionHire { .. } => {
            return LifecycleDecision::Ignore(Ignored::NotAddressed)
        }
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
    | CodingSessionLifecycleAction::SessionRestart { session, .. }
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
        // A restart reattaches too: it takes the resume's fence and the
        // resume's authority, never the stop's exemption.
        let is_resume = matches!(
            &payload.action,
            CodingSessionLifecycleAction::SessionResume { .. }
                | CodingSessionLifecycleAction::SessionRestart { .. }
        );
        // A chain this process has not re-read yet cannot answer whether the
        // session has been handed over, and "not known" must not read as "not
        // handed over".
        if claim_awaits_reverification(context, record) {
            return LifecycleDecision::Fail {
                command_id: payload.command_id,
                code: AUTHORITY_NOT_REVERIFIED,
                message: awaiting_reverification_message(),
            };
        }
        // The handover fence (§3), before the resume ACL for the same reason
        // it precedes `operator_may_steer`: once a session is handed over,
        // "the founder may always" is no longer the rule, and
        // `UNAUTHORIZED_OPERATOR` would name the wrong reason.
        //
        // A **stop** is exempt in exactly one case, and the distinction is the
        // difference between an operator tidying up and an operator destroying
        // somebody else's work. When the claim names *another* body, this
        // machine is holding a stranded execution of its own and its founder
        // may shut it down; when the claim names **this** provider, the
        // process here is the claimant's live continuation and killing it
        // would undo the handover by force. A voided umbrella has no
        // claimant's work left to protect, so its orphaned process stays
        // stoppable — otherwise a voided claim would strand a process that
        // nobody, founder included, could ever release. Retirement exempts
        // nothing: a deleted session answers `SESSION_RETIRED` to every
        // command, stops included.
        let stop_may_proceed = !is_resume
            && !matches!(
                &record.handover,
                ClaimState::Active(claim) if claim.body_pubkey == context.provider_pubkey
            );
        let fence = handover_fence(record, context.operator_pubkey, context.provider_pubkey);
        if let Some(refusal) = fence
            .filter(|refusal| refusal.code == SESSION_RETIRED || is_resume || !stop_may_proceed)
        {
            return LifecycleDecision::Fail {
                command_id: payload.command_id,
                code: refusal.code,
                message: refusal.message,
            };
        }
        let authorized = if is_resume {
            operator_may_resume(record, context.operator_pubkey)
        } else {
            operator_owns_session(record, context.operator_pubkey)
        };
        if !authorized {
            let message = if is_resume {
                "only the session founder or a granted operator may resume this execution"
            } else {
                "only the session founder may stop this execution"
            };
            return LifecycleDecision::Fail {
                command_id: payload.command_id,
                code: UNAUTHORIZED_OPERATOR,
                message: message.into(),
            };
        }

        return match &payload.action {
            CodingSessionLifecycleAction::SessionResume { .. }
            | CodingSessionLifecycleAction::SessionRestart { .. }
                if record.closed =>
            {
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
            CodingSessionLifecycleAction::SessionRestart { .. } => {
                LifecycleDecision::Restart(RestartPlan {
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
            // Guarded by the `if let` above: only a resume or a stop reaches
            // here.
            _ => unreachable!(),
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
        hire_ref,
        routing,
    } = &payload.action
    else {
        unreachable!()
    };

    // The fence over a **create**, and the reason it cannot be left to the
    // turn path: a create mints a record, spawns an adapter and dispatches its
    // `initialTurn` directly, none of which passes through
    // [`decide_turn_command`]. Without this, the machine that lost the session
    // simply starts a second execution under the same umbrella and the fence
    // never sees it.
    //
    // Asked of the umbrella rather than of one record, because the record this
    // create would mint does not exist yet. The three answers that matter:
    //
    // * this provider is the claimed body and the sender is the claimant —
    //   which is exactly B's own reconstruct or native continuation — admitted;
    // * this provider is not the claimed body, or the sender is not the
    //   claimant — A coming back and starting again — `HANDOVER_FENCED`;
    // * the claim was voided — nobody starts anything under this umbrella
    //   until a fresh accepted takeover — `HANDOVER_FENCED`.
    //
    // A provider holding no record of this genesis (B's machine, seeing the
    // umbrella for the first time) fences nothing: it has folded no claim, and
    // inventing one from an unverified read would refuse the very continuation
    // this whole feature exists to allow.
    // Deliberately **not** gated on `claims_pending_reverification` here. That
    // flag says "some execution of this umbrella has not folded the chain",
    // which is the right answer for a turn on one of those executions and the
    // wrong one for a create: `on_lifecycle` reads and verifies the chain for
    // this create specifically, which is stronger evidence than a stale flag,
    // and refusing on the flag alone would leave a provider unable to ever
    // join an umbrella it once failed to read. The siblings stay fenced by the
    // flag; this create is judged on what was actually read.
    if let Some(genesis_ref) = genesis_ref.as_deref() {
        if let Some(refusal) = umbrella_fence(
            context.state,
            channel_id,
            genesis_ref,
            context.operator_pubkey,
            context.provider_pubkey,
        ) {
            return LifecycleDecision::Fail {
                command_id: payload.command_id,
                code: refusal.code,
                message: refusal.message,
            };
        }
    }

    // The command is addressed to *this* signer, so no other process will ever
    // answer it. A ref naming no descriptor therefore fails loudly — silence
    // would strand the consumer's durable create forever.
    if !context
        .runtimes
        .iter()
        // Both sides name a provider instance **alias**: the descriptor's
        // `instance_ref` is the alias this provider advertises in its catalog,
        // and the create's `providerInstanceRef` is the alias the operator
        // asked for. Comparing either with a `cs-target.instanceId` is ledger
        // item 102 and is a compile error since B2.
        .any(|descriptor| descriptor.instance_ref.as_str() == provider_instance_ref.as_str())
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
                // `.as_str()`, not the newtype: `{:?}` on a
                // `ProviderInstanceAlias` renders `ProviderInstanceAlias("…")`
                // into a message an operator reads, which is a Rust type name
                // leaking onto the wire.
                "unknown providerInstanceRef {:?}; this provider offers: {}",
                provider_instance_ref.as_str(),
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

    if let Some(message) =
        consumed_hint_without_record(context.projects, context.state, &payload.command_id)
    {
        return LifecycleDecision::Fail {
            command_id: payload.command_id,
            code: PROJECT_CWD_UNRESOLVED,
            message,
        };
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
        created_by: context.operator_pubkey.to_owned(),
        // Nothing is known about the umbrella's claim at decision time: this
        // decision runs before any relay read. `on_lifecycle` resolves the
        // genesis and then the chain, and replaces this.
        verified_claim: ClaimState::NoClaim,
        command_id: payload.command_id.clone(),
        runtime_instance_ref: provider_instance_ref.as_str().to_owned(),
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
        routing: routing.clone(),
        actor: actor.clone(),
        role: role.clone(),
        hire_ref: hire_ref.clone(),
    }))
}

/// What a consumer could verify about who asked for a hired seat.
///
/// Three answers, never two. A `session.hire` may carry `requestedBy` — the
/// pubkey of the seat that ran `bee sessions hire` — and **the relay does not
/// compare it with the event's signer** (`docs/design/portable-team-loop/
/// POLICY.md` §5). It could; v1 does not. So any signer the relay admits for a
/// hire can write another seat's pubkey there, and a consumer that printed the
/// claim as fact would be attributing a brief to somebody who never asked for
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HireAttribution {
    /// The hire claimed no requester. *Unknown*, never *mismatched*: a hire
    /// signed before the key existed is not a hire whose requester disagrees
    /// with its signer.
    Unclaimed,
    /// `requestedBy` equals the pubkey that signed the hire. This is the only
    /// answer a consumer may render as attribution.
    Attributed(String),
    /// `requestedBy` names a pubkey that did not sign the hire. Disclosed, and
    /// never used: the caller falls back to the unattributed delivery a hire
    /// without the key would have got.
    Disputed {
        /// The pubkey the hire claimed asked for the seat.
        claimed: String,
        /// The pubkey that actually signed the hire.
        signer: String,
    },
}

/// Compare a decoded `session.hire`'s requester claim with its own signer.
///
/// `signer_pubkey_hex` must be the pubkey of the **signed event** the payload
/// was decoded from — a locally verified fact, never a value out of the
/// content. Any action that is not a hire answers [`HireAttribution::Unclaimed`],
/// because it claims nothing.
pub fn hire_attribution(
    hire: &CodingSessionLifecycleCommandPayload,
    signer_pubkey_hex: &str,
) -> HireAttribution {
    match hire.hire_requested_by() {
        None => HireAttribution::Unclaimed,
        Some(claimed) if claimed == signer_pubkey_hex => {
            HireAttribution::Attributed(claimed.to_owned())
        }
        Some(claimed) => HireAttribution::Disputed {
            claimed: claimed.to_owned(),
            signer: signer_pubkey_hex.to_owned(),
        },
    }
}

/// Decide what a 44220 turn command means for this provider.
pub fn decide_turn(context: &CommandContext<'_>, created_at: u64, content: &str) -> TurnDecision {
    let command = match decode_turn_command(content) {
        Ok(command) => command,
        Err(error) => return TurnDecision::Ignore(Ignored::Malformed(error)),
    };
    decide_turn_command(context, created_at, command)
}

/// Decide what an already-decoded 44220 means for this provider.
///
/// The same decision [`decide_turn`] makes, reachable without a signed event.
/// The CI-continuation delivery path uses it to run the turn it rebuilds from
/// a durable registration through the **identical** check order — authority,
/// generation, closure, budget, and the operation fence are re-evaluated at
/// delivery, not at registration, so a grant revoked while CI was running
/// refuses the turn instead of starting it.
pub fn decide_turn_command(
    context: &CommandContext<'_>,
    created_at: u64,
    command: TurnCommand,
) -> TurnDecision {
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
    // The durable CI promise owns this id across action shapes too. An
    // ordinary start must never substitute its text for a waiting promise.
    // Provider-materialized delivery carries the original registration digest.
    if context
        .ci_continuations
        .record(&command.command_id)
        .is_some_and(|record| record.payload_digest != command.payload_digest)
    {
        return TurnDecision::Fail {
            command_id: command.command_id,
            target: command.target,
            code: COMMAND_ID_CONFLICT,
            message: "a different CI continuation is already durably registered under this commandId; the first durable record wins".into(),
        };
    }
    if context.in_flight.contains_key(&command.command_id) {
        return TurnDecision::Ignore(Ignored::AlreadyAccepted);
    }
    // A native steer attempt that is still open owns this command as surely
    // as a mailbox does: its answer is owed by the attempt's resolution, and
    // re-admitting the command here would be a second delivery of an input
    // that may already be in the runtime. A resolved attempt has already put
    // the command in the consumed or refused ledger above.
    if context
        .state
        .steer_attempt_for_command(&command.command_id)
        .is_some_and(|attempt| attempt.disposition.is_open())
    {
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
    // A chain this process has not re-read since it started cannot say whether
    // this session has been handed over. Refusing by name beats guessing that
    // nothing changed while the provider was down.
    if claim_awaits_reverification(context, record) {
        return TurnDecision::Fail {
            command_id: command.command_id,
            target: command.target,
            code: AUTHORITY_NOT_REVERIFIED,
            message: awaiting_reverification_message(),
        };
    }
    // The handover fence, before the ordinary steering ACL and deliberately
    // so: once a session has been handed over, "the founder may always steer"
    // is no longer the rule, and answering `UNAUTHORIZED_OPERATOR` to the
    // founder of a session somebody else now holds would name the wrong
    // reason. §3 of `docs/HANDOVER_IMPL.md`. The refusal is recorded in the
    // durable refusal ledger by `on_turn`'s `Fail` arm before it is published,
    // like every other turn refusal.
    //
    // A plain `thread.turn.interrupt` is outside the fence, on exactly the
    // reasoning the budget gate below already uses: cancelling starts no work,
    // spends no turn, and produces no second execution of anything. A fence
    // that stopped a founder cancelling a runaway turn while still letting
    // them stop the whole process would be strictly worse than useless. A
    // **retired** session still refuses even an interrupt: there is no turn
    // under a deleted umbrella to cancel.
    if let Some(refusal) = handover_fence(record, context.operator_pubkey, context.provider_pubkey)
        .filter(|refusal| {
            refusal.code == SESSION_RETIRED || !matches!(command.action, TurnAction::Interrupt)
        })
    {
        return TurnDecision::Fail {
            command_id: command.command_id,
            target: command.target,
            code: refusal.code,
            message: refusal.message,
        };
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
        if let Some((used, limit, source)) = exhausted_turn_budget(context, record) {
            return TurnDecision::Fail {
                command_id: command.command_id,
                target: command.target,
                code: BUDGET_EXHAUSTED,
                message: if reserved_ci_turns(context, record) > 0 {
                    format!("this team session has used or committed {used} of its {limit} allowed turns, including CI starts awaiting their started report; the session founder can still send turns")
                } else {
                    budget_exhausted_message(used, limit, source)
                },
            };
        }
    }

    // The operation fence. Everything above answers "have I already acted on
    // *this command*"; this answers "is this *operation* already somebody's".
    // Two producers mint different command ids for one identifier-only wake
    // pointer on purpose, so without this a producer bug on either side buys
    // a second turn of the lead's context on a fact it already has. Checked
    // here, at the only place that spends model context, and answered with a
    // refusal that spends no turn.
    let operation_key = match &command.action {
        // The pointer the fence reads: the provider-supplied one when the
        // provider minted this turn and knows it independently of the text
        // (a CI continuation materializes its whole result into the prompt),
        // otherwise the prompt itself, which is what every existing producer
        // means by a pointer.
        TurnAction::Start { text, .. } => crate::team_wake::operation_fence_key(
            &command.target,
            command.operation_key.as_deref().unwrap_or(text),
        ),
        _ => None,
    };
    if let Some(key) = &operation_key {
        if let Some(owner) = duplicate_operation_owner(context, key, &command.command_id) {
            // This message names provider *state* (the owner), not just the
            // command — the one receipt in this module that does. It is
            // stable only because `on_turn`'s `Fail` arm records the refusal
            // durably *before* publishing, so a redelivery is answered
            // `AlreadyRefused` and can never be republished naming a
            // different owner. Moving `record_refusal` after the publish
            // would silently break that.
            return TurnDecision::Fail {
                command_id: command.command_id,
                target: command.target,
                code: DUPLICATE_OPERATION,
                message: format!(
                    "an identical team-wake pointer for this target is already custodied by \
                     command {owner}; this command spent no turn"
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
        TurnAction::Start {
            text,
            attachments,
            deliver,
        } => TurnDecision::Start {
            command_id: command.command_id,
            target: command.target,
            text,
            attachments,
            deliver,
            operation_key,
        },
        TurnAction::Interrupt => TurnDecision::Interrupt {
            command_id: command.command_id,
            target: command.target,
        },
        // A registration, not a turn. Every check above has already run in
        // its usual order — including `operator_may_steer`, because a
        // registration promises a turn and only somebody who may steer now
        // may promise one. What is deliberately absent is the budget spend
        // (nothing has run) and the operation ledger (nothing has started).
        TurnAction::ContinueOnCi {
            identity,
            continuation,
            expires_at,
        } => decide_ci_continuation(
            context,
            created_at,
            command.command_id,
            command.target,
            command.payload_digest,
            identity,
            continuation,
            expires_at,
        ),
    }
}

/// The CI-continuation-specific half of [`decide_turn_command`].
///
/// Reached only once the shared checks have admitted the command. Refuses,
/// in order: an identity that cannot be correlated at all, an expiry already
/// past, an expiry beyond this provider's command horizon, a `commandId`
/// already holding different bytes, and a full store.
#[allow(clippy::too_many_arguments)]
fn decide_ci_continuation(
    context: &CommandContext<'_>,
    created_at: u64,
    command_id: String,
    target: CodingSessionTarget,
    payload_digest: String,
    identity: CiResultIdentity,
    continuation: String,
    expires_at: u64,
) -> TurnDecision {
    if context
        .state
        .session(&target.session_id)
        .and_then(|record| record.project_ref.as_deref())
        != Some(identity.project.as_str())
    {
        return TurnDecision::Fail {
            command_id,
            target,
            code: crate::ci_continuation::CI_CONTINUATION_PROJECT_MISMATCH,
            message: "the CI result project must match this execution's recorded project; an execution without a project cannot register a CI continuation".into(),
        };
    }
    let correlation_id = match correlation_id(&identity) {
        Ok(digest) => digest,
        // The relay validates the same identity before storing the command,
        // so this is a version skew rather than an operator error — but it
        // still has to be visible, because the sender is waiting on a receipt.
        Err(error) => {
            return TurnDecision::Fail {
                command_id,
                target,
                code: UNKNOWN_TARGET,
                message: format!("the CI identity cannot be correlated: {error}"),
            }
        }
    };
    if expires_at <= context.now_secs {
        return TurnDecision::Fail {
            command_id,
            target,
            code: CI_CONTINUATION_EXPIRED,
            message: "the registration's expiresAt is already in the past, so no result could \
                      ever be delivered under it"
                .into(),
        };
    }
    let horizon = created_at.saturating_add(context.horizon_secs);
    if expires_at > horizon {
        return TurnDecision::Fail {
            command_id,
            target,
            code: CI_CONTINUATION_HORIZON,
            message: format!(
                "this provider holds a registration for at most {} seconds past the command's \
                 own created_at; register again with a shorter expiresAt",
                context.horizon_secs
            ),
        };
    }
    match context
        .ci_continuations
        .admission(&command_id, &payload_digest, context.channel_id)
    {
        // An exact retry — same id, same bytes — is admitted rather than
        // ignored, and the register path is idempotent: it writes no second
        // record and re-enqueues the *same* `continuation_registered` receipt
        // under the same semantic key. That is what makes §2's recovery real.
        // A caller whose acknowledgement was lost re-runs the identical
        // command and is answered, instead of being met with silence and
        // having to mint a second registration to get a receipt at all.
        Ok(Some(Admitted::AlreadyStored) | Some(Admitted::Stored)) | Ok(None) => {
            TurnDecision::RegisterCiContinuation {
                command_id,
                target,
                identity,
                correlation_id,
                continuation,
                expires_at,
                payload_digest,
            }
        }
        Err(AdmitRefusal::CommandIdConflict) => TurnDecision::Fail {
            command_id,
            target,
            code: COMMAND_ID_CONFLICT,
            message: "a different CI continuation is already durably registered under this \
                      commandId; the first durable record wins"
                .into(),
        },
        Err(AdmitRefusal::StoreFull { detail }) => TurnDecision::Fail {
            command_id,
            target,
            code: CI_CONTINUATION_STORE_FULL,
            message: format!(
                "{detail}, so this registration was refused rather than displacing one somebody \
                 is already waiting on"
            ),
        },
    }
}

/// The command already custodying this turn's team-wake operation, when it is
/// not `command_id` itself.
///
/// Two fences, in the order that answers the most cases: the durable ledger
/// first (an operation whose owner already *started*), then this process's
/// mailbox (an owner accepted and not yet started, which nothing durable can
/// know about yet).
///
/// The owner is always admitted. A command that is already the recorded owner
/// is a relay redelivery of the turn that claimed the operation — the id-based
/// fences above answer it as `AlreadyConsumed`/`AlreadyAccepted` — and after a
/// crash between the operation write and the command write it is the *only*
/// command that may run. Refusing it there would lose the wake permanently.
fn duplicate_operation_owner(
    context: &CommandContext<'_>,
    key: &str,
    command_id: &str,
) -> Option<String> {
    if let Some(owner) = context.state.operation_owner(key) {
        return (owner != command_id).then(|| owner.to_owned());
    }
    context.in_flight.iter().find_map(|(candidate, turn)| {
        (candidate != command_id && turn.operation_key.as_deref() == Some(key))
            .then(|| candidate.clone())
    })
}

/// Owner-only authority: stop/end. Checks only authority facts
/// persisted when this provider witnessed the create. Old no-genesis records
/// predate that field and remain ungoverned; genesis-bearing records can
/// never fall open when their founder is absent. `grant-operator` never moves
/// ownership, so the granted-operator set is deliberately not consulted here.
/// A named refusal the handover fence produces, ready to publish.
///
/// Carries the code and the sentence together because the two are one answer:
/// every message here names the claimant, the body, and — when it applies —
/// that the claim was voided, so an operator reading the receipt learns *who*
/// holds the session rather than only that they do not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FenceRefusal {
    /// [`SESSION_RETIRED`] or [`HANDOVER_FENCED`].
    pub code: &'static str,
    /// The operator-facing sentence.
    pub message: String,
}

/// The refusal a genesis-bearing record answers with until its authority
/// chain has been re-read since this process started.
///
/// Provider-local, like [`crate::native_restore::NO_RESUME_CURSOR`]: it
/// describes a fact about *this* process's knowledge, not a rule the wire has
/// an opinion about.
///
/// It exists because the two halves of the chain fail in opposite directions.
/// A grant that cannot be verified is simply never applied, so an unreadable
/// chain leaves an execution founder-only — closed, and safe. A **claim** that
/// cannot be verified leaves the persisted `handover` at whatever it was
/// before the restart, which for the machine that has just come back from an
/// outage is `NoClaim` — open, and exactly the divergence the fence exists to
/// stop. So a record whose chain this process has not managed to re-read
/// refuses rather than guessing that nothing has changed.
pub const AUTHORITY_NOT_REVERIFIED: &str = "AUTHORITY_NOT_REVERIFIED";

/// The strictest fence any local record of one umbrella imposes.
///
/// The claim is umbrella-wide, but the *records* of an umbrella can disagree
/// for one process-lifetime reason: a record created a moment ago starts at
/// `authority_seq` 0 with no claim folded into it yet, while its siblings
/// carry the claim. Reading only one of them — the first, or the newest —
/// would let a fresh sibling admit work the umbrella is fenced against, so
/// this reads **all** of them and returns the first refusal it finds.
///
/// `None` means no local record of this genesis is fenced, which for an
/// umbrella this provider holds no records of at all is the honest answer: it
/// has folded no claim, so it fences nothing, and the ordinary rules apply.
pub fn umbrella_fence(
    state: &StateStore,
    channel_id: Uuid,
    genesis_ref: &str,
    operator_pubkey: &str,
    provider_pubkey: &str,
) -> Option<FenceRefusal> {
    state
        .sessions()
        .filter(|record| {
            record.channel_id == channel_id && record.genesis_ref.as_deref() == Some(genesis_ref)
        })
        .find_map(|record| handover_fence(record, operator_pubkey, provider_pubkey))
}

/// The claim an umbrella's local records agree on, folded furthest along the
/// chain.
///
/// Used to **seed** a record being created under an umbrella this provider
/// already knows about. Without it a `session.create` mints a record at
/// `NoClaim` and every later turn on it walks straight past the fence, which
/// is the same divergence by a different door.
///
/// "Furthest along" is `authority_seq`: every record of a genesis is extended
/// together by [`crate::Provider::resolve_and_apply_grant`], so they normally
/// agree, and where they do not it is because one of them is newer and has
/// applied fewer links.
pub fn umbrella_claim(state: &StateStore, channel_id: Uuid, genesis_ref: &str) -> ClaimState {
    state
        .sessions()
        .filter(|record| {
            record.channel_id == channel_id && record.genesis_ref.as_deref() == Some(genesis_ref)
        })
        .max_by_key(|record| record.authority_seq)
        .map(|record| record.handover.clone())
        .unwrap_or_default()
}

/// Whether this record's chain still has to be re-read before it may act.
///
/// Only ever true for a genesis-bearing record, and only between a `recover`
/// that could not reach the relay and the first backfill that succeeds for
/// that genesis. A provider that has never recovered — every unit test that
/// builds one directly — has an empty pending set and is unaffected.
fn claim_awaits_reverification(context: &CommandContext<'_>, record: &SessionRecord) -> bool {
    record
        .genesis_ref
        .as_deref()
        .is_some_and(|genesis_ref| context.claims_pending_reverification.contains(genesis_ref))
}

/// The sentence an unverified chain refuses with.
fn awaiting_reverification_message() -> String {
    "this execution's authority chain has not been re-verified since this provider restarted, \
     so whether the session has been handed over is not yet known; it will be admitted or \
     refused by name once the chain can be read"
        .to_owned()
}

/// Whether the handover fence refuses this command, and why.
///
/// The one implementation of `docs/HANDOVER_IMPL.md` §3's rule, called from
/// every path that would let an execution act: turn admission
/// ([`decide_turn_command`], which the CI continuation's
/// `admit_ci_turn_start` also runs through), `session.resume`
/// ([`decide_lifecycle`]), native restore ([`crate::native_restore`]) and
/// team-wake admission ([`crate::team_wake`]).
///
/// The claim is **umbrella-wide**: every execution rooted at the handed-over
/// genesis carries the same [`crate::state::SessionRecord::handover`], so a
/// sibling execution of the claimed one is fenced alongside it. That is the
/// v1 scope decision, not an accident of this function.
///
/// `operator_pubkey == provider_pubkey` is the provider acting as itself — a
/// team wake it minted. On the **claimed body** that proceeds: the claimant
/// chose this machine to carry the work, and its own bookkeeping turns are
/// how the work continues. On any other body it is refused like everything
/// else, because a provider minting a wake for a session it no longer holds
/// is exactly the divergence the fence exists to stop.
pub fn handover_fence(
    record: &SessionRecord,
    operator_pubkey: &str,
    provider_pubkey: &str,
) -> Option<FenceRefusal> {
    if let Some(retired) = &record.retired {
        return Some(FenceRefusal {
            code: SESSION_RETIRED,
            message: format!(
                "this session was deleted by accepted deletion {}; nothing under it runs again, \
                 and no part of it will be republished or reconstructed",
                retired.deletion_event_id
            ),
        });
    }
    claim_fence(&record.handover, operator_pubkey, provider_pubkey)
}

/// The handover half of [`handover_fence`], over a bare claim.
///
/// Split out because the **create** path has to ask the same question before
/// any record exists: a provider seeing an umbrella for the first time reads
/// the accepted chain, folds it, and must apply exactly this rule to the claim
/// it just learned — not a weaker one, and not a second copy of it.
pub fn claim_fence(
    claim: &ClaimState,
    operator_pubkey: &str,
    provider_pubkey: &str,
) -> Option<FenceRefusal> {
    match claim {
        ClaimState::NoClaim => None,
        ClaimState::Voided {
            last, voided_by, ..
        } => Some(FenceRefusal {
            code: HANDOVER_FENCED,
            message: format!(
                "this session was handed over to {} on execution body {}, and that claim was \
                 voided by {}; nobody may steer it until a fresh accepted takeover or transfer \
                 is published",
                last.claimant, last.body_pubkey, voided_by
            ),
        }),
        ClaimState::Active(claim) => {
            if claim.body_pubkey != provider_pubkey {
                return Some(FenceRefusal {
                    code: HANDOVER_FENCED,
                    message: format!(
                        "this session is held by {} on execution body {}; this provider ({}) is \
                         not that body, so nothing here acts on it",
                        claim.claimant, claim.body_pubkey, provider_pubkey
                    ),
                });
            }
            if operator_pubkey != claim.claimant && operator_pubkey != provider_pubkey {
                return Some(FenceRefusal {
                    code: HANDOVER_FENCED,
                    message: format!(
                        "this session is held by {} on execution body {}; only that claimant may \
                         steer it until the claim is transferred or a fresh takeover is accepted",
                        claim.claimant, claim.body_pubkey
                    ),
                });
            }
            None
        }
    }
}

fn operator_owns_session(record: &crate::state::SessionRecord, operator_pubkey: &str) -> bool {
    match record.founder_pubkey.as_deref() {
        Some(founder) => founder == operator_pubkey,
        None => record.genesis_ref.is_none(),
    }
}

/// Resume authority: the founder or a currently granted operator may
/// reattach an execution. The grant cache is consulted only for a
/// genesis-bearing record; each entry was derived from the verified,
/// contiguous acceptance-receipt chain for that exact genesis and channel.
/// Viewer grants, revoked operators, project membership, and agent ownership
/// never enter this decision.
fn operator_may_resume(record: &crate::state::SessionRecord, operator_pubkey: &str) -> bool {
    operator_owns_session(record, operator_pubkey)
        || (record.genesis_ref.is_some()
            && record.founder_pubkey.is_some()
            && record.granted_operators.contains(operator_pubkey))
}

/// Steering authority: turn start/interrupt. The owner always may; beyond
/// that, only a genesis-bearing session consults its verified
/// granted-operator cache (each entry applied from a relay-signed acceptance
/// receipt plus the resolved accepted transition — see [`crate::authority`]).
/// Legacy no-genesis sessions never gain operators this way (R20): umbrella
/// authority for them arrives by adoption, not provider inference.
pub(crate) fn operator_may_steer(
    record: &crate::state::SessionRecord,
    operator_pubkey: &str,
) -> bool {
    if operator_owns_session(record, operator_pubkey) {
        return true;
    }
    record.genesis_ref.is_some() && record.granted_operators.contains(operator_pubkey)
}

/// The umbrella allowance this turn would exceed, as `(used, limit, source)`,
/// or `None` when the turn is within budget or outside the budget's reach.
///
/// A thin wrapper over [`exhausted_umbrella_budget`] so the decision path and
/// the create path answer the same question from the same facts.
fn exhausted_turn_budget(
    context: &CommandContext<'_>,
    record: &crate::state::SessionRecord,
) -> Option<(u64, u64, TurnBudgetSource)> {
    exhausted_umbrella_budget_with_reservations(
        context.state,
        context.turn_budget,
        record
            .session_ref
            .as_deref()
            .and_then(|session_ref| context.policy_turn_budgets.get(session_ref).copied()),
        record.session_ref.as_deref(),
        context.operator_pubkey,
        reserved_ci_turns(context, record),
    )
}

// A CI actor has durable start permission before it emits TurnStarted. Count
// that brief reservation until the report records actual spend and removes the
// in-flight entry. Ordinary queued commands have not been consumed yet.
fn reserved_ci_turns(context: &CommandContext<'_>, record: &crate::state::SessionRecord) -> u64 {
    let Some(session_ref) = record.session_ref.as_deref() else {
        return 0;
    };
    context
        .in_flight
        .iter()
        .filter(|(command_id, turn)| {
            context.state.is_command_consumed(command_id)
                && context
                    .state
                    .session(&turn.session_id)
                    .and_then(|session| session.session_ref.as_deref())
                    == Some(session_ref)
        })
        .count() as u64
}

/// Which ceiling refused a turn.
///
/// Two ceilings can bind the same turn and a reader has to be able to tell
/// them apart, because they are changed in completely different places: one is
/// an environment variable on the machine running the provider, the other is a
/// signed record anyone reading the session can see. A refusal that named
/// neither sent every reader to the wrong knob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnBudgetSource {
    /// `BUZZ_CSP_TURN_BUDGET` on the host running this provider.
    Environment,
    /// `budget.turns` in the umbrella's newest accepted kind-44245 policy.
    Policy,
}

/// The operator-facing sentence for an exhausted turn allowance.
///
/// Public so the create path — whose first turn never passes through
/// [`decide_turn`] — answers with the same words rather than a paraphrase that
/// drifts.
pub fn budget_exhausted_message(used: u64, limit: u64, source: TurnBudgetSource) -> String {
    match source {
        TurnBudgetSource::Environment => format!(
            "this team session has started {used} of its {limit} allowed turns; the session \
             founder can still send turns, and raising \"Turns per team session\" takes effect \
             the next time the provider starts"
        ),
        TurnBudgetSource::Policy => format!(
            "this team session has started {used} of the {limit} turns its published session \
             policy allows; the session founder can still send turns, and changing this ceiling \
             means publishing a new session policy for this session, not restarting the provider"
        ),
    }
}

/// The umbrella allowance a turn by `operator_pubkey` would exceed, as
/// `(used, limit)`, or `None` when it is within budget or outside the
/// budget's reach.
///
/// The ceiling is `policy_turns` when the umbrella published one, and
/// `env_limit` otherwise — see [`TurnBudgetSource`], which the caller must
/// name in its refusal so a reader knows which one to change.
///
/// Four ways a turn is outside its reach, and each is a fact rather than a
/// tolerance: no ceiling applies (`UNLIMITED_TURN_BUDGET`); the execution
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
    env_limit: u64,
    policy_turns: Option<u32>,
    session_ref: Option<&str>,
    operator_pubkey: &str,
) -> Option<(u64, u64, TurnBudgetSource)> {
    exhausted_umbrella_budget_with_reservations(
        state,
        env_limit,
        policy_turns,
        session_ref,
        operator_pubkey,
        0,
    )
}

fn exhausted_umbrella_budget_with_reservations(
    state: &StateStore,
    env_limit: u64,
    policy_turns: Option<u32>,
    session_ref: Option<&str>,
    operator_pubkey: &str,
    reserved: u64,
) -> Option<(u64, u64, TurnBudgetSource)> {
    // The one thing a published session policy enforces (POLICY.md §4). It
    // *overrides* rather than tightens: a host ceiling and a session ceiling
    // answer different questions — how much this machine will spend on
    // anything, and how much this mission was authorized to spend — and the
    // session's own signed answer is the more specific one. It therefore also
    // binds where the host set no ceiling at all, which is the common case.
    let (limit, source) = match policy_turns {
        Some(turns) => (u64::from(turns), TurnBudgetSource::Policy),
        None => (env_limit, TurnBudgetSource::Environment),
    };
    if limit == crate::config::UNLIMITED_TURN_BUDGET {
        return None;
    }
    let session_ref = session_ref?;
    if state.umbrella_founder(session_ref).as_deref() == Some(operator_pubkey) {
        return None;
    }
    let used = state.turns_used(session_ref).saturating_add(reserved);
    (used >= limit).then_some((used, limit, source))
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
/// untouched either way — stop and end never move to a seat. Resume separately
/// accepts any currently granted operator.
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
    /// The operation pointer this turn takes custody of, when the provider
    /// itself minted the turn and knows the pointer independently of the text.
    ///
    /// `None` for every command decoded off the wire, which is what keeps
    /// existing turns byte-for-byte unchanged: their pointer, if they have
    /// one, *is* their text. A CI continuation is the one case where the two
    /// differ — the delivered text materializes the whole verified result, but
    /// the fence has to stay on the compact
    /// [`buzz_core::coding_session_command::ci_continuation_pointer`] so that
    /// duplicate results and alternate command ids converge on one operation.
    pub operation_key: Option<String>,
    /// SHA-256 (lowercase hex) of the raw event content this command decoded
    /// from, or the empty string for a command the provider minted itself.
    ///
    /// Only a CI continuation reads it: the same `commandId` carrying the same
    /// bytes is the same registration, and carrying different bytes is a
    /// `COMMAND_ID_CONFLICT`.
    pub payload_digest: String,
}

/// The two turn actions the donor contract defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnAction {
    /// Start a turn with operator-entered text.
    Start {
        /// The prompt.
        text: String,
        /// Images the operator attached, addressed by Blossom hash.
        attachments: Vec<TurnAttachment>,
        /// How the sender asked for it to be delivered.
        deliver: CodingSessionDelivery,
    },
    /// Cancel the in-flight turn.
    Interrupt,
    /// Register a turn for when one exact CI result is recorded.
    ContinueOnCi {
        /// The exact CI run attempt awaited.
        identity: CiResultIdentity,
        /// Text to deliver alongside the verified result.
        continuation: String,
        /// Unix seconds after which the registration is refused.
        expires_at: u64,
    },
}

/// Strictly decode a 44220 payload of either action.
pub fn decode_turn_command(content: &str) -> Result<TurnCommand, String> {
    let payload: CodingSessionCommandPayload = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session command payload: {error}"))?;
    payload.validate()?;
    let action = match payload.action {
        CodingSessionAction::ThreadTurnStart {
            text,
            attachments,
            deliver,
        } => TurnAction::Start {
            text,
            attachments,
            deliver,
        },
        CodingSessionAction::ThreadTurnInterrupt => TurnAction::Interrupt,
        CodingSessionAction::ThreadTurnContinueOnCi {
            identity,
            continuation,
            expires_at,
        } => TurnAction::ContinueOnCi {
            identity,
            continuation,
            expires_at,
        },
    };
    Ok(TurnCommand {
        command_id: payload.command_id,
        target: payload.target,
        action,
        operation_key: None,
        payload_digest: hex::encode(<Sha256 as Digest>::digest(content.as_bytes())),
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
    /// Directory holding one-shot per-command hint files, beside the projects
    /// file itself. Never part of the file's own JSON.
    #[serde(skip)]
    pub hints_dir: Option<PathBuf>,
}

/// Name of the directory holding one-shot create hints, beside the projects
/// file.
///
/// It exists because `pending[commandId]` is not a durable place to leave one.
/// The desktop rematerializes the whole projects file from its canonical store
/// whenever anything unrelated is saved, which erases a `pending` entry the CLI
/// wrote moments earlier and before the create is admitted — a race whose only
/// symptom is a session that ran in the wrong directory. A separate directory
/// the rematerialization never touches removes the race rather than narrowing
/// it.
pub const PENDING_HINTS_DIR: &str = "pending-hints";

/// One `pending-hints/<commandId>.json` file.
///
/// `writtenAt` is not consulted for resolution — a hint is either the one this
/// command names or it is nothing — but it is what lets the provider prune
/// files whose create never arrived, so an abandoned hint cannot sit on disk
/// naming a directory forever.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingHint {
    /// The create's `commandId`. Must match the file name's stem, or the file
    /// is ignored: a hint that disagrees with its own name is not evidence
    /// about either command.
    pub command_id: String,
    /// Absolute path of the directory the session should run in.
    pub path: PathBuf,
    /// When the CLI wrote it, Unix seconds.
    #[serde(default)]
    pub written_at: u64,
}

/// The marker a consumed hint leaves behind.
///
/// A hint is **not deleted** when it is spent, and the difference matters.
/// Deleting it makes "this command's directory was already decided" look
/// exactly like "this command never had a hint", so a create that arrives late
/// — a replay, or the same durable create after a lost state file — would
/// resolve to the project or channel default and run the work in a different
/// folder than the one it ran in the first time. Silently. The marker records
/// what was decided so that case is answered by name instead.
///
/// `sessionId` is optional so the shape the rename leaves behind mid-crash (a
/// `.consumed` file still holding the [`PendingHint`] body) still reads.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumedHint {
    /// The create's `commandId`.
    pub command_id: String,
    /// The directory that was resolved for it.
    pub path: PathBuf,
    /// The execution the create minted, when it minted one.
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Path of the live hint file for one command.
pub fn pending_hint_path(hints_dir: &Path, command_id: &str) -> PathBuf {
    hints_dir.join(format!("{command_id}.json"))
}

/// Path of the marker left when that hint is consumed.
pub fn consumed_hint_path(hints_dir: &Path, command_id: &str) -> PathBuf {
    hints_dir.join(format!("{command_id}.consumed"))
}

/// The marker for `command_id`, if this command's hint has been spent.
///
/// A marker that cannot be read at all is treated as present-but-opaque by the
/// caller rather than as absent: the whole point is that "already decided"
/// never degrades into "never had one".
pub fn read_consumed_hint(hints_dir: &Path, command_id: &str) -> Option<ConsumedHint> {
    let path = consumed_hint_path(hints_dir, command_id);
    let body = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<ConsumedHint>(&body) {
        Ok(hint) => Some(hint),
        Err(error) => {
            tracing::warn!(
                target: "csp::projects",
                "consumed-hint marker {} is unreadable: {error}",
                path.display()
            );
            Some(ConsumedHint {
                command_id: command_id.to_owned(),
                path: PathBuf::new(),
                session_id: None,
            })
        }
    }
}

/// Whether this command's hint has already been spent.
pub fn hint_is_consumed(hints_dir: &Path, command_id: &str) -> bool {
    consumed_hint_path(hints_dir, command_id).exists()
}

/// Read the one-shot hint for `command_id`, if there is a usable one.
///
/// Every failure is "no hint": a missing file, unreadable bytes, malformed
/// JSON, a `commandId` that disagrees with the file name, or a path that is
/// not an existing absolute directory. Each is warned about and none is fatal,
/// because the alternative to falling through to the project and channel
/// entries is a create refused for a reason the operator did not cause.
pub fn read_pending_hint(hints_dir: &Path, command_id: &str) -> Option<PathBuf> {
    let path = pending_hint_path(hints_dir, command_id);
    let body = match std::fs::read_to_string(&path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(
                target: "csp::projects",
                "cannot read create hint {}: {error}",
                path.display()
            );
            return None;
        }
    };
    let hint: PendingHint = match serde_json::from_str(&body) {
        Ok(hint) => hint,
        Err(error) => {
            tracing::warn!(
                target: "csp::projects",
                "ignoring malformed create hint {}: {error}",
                path.display()
            );
            return None;
        }
    };
    if hint.command_id != command_id {
        tracing::warn!(
            target: "csp::projects",
            "ignoring create hint {} whose commandId names {:?}",
            path.display(),
            hint.command_id
        );
        return None;
    }
    usable_directory(&hint.path).then_some(hint.path)
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
        // Set on every path below, including the ones that read nothing: a
        // create's one-shot hint lives beside this file rather than in it, and
        // a projects file that is missing or unreadable says nothing about
        // whether a hint was written.
        let hints_dir = path.parent().map(|parent| parent.join(PENDING_HINTS_DIR));
        let empty = || Self {
            hints_dir: hints_dir.clone(),
            ..Self::default()
        };
        let body = match std::fs::read_to_string(path) {
            Ok(body) => body,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return empty(),
            Err(error) => {
                tracing::warn!(target: "csp::projects", "cannot read {}: {error}", path.display());
                return empty();
            }
        };
        let mut file: Self = match serde_json::from_str(&body) {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(target: "csp::projects", "cannot parse {}: {error}", path.display());
                Self::default()
            }
        };
        file.hints_dir = hints_dir;
        file
    }

    /// Resolve a working directory: in-file pending hint, then the one-shot
    /// hint file, then project, then channel.
    ///
    /// The hint **file** sits second rather than first because an in-file
    /// `pending` entry is the stronger statement when it survived: the host
    /// wrote it into its own canonical store. The file is what covers the case
    /// where that entry did not survive, which is the ordinary case whenever
    /// the desktop saved anything between the CLI writing it and the create
    /// arriving (see [`PENDING_HINTS_DIR`]).
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
        if let Some(path) = self
            .pending
            .get(command_id)
            .filter(|path| usable_directory(path))
        {
            return Some(path.clone());
        }
        if let Some(path) = self
            .hints_dir
            .as_deref()
            .and_then(|hints_dir| read_pending_hint(hints_dir, command_id))
        {
            return Some(path);
        }
        // A spent hint still answers for its own command. Falling through to
        // the project or channel default here is the silent wrong-folder case
        // this whole mechanism exists to remove; a marker whose path is
        // unusable resolves to nothing at all, which
        // [`consumed_hint_without_record`] turns into a named refusal.
        if let Some(hints_dir) = self.hints_dir.as_deref() {
            if let Some(consumed) = read_consumed_hint(hints_dir, command_id) {
                return usable_directory(&consumed.path).then_some(consumed.path);
            }
        }
        [
            project_ref.and_then(|project_ref| self.projects.get(project_ref)),
            self.channels.get(&channel_id),
        ]
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

/// The refusal a create earns when its hint was already spent but this
/// provider has no record of the execution it was spent on.
///
/// The ordering makes this unreachable in the ordinary crash: the record is
/// persisted *before* the marker is written, so a marker implies a record. It
/// becomes reachable when the state file is lost and the hints directory is
/// not — and in that state the honest answer is "this command's directory was
/// already decided and I no longer know what ran there", not a fresh session
/// in whatever folder the channel default happens to name today.
///
/// Returns the message, or `None` when there is nothing to refuse.
pub fn consumed_hint_without_record(
    projects: &ProjectsFile,
    state: &StateStore,
    command_id: &str,
) -> Option<String> {
    let hints_dir = projects.hints_dir.as_deref()?;
    let consumed = read_consumed_hint(hints_dir, command_id)?;
    if state
        .sessions()
        .any(|record| record.command_id == command_id)
    {
        return None;
    }
    Some(format!(
        "this create's working-directory hint was already consumed for {}, but this provider \
         has no record of the execution it started; refusing rather than starting a second one \
         somewhere else",
        if consumed.path.as_os_str().is_empty() {
            "an unreadable path".to_owned()
        } else {
            consumed.path.display().to_string()
        }
    ))
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
            steer_idle_guard: None,
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
            channel_id: Uuid::nil(),
            runtimes: runtimes(),
            instance_id: "instance-1",
            now_secs,
            horizon_secs: 86_400,
            max_sessions: 4,
            // Unbudgeted by default: every decision test that predates D9
            // describes a provider with no crew budget configured.
            turn_budget: crate::config::UNLIMITED_TURN_BUDGET,
            policy_turn_budgets: HashMap::new(),
            active_session_count: state.live_session_count(),
            state,
            ci_continuations: no_ci_continuations(),
            projects,
            actor_seats,
            in_flight: no_commands_in_flight(),
            delivered_cancels: no_delivered_cancels(),
            claims_pending_reverification: no_pending_reverification(),
        }
    }

    /// The empty pending-reverification set: a provider that has not recovered
    /// is verified by construction, which is what every decision test here
    /// describes.
    fn no_pending_reverification() -> &'static HashSet<String> {
        static EMPTY: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
        EMPTY.get_or_init(HashSet::new)
    }

    /// The empty accepted-not-started set, leaked once so every decision test
    /// can borrow it for `'a` without threading a local through each call.
    fn no_commands_in_flight() -> &'static HashMap<String, crate::InFlightTurn> {
        static EMPTY: std::sync::OnceLock<HashMap<String, crate::InFlightTurn>> =
            std::sync::OnceLock::new();
        EMPTY.get_or_init(HashMap::new)
    }

    /// The empty CI-continuation store, leaked once for the same reason: a
    /// decision test that names no continuation must not have to own a state
    /// directory to run. Shared and immutable, so it never touches the disk
    /// again after it is opened.
    fn no_ci_continuations() -> &'static CiContinuationStore {
        static EMPTY: std::sync::OnceLock<CiContinuationStore> = std::sync::OnceLock::new();
        EMPTY.get_or_init(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            CiContinuationStore::open(dir.path()).expect("empty continuation store")
        })
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
            routing: None,
            resume_cursor: None,
            title: None,
            created_at_ms: 0,
            next_seq: 1,
            next_lease_sequence: 1,
            bootstrap_transport: None,
            open_turn: None,
            closed: false,
            created_by: None,
            handover: ClaimState::NoClaim,
            retired: None,
            pack_ref: None,
            compose_ref: None,
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
            hints_dir: None,
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
        let grantee = "ef".repeat(32);

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

        let mut founderless_state = store(&dir.path().join("founderless"));
        let mut founderless = session("founderless", dir.path());
        founderless.genesis_ref = Some("34".repeat(32));
        founderless.founder_pubkey = None;
        founderless.granted_operators = [grantee.clone()].into_iter().collect();
        founderless.authority_seq = 1;
        founderless_state
            .insert_session(founderless)
            .expect("insert founderless");
        assert!(matches!(
            decide_lifecycle(
                &ctx_as(&founderless_state, &projects, 1_000, &grantee),
                Uuid::nil(),
                1_000,
                &lifecycle_target_content(
                    "session.resume",
                    "resume-without-founder",
                    "founderless",
                    1,
                ),
            ),
            LifecycleDecision::Fail {
                code: UNAUTHORIZED_OPERATOR,
                ..
            }
        ));
    }

    /// A granted operator may steer and reconnect a genesis-bearing session,
    /// while stop remains owner-only — `grant-operator` never moves ownership.
    #[test]
    fn a_granted_operator_may_steer_and_resume_but_never_stop() {
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
        assert!(matches!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.resume", "resume-grantee", "s1", 1),
            ),
            LifecycleDecision::Resume(_)
        ));
        assert!(matches!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.stop", "stop-grantee", "s1", 1),
            ),
            LifecycleDecision::Fail {
                code: UNAUTHORIZED_OPERATOR,
                ..
            }
        ));
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
        assert!(matches!(
            decide_lifecycle(
                &ctx_as(&legacy_state, &projects, 1_000, &grantee),
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.resume", "resume-inert-grant", "s2", 1),
            ),
            LifecycleDecision::Fail {
                code: UNAUTHORIZED_OPERATOR,
                ..
            }
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
                attachments: Vec::new(),
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
        // Empty of *entries* — but the hints directory beside the file is
        // still named, because a projects file that is missing or unreadable
        // says nothing about whether a create wrote a one-shot hint.
        let beside = Some(dir.path().join(PENDING_HINTS_DIR));
        let absent = ProjectsFile::load(Some(&dir.path().join("absent.json")));
        assert_eq!(
            absent,
            ProjectsFile {
                hints_dir: beside.clone(),
                ..ProjectsFile::default()
            }
        );
        let broken = dir.path().join("broken.json");
        std::fs::write(&broken, b"{ not json").expect("write");
        assert_eq!(
            ProjectsFile::load(Some(&broken)),
            ProjectsFile {
                hints_dir: beside,
                ..ProjectsFile::default()
            }
        );
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
                operation_key: None,
                operator_pubkey: String::new(),
                steer_attempt: None,
                text: None,
                framing: None,
                fenced_after_dispatch: false,
                native_authority_refusal: None,
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
