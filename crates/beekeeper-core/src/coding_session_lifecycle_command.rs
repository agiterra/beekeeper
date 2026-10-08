//! Provider-neutral coding-session lifecycle command contract.
//!
//! Events use [`crate::kind::KIND_CODING_SESSION_LIFECYCLE_COMMAND`] and public
//! JSON so an installed provider adapter can create, resume, or stop a session.
//! Event authorship
//! is the authority; content carries no claimed actor identity or host-specific
//! execution state — notably, never a filesystem path.
//!
//! # Fork amendment: `projectRef` is optional
//!
//! The donor contract required every session to name a Beekeeper project. Here a
//! session may stand alone: `projectRef` is `Option<String>` and, when present,
//! must be a NIP-MP project coordinate (`30621:<owner>:<d>`). The field is still
//! *structurally required* in signed JSON — `null` must be written explicitly —
//! so a truncated or partially-serialized payload can never be mistaken for a
//! deliberate standalone session. See `docs/nips/NIP-CSL.md`.
//!
//! # Fork amendment: `sessionRef` umbrella reference
//!
//! `session.create` also carries a nullable `sessionRef` — a client-minted
//! lowercase UUID grouping several provider executions into one user-facing
//! umbrella session. Unlike `projectRef`, this field was added *after* the v1
//! schema shipped, so signed events without the key exist and must stay valid
//! forever. The decoder therefore accepts exactly one of three forms: the
//! historical 8-key action, the 9-key action including `sessionRef`, or the
//! 10-key action including both `sessionRef` and `genesisRef`. `genesisRef`
//! can never appear without `sessionRef`; nothing between or beyond those
//! forms is accepted. See `docs/nips/NIP-CSL.md`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::coding_session_command::{
    CodingSessionTarget, MAX_IDENTIFIER_BYTES, MAX_SAFE_GENERATION,
};
use crate::coding_session_identity::ProviderInstanceAlias;
use crate::coding_session_payload::ACTOR_ROLE_PAIR;
use crate::coding_session_routing::{HireRoutingRequest, RoutingRecord};
use crate::kind::KIND_PROJECT;

/// The currently supported coding-session lifecycle command envelope schema.
pub const CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA: &str =
    "buzz-coding-session-lifecycle-command/v1";
/// The version tag placed on each coding-session lifecycle command event.
pub const CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION: &str = "csl1-1";
/// Maximum UTF-8 byte length for a lifecycle command identifier.
pub const MAX_LIFECYCLE_COMMAND_ID_BYTES: usize = 256;
/// Maximum UTF-8 byte length for project, repository, provider, model, or title references.
pub const MAX_LIFECYCLE_REFERENCE_BYTES: usize = 2 * 1024;
/// Maximum UTF-8 byte length for an initial turn.
pub const MAX_LIFECYCLE_INITIAL_TURN_BYTES: usize = 12 * 1024;

/// The framing the founder's host puts in front of a hired seat's first turn.
///
/// Mirrors `CODING_SESSION_HIRE_BRIEF_PREFIX` in
/// `desktop/src/features/coding-sessions/lib/codingSessionHireSeat.ts:34`,
/// which is the only place that string is written on the way to the wire. It
/// is declared here so [`MAX_LIFECYCLE_HIRE_BRIEF_BYTES`] can measure the
/// prefix rather than restate its length as a number that can drift away from
/// it.
pub const CODING_SESSION_HIRE_BRIEF_PREFIX: &str = "[From the lead] ";

/// Maximum UTF-8 byte length for a `session.hire`'s `brief`.
///
/// A hire's brief is not stored as a brief: the founder's host publishes the
/// seat's create with `initialTurn` = [`CODING_SESSION_HIRE_BRIEF_PREFIX`] +
/// brief (`codingSessionHireSeat.ts:130-133`), and that create is validated at
/// [`MAX_LIFECYCLE_INITIAL_TURN_BYTES`]. So the brief's real ceiling is the
/// initial turn's minus the prefix the host adds; anything above it names a
/// seat no host can create.
///
/// **The narrowing has a disclosed consequence.** A hire already signed and
/// stored with a brief of 12,273..=12,288 bytes stops decoding, so readers
/// skip it — `find_hired_seat` does `let Ok(payload) = decode… else
/// { continue }` (`crates/beekeeper-cli/src/commands/sessions/crew.rs:1642-1648`).
/// That window is exactly the set of hires the founder's host has always
/// thrown on when it tried to build the create, so no hire that could ever be
/// seated is lost — but the events themselves are now unreadable rather than
/// readable-and-unseatable, and that is a real change, not an invisible one.
pub const MAX_LIFECYCLE_HIRE_BRIEF_BYTES: usize =
    MAX_LIFECYCLE_INITIAL_TURN_BYTES - CODING_SESSION_HIRE_BRIEF_PREFIX.len();
/// Maximum UTF-8 byte length for the complete signed event content.
pub const MAX_LIFECYCLE_CONTENT_BYTES: usize = 16 * 1024;
/// Maximum bytes in an agent seat's `role` slug.
///
/// A role is a label a human reads in an execution row and a sibling seat
/// resolves by name (`bee sessions send --to lead`), not prose: short enough to
/// fit a label, long enough for `verifier-secondary`.
pub const MAX_ROLE_SLUG_BYTES: usize = 64;

/// The kind segment every `projectRef` coordinate must carry.
///
/// Sessions bind to NIP-MP projects (kind 30621) and nothing else. The donor's
/// era of `30178:` team-catalog coordinates is not accepted here.
pub const PROJECT_REF_KIND_SEGMENT: &str = "30621";
const _: () = assert!(KIND_PROJECT == 30621);

/// Supported coding-session lifecycle actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CodingSessionLifecycleAction {
    /// Create a new provider session, optionally bound to a Beekeeper project.
    #[serde(rename = "session.create")]
    SessionCreate {
        /// Optional Beekeeper project reference (`30621:<owner>:<d>`).
        ///
        /// `None` creates a standalone session, owned by the channel it is
        /// published into rather than by a project.
        project_ref: Option<String>,
        /// Optional repository reference within the project.
        repo_ref: Option<String>,
        /// Optional umbrella session reference (canonical lowercase UUID).
        ///
        /// A later create carrying the same `sessionRef` joins the same
        /// umbrella as a new execution. `None` claims no umbrella — the
        /// pre-amendment semantics, an implicit umbrella of one. Decodes to
        /// `None` both from an explicit `null` and from the historical 8-key
        /// form that predates the field.
        session_ref: Option<String>,
        /// Optional event id of the immutable genesis founding this umbrella.
        ///
        /// Emitted only for the 10-key authority-aware form. A present
        /// reference requires a present `sessionRef`; consumers resolve it by
        /// event id rather than querying by the umbrella label.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        genesis_ref: Option<String>,
        /// Required capability-advertised provider instance **alias**
        /// (`claude-primary`).
        ///
        /// Typed since batch 2 lane B2: the field, not merely the read. An
        /// alias is not an instance id, and ledger item 102 is what happens
        /// when both are `String`. The newtype is `#[serde(transparent)]`, so
        /// nothing about the wire changed — every create that decoded
        /// yesterday still decodes.
        provider_instance_ref: ProviderInstanceAlias,
        /// Required signing pubkey of the selected provider catalog authority.
        provider_authority_pubkey: String,
        /// Optional provider-neutral model identifier.
        model: Option<String>,
        /// Optional operator-facing session title.
        title: Option<String>,
        /// Optional first turn to deliver after session creation.
        initial_turn: Option<String>,
        /// Optional agent seat: the pubkey whose identity this execution runs
        /// as, lowercase 64-hex (plan D1).
        ///
        /// The pubkey only *names* the seat. Its key material never appears
        /// here or anywhere else on the wire — the provider resolves it
        /// host-locally and refuses the create when it cannot
        /// ([`crate::coding_session_payload::ACTOR_UNAVAILABLE`]).
        ///
        /// Emitted only for a seated create, always together with
        /// [`role`](Self::SessionCreate::role); neither key is ever written as
        /// an explicit `null`, so an unseated create keeps the exact key sets
        /// pre-amendment consumers require.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        actor: Option<String>,
        /// Optional role slug this seat holds within the umbrella, e.g.
        /// `lead`, `architect`, `builder`.
        ///
        /// `[a-z0-9-]+`, 1..=[`MAX_ROLE_SLUG_BYTES`] bytes. Present exactly
        /// when [`actor`](Self::SessionCreate::actor) is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        /// Event id of the kind:44221 `session.hire` this create answers, when
        /// it answers one (COMMS-MAP §3).
        ///
        /// The seventh additive key, and optional for the same reason the
        /// others are: every create signed before the hire loop was closed
        /// must stay valid forever, so the key is **omitted** rather than
        /// written as an explicit `null` when the founder's host created the
        /// seat itself. It closes the attribution loop from the create's end:
        /// a seated create that names no hire is a seat nobody can trace back
        /// to a request, which is exactly the state the live run left behind.
        /// Lowercase 64-hex, resolved by event id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hire_ref: Option<String>,
        /// Why *this* execution target — the routing record echoed from the
        /// hire that produced this seat, or written by whatever chose the
        /// model (Brian's ruling of 2026-08-30).
        ///
        /// The sixth additive key, and optional for the same reason the others
        /// are: creates signed before the router existed must stay valid
        /// forever, so the key is omitted rather than written as an explicit
        /// `null` when nothing routed. When present it must be *complete* —
        /// [`crate::coding_session_routing::RoutingRecord::is_complete`] — because a
        /// create is the answer, not the question: a record with a null
        /// `chosen` on a create would claim a decision nobody made.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        routing: Option<RoutingRecord>,
    },
    /// Ask this umbrella's host to seat a new agent on a role (plan D14).
    ///
    /// A hire is a *request*, not a create: the signer names the umbrella, the
    /// role, and the brief, and the founder's host decides — under a standing
    /// policy the operator set — whether to seat anything at all. Nothing here
    /// names an identity, a working directory, or key material; choosing the
    /// identity and staging its custody are host-local steps, exactly as they
    /// are for a seated [`SessionCreate`](Self::SessionCreate). The seat that
    /// results is published as an ordinary seated create, and *its* receipts
    /// are this hire's receipts.
    ///
    /// A refusal is answered to the requesting seat as a kind:44220 turn
    /// prefixed [`HIRE_REFUSAL_PREFIX`], carrying one of [`HIRE_REFUSAL_CODES`].
    #[serde(rename = "session.hire")]
    SessionHire {
        /// Umbrella the hired seat joins; canonical lowercase UUID.
        session_ref: String,
        /// Event id of that umbrella's immutable genesis, lowercase 64-hex.
        ///
        /// Required, unlike the create's optional `genesisRef`: a hire is
        /// answered by an authority check against the genesis, so a hire that
        /// names no genesis names nothing that can authorize it.
        genesis_ref: String,
        /// Role slug the hired seat holds — `[a-z0-9-]+`, 1..=
        /// [`MAX_ROLE_SLUG_BYTES`] bytes, the same slug a seated create writes.
        role: String,
        /// Provider instance **alias** the host should run the seat on, or
        /// `null` to take the host policy's default. Written explicitly
        /// either way.
        ///
        /// Typed since batch 2 lane B2; see
        /// [`SessionCreate::provider_instance_ref`](Self::SessionCreate::provider_instance_ref).
        provider_instance_ref: Option<ProviderInstanceAlias>,
        /// Model the host should run the seat on, or `null` to take the
        /// chosen identity's own. Written explicitly either way.
        model: Option<String>,
        /// The brief. It becomes the seat's first turn verbatim (the host
        /// prefixes it), so it is required and non-empty:
        /// 1..=[`MAX_LIFECYCLE_HIRE_BRIEF_BYTES`] bytes — the initial turn's
        /// ceiling minus the host's [`CODING_SESSION_HIRE_BRIEF_PREFIX`],
        /// because the prefixed string is what is validated on the create.
        brief: String,
        /// Lowercase 64-hex pubkey of the seat that ran `bee sessions hire` —
        /// the hire event's own signer today (COMMS-MAP §3).
        ///
        /// An additive key, omitted rather than written as an explicit `null`,
        /// because hires signed before it existed are already on relays.
        ///
        /// It exists because a hired seat's brief arrived **unattributed**: the
        /// create's `initial_turn` is delivered with no framing and with the
        /// founder named as operator, so nothing on the wire recorded which
        /// lead asked for the seat — the only trace was a 16-byte
        /// `"[From the lead] "` prefix added in TypeScript.
        ///
        /// **The relay does not check this claim — and that is a choice, not
        /// an impossibility.** An earlier draft of this comment said the relay
        /// *cannot*; it can. It verifies the event signature and therefore
        /// holds `event.pubkey`, so comparing it with `action.requestedBy` is
        /// one line beside `hire_authority_verdict`. v1 leaves the check to
        /// the consumer because LANE-B1 §B1.2 scoped it there.
        ///
        /// The consequence is exact, and nothing enforces against it today:
        /// **any signer the relay admits for a hire can attribute the request
        /// to a different seat's pubkey.** Until a consumer calls
        /// [`hire_requester_matches_signer`](CodingSessionLifecycleCommandPayload::hire_requester_matches_signer),
        /// `requestedBy` is an *unverified claim*, not an attribution, and any
        /// surface that renders it must say so. B2 owns closing this: compare
        /// the claim with the signer at the CLI and at the founder's host, and
        /// disclose a mismatch rather than dropping it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_by: Option<String>,
        /// The routing **request** behind this hire, or absent when nothing
        /// routed (Brian's ruling of 2026-08-30).
        ///
        /// The one additive key on an otherwise exact seven-key action, and a
        /// [`HireRoutingRequest`] rather than a [`RoutingRecord`]: a hire
        /// carries the *question* — a class and a risk triple — because only
        /// the founder's host can see its own live kind:44222 catalog, and
        /// only the host may therefore decide. The requester's own local
        /// answer may ride along as [`HireRoutingRequest::proposed`], labelled
        /// informational and binding nothing.
        ///
        /// The two were one type until 2026-08-30, and the cost of that was
        /// exact: the CLI emitted the record here, the desktop host accepted
        /// only the request, and every routed hire was classified malformed
        /// and dropped without a kind:44220, a log line or a pixel (ledger
        /// draft 97). A record on a hire is now refused *by the name of the
        /// key that does not belong*.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        routing: Option<HireRoutingRequest>,
    },
    /// Reattach a disconnected, non-stopped execution as a new generation.
    #[serde(rename = "session.resume")]
    SessionResume {
        /// Exact previous generation being resumed.
        session: CodingSessionTarget,
        /// Signing pubkey of the provider authority that owns the target.
        provider_authority_pubkey: String,
    },
    /// Detach a live execution and reattach it at once as a new generation
    /// with a freshly staged seat, continuing from its cursor (spec § 4.9,
    /// **Restart with current definition**). The same shape and authority
    /// as a resume; unlike a stop, nothing becomes terminal. A provider
    /// refuses it while a turn is open (`SESSION_BUSY`).
    #[serde(rename = "session.restart")]
    SessionRestart {
        /// Exact current generation being restarted.
        session: CodingSessionTarget,
        /// Signing pubkey of the provider authority that owns the target.
        provider_authority_pubkey: String,
    },
    /// Detach a live execution and open a new generation whose context is
    /// cut at the start of a recorded turn (SV-29, **Edit from here**).
    ///
    /// `checkpoint` is the kind:44231 `turn` checkpoint of the first turn to
    /// forget: the transcript is cut at its `coverage.fromSeq − 1`, and with
    /// [`RewindFiles::Restore`] the working tree is returned to its
    /// `git.baseTree`. The same shape and authority as a restart plus those
    /// two keys; a provider refuses it while a turn is open (`SESSION_BUSY`).
    /// The new generation is a fresh conversation seeded from the signed
    /// record, never the native cursor, which still remembers the turns.
    #[serde(rename = "session.rewind")]
    SessionRewind {
        /// Exact current generation being rewound.
        session: CodingSessionTarget,
        /// Signing pubkey of the provider authority that owns the target.
        provider_authority_pubkey: String,
        /// Event id (64 lowercase hex) of the kind:44231 checkpoint whose
        /// turn is the first one rewound.
        checkpoint: String,
        /// Whether the working tree is left alone or restored.
        files: RewindFiles,
    },
    /// Durably stop an execution so a provider restart cannot revive it.
    #[serde(rename = "session.stop")]
    SessionStop {
        /// Exact current generation being stopped.
        session: CodingSessionTarget,
        /// Signing pubkey of the provider authority that owns the target.
        provider_authority_pubkey: String,
    },
}

/// What a `session.rewind` does to the working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RewindFiles {
    /// Rewind the conversation only; the files stay as they are.
    Keep,
    /// Also return the working tree to the checkpoint's `baseTree`.
    Restore,
}

impl CodingSessionLifecycleAction {
    /// The provider instance **alias** this action names, typed so it can
    /// never be compared with a
    /// [`crate::coding_session_identity::ProviderInstanceId`].
    ///
    /// `Ok(None)` means the action names no alias: a hire that left the choice
    /// to the host's policy, or a resume/stop, which name an exact
    /// `cs-target` instead. `Err` means the alias on the wire is not one —
    /// blank, oversized, or carrying control characters.
    ///
    /// This is an accessor rather than the field's own type. Ledger item 102's
    /// defect was comparing this alias with a receipt's cryptographic
    /// `cs-target.instanceId`; typing the *reads* closes that hole without
    /// changing a single byte on the wire, and without a construction-time
    /// failure mode appearing inside `bee sessions hire`. Migrating the fields
    /// themselves is B2's, whose crates hold every construction site.
    pub fn provider_instance_alias(&self) -> Result<Option<ProviderInstanceAlias>, String> {
        let raw = match self {
            Self::SessionCreate {
                provider_instance_ref,
                ..
            } => Some(provider_instance_ref.as_str()),
            Self::SessionHire {
                provider_instance_ref,
                ..
            } => provider_instance_ref
                .as_ref()
                .map(ProviderInstanceAlias::as_str),
            Self::SessionResume { .. }
            | Self::SessionRestart { .. }
            | Self::SessionRewind { .. }
            | Self::SessionStop { .. } => None,
        };
        // The field carries the newtype since B2, but `#[serde(transparent)]`
        // decoding does not run `from_wire` — a signed create may still carry
        // a blank or oversized alias. Re-validating here keeps the accessor's
        // three answers exactly what B1 froze.
        raw.map(ProviderInstanceAlias::from_wire).transpose()
    }
}

/// Durable coding-session lifecycle command JSON payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionLifecycleCommandPayload {
    /// Must equal [`CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA`].
    pub schema: String,
    /// Client-generated id used by provider adapters for idempotency.
    pub command_id: String,
    /// Requested lifecycle action.
    pub action: CodingSessionLifecycleAction,
}

impl CodingSessionLifecycleCommandPayload {
    /// The umbrella a `session.hire` names, or `None` for every other action.
    ///
    /// The relay's ingest gate uses this to decide whether a 44221 needs the
    /// founder-or-grant check: every other lifecycle action is authorized by
    /// the provider, not by the relay, so returning `None` for them keeps
    /// their gate exactly as it was.
    pub fn hire_session_ref(&self) -> Option<&str> {
        match &self.action {
            CodingSessionLifecycleAction::SessionHire { session_ref, .. } => Some(session_ref),
            _ => None,
        }
    }

    /// The pubkey a `session.hire` claims ran it, or `None` for every other
    /// action and for a hire that claimed none.
    ///
    /// `None` means *unknown*, never *mismatched*: a hire signed before the
    /// key existed is not a hire whose requester disagrees with its signer.
    pub fn hire_requested_by(&self) -> Option<&str> {
        match &self.action {
            CodingSessionLifecycleAction::SessionHire { requested_by, .. } => {
                requested_by.as_deref()
            }
            _ => None,
        }
    }

    /// Whether a hire's claimed requester equals the event's signer, or `None`
    /// when the action is not a hire or claimed no requester.
    ///
    /// The relay *could* make this check and in v1 does not (see
    /// [`SessionHire::requested_by`](CodingSessionLifecycleAction::SessionHire)),
    /// so the decoder puts both values in the consumer's hands and the CLI or
    /// the founder's host compares them. **Nothing calls this yet**, which is
    /// why an unverified `requestedBy` is the current state of the wire.
    ///
    /// Three answers, not two: `Some(true)` attributed, `Some(false)`
    /// disputed, `None` unclaimed. Collapsing the third into either of the
    /// others is how an unattributed seat comes to look attributed.
    pub fn hire_requester_matches_signer(&self, signer_pubkey_hex: &str) -> Option<bool> {
        self.hire_requested_by()
            .map(|requested_by| requested_by == signer_pubkey_hex)
    }

    /// The kind:44221 hire event id a `session.create` answers, or `None` for
    /// every other action and for a create that answers no hire.
    pub fn create_hire_ref(&self) -> Option<&str> {
        match &self.action {
            CodingSessionLifecycleAction::SessionCreate { hire_ref, .. } => hire_ref.as_deref(),
            _ => None,
        }
    }

    /// This action's provider instance **alias**, typed.
    ///
    /// See [`CodingSessionLifecycleAction::provider_instance_alias`]. Returns
    /// `Ok(None)` for a resume or a stop, which name a target rather than an
    /// instance to select.
    pub fn provider_instance_alias(&self) -> Result<Option<ProviderInstanceAlias>, String> {
        self.action.provider_instance_alias()
    }

    /// Validate all payload fields before signing a lifecycle command.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA {
            return Err("unsupported coding-session lifecycle command schema".into());
        }
        validate_required(
            &self.command_id,
            "commandId",
            MAX_LIFECYCLE_COMMAND_ID_BYTES,
        )?;
        match &self.action {
            CodingSessionLifecycleAction::SessionCreate {
                project_ref,
                repo_ref,
                session_ref,
                genesis_ref,
                provider_instance_ref,
                provider_authority_pubkey,
                model,
                title,
                initial_turn,
                actor,
                role,
                hire_ref,
                routing,
            } => {
                validate_actor_role_pair(actor.as_deref(), role.as_deref())?;
                if let Some(project_ref) = project_ref {
                    validate_required(
                        project_ref,
                        "action.projectRef",
                        MAX_LIFECYCLE_REFERENCE_BYTES,
                    )?;
                    validate_project_ref(project_ref)?;
                }
                if let Some(session_ref) = session_ref {
                    validate_session_ref(session_ref)?;
                }
                if let Some(genesis_ref) = genesis_ref {
                    if session_ref.is_none() {
                        return Err(
                            "action.genesisRef requires a non-null action.sessionRef".into()
                        );
                    }
                    validate_event_id_hex("action.genesisRef", genesis_ref)?;
                }
                validate_optional(repo_ref, "action.repoRef", MAX_LIFECYCLE_REFERENCE_BYTES)?;
                validate_required(
                    provider_instance_ref.as_str(),
                    "action.providerInstanceRef",
                    MAX_LIFECYCLE_REFERENCE_BYTES,
                )?;
                validate_provider_authority_pubkey(provider_authority_pubkey)?;
                validate_optional(model, "action.model", MAX_LIFECYCLE_REFERENCE_BYTES)?;
                validate_optional(title, "action.title", MAX_LIFECYCLE_REFERENCE_BYTES)?;
                validate_optional(
                    initial_turn,
                    "action.initialTurn",
                    MAX_LIFECYCLE_INITIAL_TURN_BYTES,
                )?;
                if let Some(actor) = actor {
                    validate_actor_pubkey(actor)?;
                }
                if let Some(role) = role {
                    validate_role_slug(role)?;
                }
                if let Some(hire_ref) = hire_ref {
                    validate_event_id_hex("action.hireRef", hire_ref)?;
                }
                if let Some(routing) = routing {
                    routing
                        .validate()
                        .map_err(|detail| format!("action.{detail}"))?;
                    // A create is the answer, not the question. A record with
                    // a null `chosen` would put a decision on the wire that
                    // nobody actually made.
                    if !routing.is_complete() {
                        return Err(
                            "action.routing on a create must carry chosen, reason, reviewRequired \
                             and registryVersion: a create records the decision, not the question"
                                .into(),
                        );
                    }
                }
            }
            CodingSessionLifecycleAction::SessionHire {
                session_ref,
                genesis_ref,
                role,
                provider_instance_ref,
                model,
                brief,
                requested_by,
                routing,
            } => {
                validate_session_ref(session_ref)?;
                validate_event_id_hex("action.genesisRef", genesis_ref)?;
                validate_role_slug(role)?;
                if let Some(provider_instance_ref) = provider_instance_ref {
                    validate_required(
                        provider_instance_ref.as_str(),
                        "action.providerInstanceRef",
                        MAX_LIFECYCLE_REFERENCE_BYTES,
                    )?;
                }
                validate_optional(model, "action.model", MAX_LIFECYCLE_REFERENCE_BYTES)?;
                validate_hire_brief(brief)?;
                if let Some(requested_by) = requested_by {
                    validate_pubkey_hex("action.requestedBy", requested_by)?;
                }
                if let Some(routing) = routing {
                    routing
                        .validate()
                        .map_err(|detail| format!("action.{detail}"))?;
                }
            }
            CodingSessionLifecycleAction::SessionResume {
                session,
                provider_authority_pubkey,
            }
            | CodingSessionLifecycleAction::SessionRestart {
                session,
                provider_authority_pubkey,
            }
            | CodingSessionLifecycleAction::SessionStop {
                session,
                provider_authority_pubkey,
            } => {
                validate_target(session)?;
                validate_provider_authority_pubkey(provider_authority_pubkey)?;
            }
            CodingSessionLifecycleAction::SessionRewind {
                session,
                provider_authority_pubkey,
                checkpoint,
                files: _,
            } => {
                validate_target(session)?;
                validate_provider_authority_pubkey(provider_authority_pubkey)?;
                validate_event_id_hex("action.checkpoint", checkpoint)?;
            }
        }
        Ok(())
    }
}

/// Strictly decode and validate signed lifecycle-command content.
///
/// Nullable action fields must be present explicitly, even when their value is
/// `null`. Unknown, duplicate, or missing fields are rejected. The one
/// exceptions are the additive authority fields: create carries exactly the
/// historical 8-key set, the 9-key set including `sessionRef`, or the 10-key
/// set including both `sessionRef` and `genesisRef`.
pub fn decode_coding_session_lifecycle_command(
    content: &str,
) -> Result<CodingSessionLifecycleCommandPayload, String> {
    if content.len() > MAX_LIFECYCLE_CONTENT_BYTES {
        return Err(format!(
            "coding-session lifecycle command content exceeds {MAX_LIFECYCLE_CONTENT_BYTES} bytes"
        ));
    }

    let value: Value = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session lifecycle command payload".to_string())?;
    require_exact_fields(&value, &["schema", "commandId", "action"], "payload")?;
    let action = value
        .get("action")
        .ok_or_else(|| "coding-session lifecycle command payload missing action".to_string())?;
    match action.get("type").and_then(Value::as_str) {
        Some("session.create") => {
            // The seat pair is checked before the shape so a half-written seat
            // is answered by name rather than by the generic shape error.
            require_seat_pair_shape(action)?;
            // Absent is not null on the additive hire reference: a producer
            // with nothing to say omits the key. Checked before the shape
            // matrix, because an explicit null is *present* to a key-set check
            // and would then decode as `None` — a shape pun that lets one
            // client write bytes a strict reader silently reinterprets.
            reject_explicit_null(action, "hireRef")?;
            reject_foreign_additive_key(
                action,
                "requestedBy",
                "hire",
                "create",
                "requestedBy names the seat that ran `bee sessions hire`; a create names the \
                 hire it answers with hireRef",
            )?;
            let seated = action.get("actor").is_some();
            let routed = action.get("routing").is_some();
            let attributed = action.get("hireRef").is_some();
            // Three historical key sets × seated-or-not × attributed-or-not
            // × routed-or-not. Each additive key is an independent axis, so
            // the forms are built rather than listed: a hand-written list of
            // twenty-four is a list that grows a hole the next time a key is
            // added.
            let mut forms: Vec<Vec<&str>> = Vec::with_capacity(24);
            for base in CREATE_ACTION_FORMS {
                for with_seat in [false, true] {
                    let mut seat_form = base.to_vec();
                    if with_seat {
                        seat_form.push("actor");
                        seat_form.push("role");
                    }
                    for with_hire in [false, true] {
                        let mut form = seat_form.clone();
                        if with_hire {
                            form.push("hireRef");
                        }
                        let mut routed_form = form.clone();
                        routed_form.push("routing");
                        forms.push(form);
                        forms.push(routed_form);
                    }
                }
            }
            let forms: Vec<&[&str]> = forms
                .iter()
                .filter(|form| {
                    form.contains(&"actor") == seated
                        && form.contains(&"routing") == routed
                        && form.contains(&"hireRef") == attributed
                })
                .map(Vec::as_slice)
                .collect();
            require_exact_field_forms(action, &forms, "action")?;
            if let Some(routing) = action.get("routing") {
                require_routing_record_shape(routing)?;
            }
            if action.get("genesisRef").is_some()
                && (action.get("sessionRef").and_then(Value::as_str).is_none()
                    || action.get("genesisRef").and_then(Value::as_str).is_none())
            {
                return Err(
                    "coding-session lifecycle command action.genesisRef requires non-null string sessionRef and genesisRef"
                        .into(),
                );
            }
        }
        Some("session.hire") => {
            // Same rule as the create's `hireRef`: omitted, never null.
            reject_explicit_null(action, "requestedBy")?;
            reject_foreign_additive_key(
                action,
                "hireRef",
                "create",
                "hire",
                "a hire cannot answer itself; the founder's host writes hireRef on the create it \
                 publishes in reply",
            )?;
            // `requestedBy` and `routing` are independent axes, so the four
            // accepted hire shapes are built from the base form rather than
            // listed — the same discipline the create above uses, and for
            // the same reason.
            let requested = action.get("requestedBy").is_some();
            let routed = action.get("routing").is_some();
            let mut forms: Vec<Vec<&str>> = Vec::with_capacity(4);
            for with_requester in [false, true] {
                let mut form = HIRE_ACTION_FORM.to_vec();
                if with_requester {
                    form.push("requestedBy");
                }
                let mut routed_form = form.clone();
                routed_form.push("routing");
                forms.push(form);
                forms.push(routed_form);
            }
            let forms: Vec<&[&str]> = forms
                .iter()
                .filter(|form| {
                    form.contains(&"requestedBy") == requested
                        && form.contains(&"routing") == routed
                })
                .map(Vec::as_slice)
                .collect();
            require_exact_field_forms(action, &forms, "action")?;
            // Checked here, before serde, because serde's own
            // `deny_unknown_fields` error is collapsed below into the single
            // sentence "malformed coding-session lifecycle command payload".
            // That sentence is exactly what a host answered nothing to on
            // 2026-08-30: a lead cannot act on it. Name the key.
            if let Some(routing) = action.get("routing") {
                require_hire_routing_request_shape(routing)?;
            }
        }
        Some("session.resume" | "session.restart" | "session.stop") => require_exact_fields(
            action,
            &["type", "session", "providerAuthorityPubkey"],
            "action",
        )?,
        Some("session.rewind") => require_exact_fields(
            action,
            &[
                "type",
                "session",
                "providerAuthorityPubkey",
                "checkpoint",
                "files",
            ],
            "action",
        )?,
        _ => return Err("coding-session lifecycle command action type is unsupported".into()),
    }

    // Decode a second time into the strict serde type. This preserves serde's
    // duplicate-field detection, which a Value alone cannot represent.
    let payload: CodingSessionLifecycleCommandPayload = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session lifecycle command payload".to_string())?;
    payload.validate()?;
    Ok(payload)
}

/// The keys of `object` that `allowed` does not name, ordered by `wire_order`
/// first and then alphabetically, so the sentence a lead reads is stable.
fn stray_keys(
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
    wire_order: &[&str],
) -> Vec<String> {
    let mut stray: Vec<String> = object
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect();
    stray.sort_by_key(|key| {
        (
            wire_order
                .iter()
                .position(|known| known == key)
                .unwrap_or(usize::MAX),
            key.clone(),
        )
    });
    stray
}

/// `"a", "b"` — the keys as a lead reads them.
fn quoted(keys: &[String]) -> String {
    keys.iter()
        .map(|key| format!("{key:?}"))
        .collect::<Vec<String>>()
        .join(", ")
}

/// [`quoted`] over borrowed keys.
fn quoted_refs(keys: &[&String]) -> String {
    keys.iter()
        .map(|key| format!("{key:?}"))
        .collect::<Vec<String>>()
        .join(", ")
}

/// Every key a `session.hire`'s `routing` may carry, in wire order.
///
/// The hire carries the routing **request**. `class` and `risk` are required;
/// the rest are omitted when they have nothing to say.
pub const HIRE_ROUTING_REQUEST_KEYS: &[&str] = &[
    "class",
    "risk",
    "profile",
    "override",
    "challengerSample",
    "reviewFlags",
    "proposed",
];

/// Every key a `session.create`'s `routing` may carry, in wire order.
///
/// The create carries the routing **record**: the first thirteen are always
/// written, `null` where the answer is genuinely absent, and
/// `proposedDisagreement` is written only when the host overruled the
/// requester's `proposed`.
pub const ROUTING_RECORD_KEYS: &[&str] = &[
    "class",
    "tier",
    "risk",
    "profile",
    "chosen",
    "runnerUp",
    "reason",
    "reviewRequired",
    "reviewReasons",
    "challengerSample",
    "override",
    "registryVersion",
    "catalogRevision",
    "proposedDisagreement",
];

/// The three keys a request's `risk` carries. No `score`: the product is
/// arithmetic the host does, and a score a requester could set is a number
/// that can disagree with its own factors.
const HIRE_ROUTING_RISK_KEYS: &[&str] = &["impact", "uncertainty", "irreversibility"];

/// Refuse a routing **record** on a `session.hire`, naming the key.
///
/// This is the fix for the failure of 2026-08-30 09:52: `bee sessions hire`
/// emitted the record, the desktop host's parser accepted only the request,
/// and the hire was classified malformed and dropped with no answer of any
/// kind. Serde would refuse it too, but the decoder collapses serde's error
/// into one unactionable sentence — so the offending key is named here, with
/// the way out, before serde ever runs.
fn require_hire_routing_request_shape(routing: &Value) -> Result<(), String> {
    let object = routing.as_object().ok_or_else(|| {
        "action.routing on a hire must be an object: the routing request \
         {class, risk, …}. Run `bee sessions route` and hire again with the request shape \
         (`bee sessions hire --help`)"
            .to_owned()
    })?;
    // Every offending key at once, in wire order. Naming one at a time turns
    // a fix into a loop of re-signed hires, each answered by the next key.
    let stray = stray_keys(object, HIRE_ROUTING_REQUEST_KEYS, ROUTING_RECORD_KEYS);
    if !stray.is_empty() {
        let from_the_record: Vec<&String> = stray
            .iter()
            .filter(|key| ROUTING_RECORD_KEYS.contains(&key.as_str()))
            .collect();
        return Err(format!(
            "action.routing on a hire is the routing REQUEST, and {} {} not one of its keys \
             ({}){}. Run `bee sessions route` and hire again with the request shape \
             (`bee sessions hire --help`)",
            quoted(&stray),
            if stray.len() == 1 { "is" } else { "are" },
            HIRE_ROUTING_REQUEST_KEYS.join(", "),
            if from_the_record.is_empty() {
                String::new()
            } else {
                format!(
                    " — {} belong{} to the routing record the host writes on the create",
                    quoted_refs(&from_the_record),
                    if from_the_record.len() == 1 { "s" } else { "" }
                )
            }
        ));
    }
    for required in ["class", "risk"] {
        if !object.contains_key(required) {
            return Err(format!(
                "action.routing on a hire must carry {required:?}: the lead names a class and \
                 a risk triple, and the host routes. Run `bee sessions route` and hire again \
                 with the request shape (`bee sessions hire --help`)"
            ));
        }
    }
    let risk = object
        .get("risk")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            "action.routing.risk on a hire must be an object of impact, uncertainty and \
         irreversibility. Run `bee sessions route` and hire again with the request shape \
         (`bee sessions hire --help`)"
                .to_owned()
        })?;
    for key in risk.keys() {
        if HIRE_ROUTING_RISK_KEYS.contains(&key.as_str()) {
            continue;
        }
        return Err(format!(
            "action.routing.risk on a hire carries {key:?}, which is not one of its three \
             factors ({}){}. Run `bee sessions route` and hire again with the request shape \
             (`bee sessions hire --help`)",
            HIRE_ROUTING_RISK_KEYS.join(", "),
            if key == "score" {
                " — the product belongs to the record the host writes, so that a score can \
                 never disagree with its own factors"
            } else {
                ""
            }
        ));
    }
    Ok(())
}

/// Refuse a routing **request** on a `session.create`, naming the key.
///
/// The mirror of [`require_hire_routing_request_shape`]. A create is the
/// answer; a create carrying only the question would put a decision on the
/// wire that nobody made.
fn require_routing_record_shape(routing: &Value) -> Result<(), String> {
    let object = routing
        .as_object()
        .ok_or_else(|| "action.routing on a create must be an object".to_owned())?;
    let stray = stray_keys(object, ROUTING_RECORD_KEYS, HIRE_ROUTING_REQUEST_KEYS);
    if !stray.is_empty() {
        let from_the_request: Vec<&String> = stray
            .iter()
            .filter(|key| HIRE_ROUTING_REQUEST_KEYS.contains(&key.as_str()))
            .collect();
        return Err(format!(
            "action.routing on a create is the routing RECORD, and {} {} not one of its keys \
             ({}){}",
            quoted(&stray),
            if stray.len() == 1 { "is" } else { "are" },
            ROUTING_RECORD_KEYS.join(", "),
            if from_the_request.is_empty() {
                String::new()
            } else {
                format!(
                    " — {} belong{} to the routing request a hire carries; the host routes \
                     and writes the record",
                    quoted_refs(&from_the_request),
                    if from_the_request.len() == 1 { "s" } else { "" }
                )
            }
        ));
    }
    Ok(())
}

/// Check that `project_ref` is a canonical NIP-MP project coordinate.
///
/// Splits on the first two colons only, matching how NIP-09 deletion handling
/// and NIP-MP member parsing read coordinates, so a project whose `d` tag
/// contains a colon stays addressable. Owner hex must be lowercase: `#a` filter
/// matching is byte-exact, so an uppercase-owner coordinate would be invisible
/// to the queries readers actually issue.
pub fn validate_project_ref(project_ref: &str) -> Result<(), String> {
    let malformed = || {
        format!(
            "action.projectRef must be \
             `{PROJECT_REF_KIND_SEGMENT}:<lowercase-64-hex-owner>:<project-d>` \
             (got {project_ref:?})"
        )
    };
    let mut segments = project_ref.splitn(3, ':');
    let (Some(kind), Some(owner), Some(project_d)) =
        (segments.next(), segments.next(), segments.next())
    else {
        return Err(malformed());
    };
    if kind != PROJECT_REF_KIND_SEGMENT {
        return Err(malformed());
    }
    if owner.len() != 64
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(malformed());
    }
    if project_d.is_empty() {
        return Err(malformed());
    }
    Ok(())
}

/// Check that `session_ref` is a canonical lowercase hyphenated UUID.
///
/// The umbrella reference never rides in a tag, so nothing downstream
/// normalizes it — two clients only agree on membership if the bytes are
/// byte-exact. Canonical form is therefore the contract: 36 characters,
/// `8-4-4-4-12`, lowercase hex. Uppercase, braces, URNs, and truncations are
/// all rejected rather than coerced.
pub fn validate_session_ref(session_ref: &str) -> Result<(), String> {
    let malformed = || {
        format!(
            "action.sessionRef must be a canonical lowercase hyphenated UUID \
             (got {session_ref:?})"
        )
    };
    let bytes = session_ref.as_bytes();
    if bytes.len() != 36 {
        return Err(malformed());
    }
    for (index, byte) in bytes.iter().enumerate() {
        let valid = match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(byte),
        };
        if !valid {
            return Err(malformed());
        }
    }
    Ok(())
}

/// Check that a Nostr public key is canonical lowercase 64-hex.
///
/// The same rule [`validate_actor_pubkey`] applies, parameterized by field
/// name so an additive key answers with its own name rather than borrowing
/// `action.actor`'s.
pub fn validate_pubkey_hex(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{field} must be a lowercase 64-hex public key"));
    }
    Ok(())
}

/// Refuse an additive key written as an explicit `null`, naming it.
///
/// Absent and null are different claims. A key-set check sees a null as
/// *present*, and serde then decodes it to `None` — so without this the same
/// signed bytes mean "unset" to one reader and "the additive form" to another,
/// which is precisely the divergence the item-102 ingress rule forbids.
fn reject_explicit_null(action: &Value, key: &str) -> Result<(), String> {
    if action.get(key).is_some_and(Value::is_null) {
        return Err(format!(
            "coding-session lifecycle command action.{key} must be omitted when it is not set, \
             never written as an explicit null"
        ));
    }
    Ok(())
}

/// Refuse the *other* action's additive key, naming it and saying where it
/// belongs.
///
/// `requestedBy` and `hireRef` are the two halves of one loop — the hire names
/// who asked, the create names the hire it answers — and putting either on the
/// wrong action is the mistake most likely to sit beside the one
/// [`reject_explicit_null`] already catches by name. Without this pass both
/// fall through to `require_exact_field_forms`, whose sentence is
/// "action has missing or unsupported fields": true, unactionable, and exactly
/// the kind of answer that cost a lead fifteen minutes on 2026-08-30. Checked
/// before the shape matrix so the key is named rather than counted.
fn reject_foreign_additive_key(
    action: &Value,
    key: &str,
    belongs_on: &str,
    this_action: &str,
    remedy: &str,
) -> Result<(), String> {
    if action.get(key).is_some() {
        return Err(format!(
            "coding-session lifecycle command action.{key} is a {belongs_on} field and does not \
             belong on a {this_action}: {remedy}"
        ));
    }
    Ok(())
}

/// Check that a Nostr event reference is canonical lowercase 64-hex.
pub fn validate_event_id_hex(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{field} must be a lowercase 64-hex event id"));
    }
    Ok(())
}

/// The base `session.hire` key set, before either additive key.
///
/// Unlike the create, a hire has no *historical* forms to keep valid — it is
/// new with the relay that validates it — so exactly these seven keys are the
/// floor. Two independent additive keys sit on top of it, each omitted rather
/// than written as an explicit `null`, giving four accepted shapes in total:
/// `routing` (the router request, Brian's ruling of 2026-08-30) and
/// `requestedBy` (the requesting seat, COMMS-MAP §3). The decoder builds the
/// four from this base rather than listing them, so the next additive key
/// cannot leave a hole in a hand-written list.
const HIRE_ACTION_FORM: &[&str] = &[
    "type",
    "sessionRef",
    "genesisRef",
    "role",
    "providerInstanceRef",
    "model",
    "brief",
];

/// The exact prefix a host writes on the kind:44220 turn that answers a
/// refused hire, so the requesting seat and `bee sessions hire` recognize a
/// refusal without parsing prose: `"hire refused: <code> — <reason>"`.
pub const HIRE_REFUSAL_PREFIX: &str = "hire refused: ";

/// Every refusal code a host may answer a `session.hire` with.
///
/// A refusal names which standing policy stopped the hire, so the lead can
/// act on it rather than retry blindly. `HIRE_NO_IDENTITY` and
/// `HIRE_NO_PROJECT_AGENT` are the ones whose remedy is the operator's
/// ("install team roles", or associate an agent with the session's project);
/// `HIRE_ROLE_BUSY` and
/// `HIRE_MODEL_NOT_OFFERED` and `HIRE_STALE` are facts about the request the
/// lead can fix by itself; the rest are policy.
///
/// `HIRE_ROLE_BUSY` and `HIRE_NO_IDENTITY` are deliberately two codes for
/// what was one until 2026-08-28: a role every one of whose identities is
/// already seated is not a role this computer lacks, and telling a lead to
/// install a role it already holds is an unactionable answer to a question
/// that had an answer — brief the seat that exists (item 88(h)).
pub const HIRE_REFUSAL_CODES: &[&str] = &[
    // Hiring is switched off for this host.
    "HIRE_OFF",
    // The role is not on the host's allowed-roles list.
    "HIRE_ROLE_NOT_ALLOWED",
    // The umbrella already holds the host's maximum live seats.
    "HIRE_LIMIT",
    // No installed managed agent holds this home role at all. Only the
    // operator can fix this, by installing the role on this computer.
    "HIRE_NO_IDENTITY",
    // The session belongs to a project and this computer holds no agent of
    // that project with this primary role. Agents of the role that belong to
    // another project (or to none) are never borrowed; the reason gives their
    // count. Only the operator can fix this, by installing the project's roles
    // or associating an agent with the project.
    "HIRE_NO_PROJECT_AGENT",
    // This computer holds the role, and every identity that *is* it is
    // already seated in this umbrella. Nothing is broken and nothing needs
    // installing: the lead addresses the seat the reason names instead.
    "HIRE_ROLE_BUSY",
    // The requested provider instance is not on the host's allowed list.
    "HIRE_PROVIDER_NOT_ALLOWED",
    // The requested model is not one the chosen provider's catalog offers,
    // and it is not an alias the host could translate into one. The reason
    // carries the offered ids, because a model id is only ever the catalog's.
    "HIRE_MODEL_NOT_OFFERED",
    // The request is older than the host's answering window. A host that only
    // now observed it seats nothing, so a lead is never surprised by a seat
    // from an hour ago.
    "HIRE_STALE",
    // Nothing the live catalog offers clears this class's gate at this risk
    // tier. The reason names the binding trait, the minimum it wanted, and the
    // best score anything available actually has. It is deliberately a refusal
    // rather than a quiet demotion to the next model down: "the smartest
    // available" and "the cheapest that fits" are both wrong answers to a
    // requirement nothing meets.
    "HIRE_NO_ROUTE",
    // The hire's `routing` did not parse against the request shape. Named
    // rather than dropped, because dropping it is the bug this code exists
    // to close: on 2026-08-30 a routed hire the relay had accepted was
    // classified malformed by the host and discarded with no 44220, no log
    // line and nothing on screen, and the lead waited fifteen minutes for an
    // answer that was never going to come (ledger draft 97). A request that
    // gets no answer is a crash with better manners.
    "HIRE_MALFORMED",
    // This computer has no recorded checkout to cut the seat's worktree
    // from: the session's project has no repository folder set, and (for a
    // projectless session) the channel remembers none either. Only the
    // operator can fix this, by setting Project settings → This computer →
    // Repository folder. Named rather than seating an agent with no tree
    // (ledger 135(a), 136).
    "HIRE_CHECKOUT_NOT_RECORDED",
    // The host cut the seat's worktree and then could not stage the seat
    // itself — its key material, its role pack, or (spec § 4.11) its clone of
    // the project's agents repository. The reason carries the host's own
    // error text verbatim, because what failed is host-local and only its own
    // words say which part. Named rather than left silent: on 2026-09-19 a
    // staging failure produced no refusal at all and the lead's `bee sessions
    // hire` waited out its window on an answer nobody was going to send
    // (ledger 169). The host disposes of the tree it cut before it refuses,
    // so a retry after the operator's fix cuts a fresh one.
    "HIRE_SEAT_STAGING_FAILED",
];

/// The three historical create key sets, oldest first, before the additive
/// agent-seat pair. Each is accepted alone or with `actor` + `role` appended.
const CREATE_ACTION_FORMS: &[&[&str]] = &[
    &[
        "type",
        "projectRef",
        "repoRef",
        "providerInstanceRef",
        "providerAuthorityPubkey",
        "model",
        "title",
        "initialTurn",
    ],
    &[
        "type",
        "projectRef",
        "repoRef",
        "sessionRef",
        "providerInstanceRef",
        "providerAuthorityPubkey",
        "model",
        "title",
        "initialTurn",
    ],
    &[
        "type",
        "projectRef",
        "repoRef",
        "sessionRef",
        "genesisRef",
        "providerInstanceRef",
        "providerAuthorityPubkey",
        "model",
        "title",
        "initialTurn",
    ],
];

/// Check that a create action carries both seat keys as non-null strings, or
/// neither of them.
///
/// Runs on the raw JSON, before the key-set check, because "half a seat" is a
/// distinguishable mistake that deserves its own name
/// ([`ACTOR_ROLE_PAIR`]) rather than the shape check's catch-all.
fn require_seat_pair_shape(action: &Value) -> Result<(), String> {
    let actor = action.get("actor");
    let role = action.get("role");
    if actor.is_none() && role.is_none() {
        return Ok(());
    }
    let both_named = actor
        .and_then(Value::as_str)
        .zip(role.and_then(Value::as_str))
        .is_some();
    if both_named {
        return Ok(());
    }
    Err(format!(
        "{ACTOR_ROLE_PAIR}: coding-session lifecycle command action.actor and action.role \
         must both be present as non-null strings, or both be absent"
    ))
}

/// Same rule, applied to the decoded type so a programmatic producer cannot
/// build half a seat and sign it.
fn validate_actor_role_pair(actor: Option<&str>, role: Option<&str>) -> Result<(), String> {
    if actor.is_some() == role.is_some() {
        return Ok(());
    }
    Err(format!(
        "{ACTOR_ROLE_PAIR}: action.actor and action.role must both be set or both be unset"
    ))
}

/// Check that an agent seat's pubkey is canonical lowercase 64-hex.
///
/// Same canonical form as every other pubkey on this contract: the value is
/// compared byte-for-byte against relay-signed authority facts, so an `npub`
/// or an uppercase copy is rejected rather than coerced.
pub fn validate_actor_pubkey(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("action.actor must be a lowercase 64-hex public key".into());
    }
    Ok(())
}

/// Check that a role slug is `[a-z0-9-]+` within
/// [`MAX_ROLE_SLUG_BYTES`].
///
/// A role is resolved by name across a crew, so the same rule that makes a
/// `d` tag addressable applies: one canonical spelling, no case folding, no
/// whitespace.
pub fn validate_role_slug(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_ROLE_SLUG_BYTES {
        return Err(format!(
            "action.role must be 1..={MAX_ROLE_SLUG_BYTES} bytes (got {} bytes)",
            value.len()
        ));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(format!(
            "action.role must be a lowercase [a-z0-9-] slug (got {value:?})"
        ));
    }
    Ok(())
}

fn require_exact_fields(value: &Value, expected: &[&str], field: &str) -> Result<(), String> {
    require_exact_fields_with_optional(value, expected, &[], field)
}

/// Require every `expected` key and allow — without requiring — the `optional`
/// ones. Any key outside both sets is still a hard rejection, so "optional"
/// here means exactly "a later schema revision's additive key", never "extra
/// data tolerated".
fn require_exact_fields_with_optional(
    value: &Value,
    expected: &[&str],
    optional: &[&str],
    field: &str,
) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("coding-session lifecycle command {field} must be an object"))?;
    let complete = expected.iter().all(|key| object.contains_key(*key));
    let recognized = object
        .keys()
        .all(|key| expected.contains(&key.as_str()) || optional.contains(&key.as_str()));
    if !complete || !recognized {
        return Err(format!(
            "coding-session lifecycle command {field} has missing or unsupported fields"
        ));
    }
    Ok(())
}

fn require_exact_field_forms(
    value: &Value,
    accepted: &[&[&str]],
    field: &str,
) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("coding-session lifecycle command {field} must be an object"))?;
    if accepted
        .iter()
        .any(|keys| object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key)))
    {
        return Ok(());
    }
    Err(format!(
        "coding-session lifecycle command {field} has missing or unsupported fields"
    ))
}

fn validate_required(value: &str, field: &str, max_bytes: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.len() > max_bytes {
        return Err(format!("{field} exceeds {max_bytes} bytes"));
    }
    Ok(())
}

/// Validate a `session.hire`'s `brief` against its **effective** ceiling.
///
/// Not [`validate_required`] with a different number, because the refusal has
/// to say why the number is not the one every other free-text field carries: a
/// lead reading `exceeds 12272 bytes` about a field documented at 12,288 would
/// reasonably conclude the relay is wrong. See
/// [`MAX_LIFECYCLE_HIRE_BRIEF_BYTES`].
fn validate_hire_brief(brief: &str) -> Result<(), String> {
    if brief.trim().is_empty() {
        return Err("action.brief must not be empty".into());
    }
    if brief.len() > MAX_LIFECYCLE_HIRE_BRIEF_BYTES {
        return Err(format!(
            "coding-session lifecycle command action.brief exceeds \
             {MAX_LIFECYCLE_HIRE_BRIEF_BYTES} bytes (got {got}): a hire's brief becomes the \
             seat's first turn behind the host's {prefix_bytes}-byte {prefix:?} prefix, so its \
             ceiling is the initial-turn ceiling minus that prefix",
            got = brief.len(),
            prefix_bytes = CODING_SESSION_HIRE_BRIEF_PREFIX.len(),
            prefix = CODING_SESSION_HIRE_BRIEF_PREFIX,
        ));
    }
    Ok(())
}

fn validate_optional(value: &Option<String>, field: &str, max_bytes: usize) -> Result<(), String> {
    if let Some(value) = value {
        validate_required(value, field, max_bytes)?;
    }
    Ok(())
}

fn validate_provider_authority_pubkey(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("action.providerAuthorityPubkey must be a lowercase 64-hex public key".into());
    }
    Ok(())
}

fn validate_target(target: &CodingSessionTarget) -> Result<(), String> {
    for (field, value) in [
        ("action.session.driver", &target.driver),
        ("action.session.instanceId", &target.instance_id),
        ("action.session.sessionId", &target.session_id),
    ] {
        validate_required(value, field, MAX_IDENTIFIER_BYTES)?;
    }
    if target.generation == 0 || target.generation > MAX_SAFE_GENERATION {
        return Err("action.session.generation must be a positive safe integer".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "coding_session_hire_requester_tests.rs"]
mod hire_requester_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_session_routing::HireRoutingRequest;
    use serde_json::json;

    /// A syntactically valid project coordinate: 64 lowercase hex, then a `d` tag.
    fn project_coordinate() -> String {
        format!("30621:{}:amas-redux", "cd".repeat(32))
    }

    /// A canonical lowercase umbrella session reference.
    fn session_reference() -> String {
        "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".to_owned()
    }

    fn valid_payload() -> CodingSessionLifecycleCommandPayload {
        CodingSessionLifecycleCommandPayload {
            schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
            command_id: "create-1".into(),
            action: CodingSessionLifecycleAction::SessionCreate {
                project_ref: Some(project_coordinate()),
                repo_ref: Some("30617:owner:amas-redux".into()),
                session_ref: Some(session_reference()),
                genesis_ref: Some("12".repeat(32)),
                provider_instance_ref: "claude-primary".try_into().expect("alias"),
                provider_authority_pubkey: "ab".repeat(32),
                model: Some("claude-sonnet-4-6".into()),
                title: Some("Advance Beekeeper live sessions".into()),
                initial_turn: Some("Start with the highest priority task.".into()),
                actor: None,
                role: None,
                hire_ref: None,
                routing: None,
            },
        }
    }

    /// The historical 8-key v1 action, exactly as pre-amendment signers wrote it.
    fn lifecycle_content(project_ref_json: &str) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":{project_ref_json},"repoRef":null,"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        )
    }

    /// The 9-key action a post-amendment signer writes: `sessionRef` always
    /// present, explicit `null` allowed.
    fn lifecycle_content_with_session_ref(session_ref_json: &str) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"sessionRef":{session_ref_json},"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        )
    }

    /// The authority-aware 10-key action carries both references.
    fn lifecycle_content_with_genesis_ref(
        session_ref_json: &str,
        genesis_ref_json: &str,
    ) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"sessionRef":{session_ref_json},"genesisRef":{genesis_ref_json},"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        )
    }

    #[test]
    fn validates_and_strictly_decodes_the_exact_contract() {
        let payload = valid_payload();
        let content = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            decode_coding_session_lifecycle_command(&content).unwrap(),
            payload
        );

        let nulls = lifecycle_content("null");
        assert!(decode_coding_session_lifecycle_command(&nulls).is_ok());
    }

    /// SV-29: a rewind is the restart's three keys plus `checkpoint` and
    /// `files`, exactly. A missing or extra key, a non-hex checkpoint, and a
    /// `files` outside the closed pair are refused.
    #[test]
    fn coding_session_rewind_decodes_exactly_and_refuses_malformed() {
        let target = CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "instance-1".into(),
            session_id: "session-1".into(),
            generation: 3,
        };
        for files in [RewindFiles::Keep, RewindFiles::Restore] {
            let payload = CodingSessionLifecycleCommandPayload {
                schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
                command_id: "rewind-1".into(),
                action: CodingSessionLifecycleAction::SessionRewind {
                    session: target.clone(),
                    provider_authority_pubkey: "ab".repeat(32),
                    checkpoint: "cd".repeat(32),
                    files,
                },
            };
            let content = serde_json::to_string(&payload).unwrap();
            assert_eq!(
                decode_coding_session_lifecycle_command(&content).unwrap(),
                payload
            );
        }
        let content = |extra: &str, checkpoint: &str, files: &str| {
            format!(
                r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"rewind-1","action":{{"type":"session.rewind","session":{{"driver":"codex-acp","instanceId":"instance-1","sessionId":"session-1","generation":3}},"providerAuthorityPubkey":"{}"{checkpoint}{files}{extra}}}}}"#,
                "ab".repeat(32)
            )
        };
        let good_checkpoint = format!(r#","checkpoint":"{}""#, "cd".repeat(32));
        assert!(decode_coding_session_lifecycle_command(&content(
            "",
            &good_checkpoint,
            r#","files":"keep""#
        ))
        .is_ok());
        for bad in [
            content("", "", r#","files":"keep""#),
            content("", &good_checkpoint, ""),
            content(r#","extra":1"#, &good_checkpoint, r#","files":"keep""#),
            content("", r#","checkpoint":"not-hex""#, r#","files":"keep""#),
            content(
                "",
                &format!(r#","checkpoint":"{}""#, "CD".repeat(32)),
                r#","files":"keep""#,
            ),
            content("", &good_checkpoint, r#","files":"all""#),
            content("", &good_checkpoint, r#","files":null"#),
        ] {
            assert!(
                decode_coding_session_lifecycle_command(&bad).is_err(),
                "accepted {bad}"
            );
        }
    }

    #[test]
    fn resume_and_stop_round_trip_exact_generation_targets() {
        let target = CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "instance-1".into(),
            session_id: "session-1".into(),
            generation: 7,
        };
        for action in [
            CodingSessionLifecycleAction::SessionResume {
                session: target.clone(),
                provider_authority_pubkey: "ab".repeat(32),
            },
            CodingSessionLifecycleAction::SessionRestart {
                session: target.clone(),
                provider_authority_pubkey: "ab".repeat(32),
            },
            CodingSessionLifecycleAction::SessionStop {
                session: target.clone(),
                provider_authority_pubkey: "ab".repeat(32),
            },
        ] {
            let payload = CodingSessionLifecycleCommandPayload {
                schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
                command_id: "lifecycle-1".into(),
                action,
            };
            let content = serde_json::to_string(&payload).unwrap();
            assert_eq!(
                decode_coding_session_lifecycle_command(&content).unwrap(),
                payload
            );
        }

        let smuggled = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"lifecycle-1","action":{{"type":"session.resume","session":{{"driver":"codex-acp","instanceId":"instance-1","sessionId":"session-1","generation":7}},"providerAuthorityPubkey":"{}","cwd":"/tmp"}}}}"#,
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&smuggled).is_err());
    }

    /// Fork amendment: a session need not belong to a project.
    #[test]
    fn accepts_a_standalone_session_with_no_project_ref() {
        let mut payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *project_ref = None;
        assert!(payload.validate().is_ok());

        let content = serde_json::to_string(&payload).unwrap();
        let decoded = decode_coding_session_lifecycle_command(&content).unwrap();
        assert_eq!(decoded, payload);
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &decoded.action
        else {
            panic!("expected create action")
        };
        assert!(project_ref.is_none());
    }

    /// Optional does not mean unvalidated: a present `projectRef` must be a
    /// real NIP-MP coordinate, so a project-bound session can never be
    /// silently downgraded to a standalone one by a malformed reference.
    #[test]
    fn accepts_only_project_kind_30621_coordinates() {
        assert!(validate_project_ref(&project_coordinate()).is_ok());
        // `d` tags may contain colons — split on the first two only.
        assert!(validate_project_ref(&format!("30621:{}:a:b", "cd".repeat(32))).is_ok());

        let owner = "cd".repeat(32);
        for rejected in [
            format!("30178:{owner}:amas-redux"), // donor-era team catalog
            format!("30617:{owner}:amas-redux"), // repository announcement
            format!("30621:{}:amas-redux", "CD".repeat(32)), // uppercase owner
            format!("30621:{}:amas-redux", "cd".repeat(31)), // short owner
            format!("30621:{owner}:"),           // empty d tag
            format!("30621:{owner}"),            // no d tag at all
            "amas-redux".to_string(),            // bare slug
            String::new(),
        ] {
            assert!(
                validate_project_ref(&rejected).is_err(),
                "should reject {rejected:?}"
            );
        }
    }

    /// Fork amendment: the action is exactly the historical 8-key form or
    /// exactly the 9-key form with `sessionRef`, or exactly the 10-key form
    /// with both references. The 8-key form reads as "no umbrella claimed".
    #[test]
    fn accepts_exactly_the_8_key_9_key_and_10_key_action_forms() {
        let historical = lifecycle_content("null");
        let decoded = decode_coding_session_lifecycle_command(&historical).unwrap();
        let CodingSessionLifecycleAction::SessionCreate { session_ref, .. } = &decoded.action
        else {
            panic!("expected create action")
        };
        assert!(
            session_ref.is_none(),
            "the pre-amendment form claims no umbrella"
        );

        let explicit_null = lifecycle_content_with_session_ref("null");
        let decoded = decode_coding_session_lifecycle_command(&explicit_null).unwrap();
        let CodingSessionLifecycleAction::SessionCreate { session_ref, .. } = &decoded.action
        else {
            panic!("expected create action")
        };
        assert!(session_ref.is_none());

        let claimed = lifecycle_content_with_session_ref(&format!("\"{}\"", session_reference()));
        let decoded = decode_coding_session_lifecycle_command(&claimed).unwrap();
        let CodingSessionLifecycleAction::SessionCreate { session_ref, .. } = &decoded.action
        else {
            panic!("expected create action")
        };
        assert_eq!(session_ref.as_deref(), Some(session_reference().as_str()));

        let genesis_ref = "12".repeat(32);
        let linked = lifecycle_content_with_genesis_ref(
            &format!("\"{}\"", session_reference()),
            &format!("\"{genesis_ref}\""),
        );
        let decoded = decode_coding_session_lifecycle_command(&linked).unwrap();
        let CodingSessionLifecycleAction::SessionCreate {
            session_ref,
            genesis_ref: decoded_genesis_ref,
            ..
        } = &decoded.action
        else {
            panic!("expected create action")
        };
        assert_eq!(session_ref.as_deref(), Some(session_reference().as_str()));
        assert_eq!(decoded_genesis_ref.as_deref(), Some(genesis_ref.as_str()));
    }

    #[test]
    fn genesis_ref_is_non_null_canonical_and_requires_session_ref() {
        let session_ref = format!("\"{}\"", session_reference());
        for rejected_genesis in ["null".to_owned(), "\"short\"".to_owned()] {
            assert!(
                decode_coding_session_lifecycle_command(&lifecycle_content_with_genesis_ref(
                    &session_ref,
                    &rejected_genesis
                ))
                .is_err()
            );
        }
        assert!(
            decode_coding_session_lifecycle_command(&lifecycle_content_with_genesis_ref(
                "null",
                &format!("\"{}\"", "12".repeat(32))
            ))
            .is_err()
        );

        let mut payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { session_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *session_ref = None;
        assert!(payload.validate().is_err());
    }

    /// Authority-aware producers write both keys; legacy serializers keep
    /// emitting the 9-key `sessionRef` form when no genesis is named.
    #[test]
    fn new_producers_always_write_the_session_ref_key() {
        let mut payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { session_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *session_ref = None;
        let CodingSessionLifecycleAction::SessionCreate { genesis_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *genesis_ref = None;
        let content = serde_json::to_string(&payload).unwrap();
        assert!(content.contains("\"sessionRef\":null"));
        assert_eq!(
            decode_coding_session_lifecycle_command(&content).unwrap(),
            payload
        );
    }

    /// Optional does not mean unvalidated, and canonical form is the whole
    /// contract: the reference travels in no tag, so nothing downstream ever
    /// normalizes it — a non-canonical spelling would silently split an
    /// umbrella in two.
    #[test]
    fn rejects_a_present_but_malformed_session_ref() {
        assert!(validate_session_ref(&session_reference()).is_ok());

        for rejected in [
            session_reference().to_uppercase(),                // uppercase hex
            session_reference().replace('-', ""),              // no hyphens
            format!("{{{}}}", session_reference()),            // braced form
            format!("urn:uuid:{}", session_reference()),       // URN form
            session_reference()[..35].to_owned(),              // truncated
            format!("{}0", session_reference()),               // too long
            "5b7e1c2a-90d4x4b0e-a1f3-7c2d8e6f4a10".to_owned(), // hyphen misplaced
            "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a1g".to_owned(), // non-hex digit
            " ".repeat(36),                                    // whitespace
            String::new(),
        ] {
            assert!(
                validate_session_ref(&rejected).is_err(),
                "should reject {rejected:?}"
            );
            let content = lifecycle_content_with_session_ref(
                &serde_json::Value::String(rejected.clone()).to_string(),
            );
            assert!(
                decode_coding_session_lifecycle_command(&content).is_err(),
                "decode should reject sessionRef {rejected:?}"
            );
        }

        let mut payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { session_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *session_ref = Some("umbrella".into());
        assert!(payload.validate().is_err());
    }

    /// The three exact forms admit nothing between and nothing beyond.
    #[test]
    fn rejects_action_shapes_between_and_beyond_the_three_forms() {
        // 8 keys, but sessionRef standing in for the required repoRef.
        let swapped = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"sessionRef":null,"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&swapped).is_err());

        // 10 keys, but not the accepted 10-key form: smuggled host path.
        let beyond = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"sessionRef":null,"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null,"cwd":"/tmp"}}}}"#,
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&beyond).is_err());

        // A 9-key form with genesisRef but no sessionRef is never authority-
        // bearing: it is rejected structurally before semantic validation.
        let genesis_without_session = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"genesisRef":"{}","providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "12".repeat(32),
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&genesis_without_session).is_err());

        // 11 keys: the accepted 10-key form plus smuggled content.
        let beyond_authority = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"sessionRef":"{}","genesisRef":"{}","providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null,"cwd":"/tmp"}}}}"#,
            session_reference(),
            "12".repeat(32),
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&beyond_authority).is_err());
    }

    #[test]
    fn rejects_a_present_but_malformed_project_ref() {
        let mut payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *project_ref = Some("project".into());
        assert!(payload.validate().is_err());

        let content = lifecycle_content("\"project\"");
        assert!(decode_coding_session_lifecycle_command(&content).is_err());
    }

    /// `projectRef` stays structurally required even though it is nullable: a
    /// payload that simply omits the key is a truncation, not a standalone
    /// session, and must not be accepted as one.
    #[test]
    fn rejects_missing_unknown_and_duplicate_fields() {
        let missing_nullable_project_ref = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","repoRef":null,"providerInstanceRef":"provider","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        );
        assert!(
            decode_coding_session_lifecycle_command(&missing_nullable_project_ref).is_err(),
            "an omitted projectRef key is a truncated payload, not a standalone session"
        );

        let missing_nullable = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"providerInstanceRef":"provider","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&missing_nullable).is_err());

        let unknown = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"providerInstanceRef":"provider","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null,"cwd":"/tmp"}}}}"#,
            "ab".repeat(32)
        );
        assert!(
            decode_coding_session_lifecycle_command(&unknown).is_err(),
            "a host filesystem path must never ride along in signed content"
        );

        let duplicate = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","commandId":"create-2","action":{{"type":"session.create","projectRef":null,"repoRef":null,"providerInstanceRef":"provider","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&duplicate).is_err());
    }

    #[test]
    fn enforces_required_and_optional_string_semantics() {
        let mut payload = valid_payload();
        payload.command_id = " ".into();
        assert!(payload.validate().is_err());

        payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate {
            provider_authority_pubkey,
            ..
        } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *provider_authority_pubkey = "AB".repeat(32);
        assert!(payload.validate().is_err());

        payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { repo_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *repo_ref = Some("\n".into());
        assert!(payload.validate().is_err());

        payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *project_ref = Some(" ".into());
        assert!(payload.validate().is_err());
    }

    #[test]
    fn utf8_byte_limits_match_the_interoperability_contract() {
        let mut payload = valid_payload();
        match &mut payload.action {
            CodingSessionLifecycleAction::SessionCreate {
                project_ref,
                initial_turn,
                ..
            } => {
                // Pad the `d` segment out to the byte ceiling exactly.
                let prefix = format!("30621:{}:", "cd".repeat(32));
                let padding = MAX_LIFECYCLE_REFERENCE_BYTES - prefix.len();
                *project_ref = Some(format!(
                    "{prefix}{}{}",
                    "é".repeat(padding / 2),
                    "a".repeat(padding % 2)
                ));
                *initial_turn = Some("🐝".repeat(MAX_LIFECYCLE_INITIAL_TURN_BYTES / 4));
            }
            _ => panic!("expected create action"),
        }
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &payload.action
        else {
            panic!("expected create action")
        };
        assert_eq!(
            project_ref.as_deref().map(str::len),
            Some(MAX_LIFECYCLE_REFERENCE_BYTES)
        );
        assert!(payload.validate().is_ok());

        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        project_ref.as_mut().unwrap().push('a');
        assert!(payload.validate().is_err());

        let CodingSessionLifecycleAction::SessionCreate {
            project_ref,
            initial_turn,
            ..
        } = &mut payload.action
        else {
            panic!("expected create action")
        };
        project_ref.as_mut().unwrap().pop();
        initial_turn.as_mut().unwrap().push('a');
        assert!(payload.validate().is_err());
    }

    #[test]
    fn rejects_signed_content_over_16_kib() {
        let content = " ".repeat(MAX_LIFECYCLE_CONTENT_BYTES + 1);
        assert!(decode_coding_session_lifecycle_command(&content).is_err());
    }
    /// A create that seats an agent: the 10-key authority-aware action plus
    /// the `actor`/`role` pair (D1). Written as raw JSON so the shape under
    /// test is the wire's, not this struct's serializer's.
    fn lifecycle_content_with_seat(actor_json: &str, role_json: &str) -> String {
        let mut keys = String::new();
        if actor_json != "<absent>" {
            keys.push_str(&format!(r#","actor":{actor_json}"#));
        }
        if role_json != "<absent>" {
            keys.push_str(&format!(r#","role":{role_json}"#));
        }
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"sessionRef":"{}","genesisRef":"{}","providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}"{keys},"model":null,"title":null,"initialTurn":null}}}}"#,
            session_reference(),
            "12".repeat(32),
            "ab".repeat(32),
        )
    }

    /// D1: a create may seat an agent by naming its pubkey and role, and the
    /// pair decodes back out unchanged.
    #[test]
    fn a_create_may_seat_an_actor_with_a_role() {
        let content = lifecycle_content_with_seat(&format!("\"{}\"", "cd".repeat(32)), "\"lead\"");
        let decoded = decode_coding_session_lifecycle_command(&content).expect("seated create");
        let CodingSessionLifecycleAction::SessionCreate { actor, role, .. } = &decoded.action
        else {
            panic!("expected create action")
        };
        assert_eq!(actor.as_deref(), Some("cd".repeat(32).as_str()));
        assert_eq!(role.as_deref(), Some("lead"));

        // Round-trips: a re-serialized seated create decodes to itself.
        let reserialized = serde_json::to_string(&decoded).expect("serialize");
        assert_eq!(
            decode_coding_session_lifecycle_command(&reserialized).expect("round trip"),
            decoded
        );
    }

    /// The pairing rule, in both directions, with the code the refusal names.
    #[test]
    fn an_actor_without_a_role_is_refused_as_actor_role_pair() {
        for (actor, role) in [
            (format!("\"{}\"", "cd".repeat(32)), "<absent>".to_owned()),
            ("<absent>".to_owned(), "\"lead\"".to_owned()),
            (format!("\"{}\"", "cd".repeat(32)), "null".to_owned()),
            ("null".to_owned(), "\"lead\"".to_owned()),
            ("null".to_owned(), "null".to_owned()),
        ] {
            let error = decode_coding_session_lifecycle_command(&lifecycle_content_with_seat(
                &actor, &role,
            ))
            .expect_err("a lone or null seat key must be refused");
            assert!(
                error.contains(ACTOR_ROLE_PAIR),
                "the refusal must name {ACTOR_ROLE_PAIR}, got {error:?}"
            );
        }
    }

    /// The seat's two values are bounded exactly like every other reference on
    /// this action: a pubkey is canonical lowercase 64-hex, a role is a short
    /// `[a-z0-9-]` slug.
    #[test]
    fn a_seat_pubkey_is_canonical_hex_and_a_role_is_a_bounded_slug() {
        for actor in [
            "cd".repeat(32).to_uppercase(),
            "cd".repeat(31),
            format!("{}g", "cd".repeat(31) + "c"),
            format!("npub1{}", "cd".repeat(20)),
        ] {
            assert!(
                decode_coding_session_lifecycle_command(&lifecycle_content_with_seat(
                    &format!("\"{actor}\""),
                    "\"lead\""
                ))
                .is_err(),
                "actor {actor:?} was accepted"
            );
        }
        for role in [
            String::new(),
            "Lead".to_owned(),
            "lead builder".to_owned(),
            "lead_builder".to_owned(),
            "a".repeat(MAX_ROLE_SLUG_BYTES + 1),
        ] {
            assert!(
                decode_coding_session_lifecycle_command(&lifecycle_content_with_seat(
                    &format!("\"{}\"", "cd".repeat(32)),
                    &format!("\"{role}\"")
                ))
                .is_err(),
                "role {role:?} was accepted"
            );
        }
        for role in [
            "lead",
            "architect",
            "builder-2",
            "a",
            &"a".repeat(MAX_ROLE_SLUG_BYTES),
        ] {
            assert!(
                decode_coding_session_lifecycle_command(&lifecycle_content_with_seat(
                    &format!("\"{}\"", "cd".repeat(32)),
                    &format!("\"{role}\"")
                ))
                .is_ok(),
                "role {role:?} was refused"
            );
        }
    }

    /// A create that seats nobody is byte-for-byte what it was before this
    /// amendment: the two keys are never written as explicit nulls.
    #[test]
    fn an_unseated_create_never_writes_the_seat_keys() {
        let payload = valid_payload();
        let content = serde_json::to_string(&payload).expect("serialize");
        assert!(
            !content.contains("\"actor\"") && !content.contains("\"role\""),
            "an unseated create wrote a seat key: {content}"
        );
        assert!(decode_coding_session_lifecycle_command(&content).is_ok());
    }
    // ── session.hire (plan D14) ───────────────────────────────────────────

    /// The exact seven-key hire action a lead signs.
    fn hire_content(
        session_ref_json: &str,
        genesis_ref_json: &str,
        role_json: &str,
        provider_json: &str,
        model_json: &str,
        brief_json: &str,
    ) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"hire-1","action":{{"type":"session.hire","sessionRef":{session_ref_json},"genesisRef":{genesis_ref_json},"role":{role_json},"providerInstanceRef":{provider_json},"model":{model_json},"brief":{brief_json}}}}}"#
        )
    }

    fn valid_hire_content() -> String {
        hire_content(
            &format!("{:?}", session_reference()),
            &format!("{:?}", "12".repeat(32)),
            "\"builder\"",
            "\"claude-primary\"",
            "\"claude-sonnet-4-6\"",
            "\"Rebase the lane and run the gate.\"",
        )
    }

    /// A hire carries exactly these seven keys, `providerInstanceRef` and
    /// `model` nullable but structurally present, and round-trips byte-exact.
    #[test]
    fn accepts_exactly_the_seven_key_hire_action() {
        let decoded =
            decode_coding_session_lifecycle_command(&valid_hire_content()).expect("a hire decodes");
        let CodingSessionLifecycleAction::SessionHire {
            session_ref,
            genesis_ref,
            role,
            provider_instance_ref,
            model,
            brief,
            requested_by,
            routing,
        } = &decoded.action
        else {
            panic!("expected a hire action")
        };
        assert_eq!(*routing, None);
        assert_eq!(*requested_by, None);
        assert_eq!(session_ref, &session_reference());
        assert_eq!(genesis_ref, &"12".repeat(32));
        assert_eq!(role, "builder");
        assert_eq!(
            provider_instance_ref
                .as_ref()
                .map(ProviderInstanceAlias::as_str),
            Some("claude-primary")
        );
        assert_eq!(model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(brief, "Rebase the lane and run the gate.");
        assert_eq!(
            decoded.hire_session_ref(),
            Some(session_reference().as_str())
        );

        // The nullable pair is written explicitly, never skipped.
        let reserialized = serde_json::to_string(&decoded).expect("serialize");
        let null_pair = hire_content(
            &format!("{:?}", session_reference()),
            &format!("{:?}", "12".repeat(32)),
            "\"builder\"",
            "null",
            "null",
            "\"Rebase the lane and run the gate.\"",
        );
        assert!(decode_coding_session_lifecycle_command(&null_pair).is_ok());
        assert!(
            reserialized.contains(r#""type":"session.hire""#),
            "{reserialized}"
        );
    }

    /// Every other action reports no hire umbrella, so the relay's authority
    /// gate can never mistake a create for a hire.
    #[test]
    fn only_a_hire_names_a_hire_session_ref() {
        assert_eq!(valid_payload().hire_session_ref(), None);
    }

    /// One key more, one key fewer, or a null where a string is required is a
    /// rejection — the same "no partial shape" discipline the create forms use.
    #[test]
    fn refuses_a_hire_with_extra_missing_or_null_required_keys() {
        let session = format!("{:?}", session_reference());
        let genesis = format!("{:?}", "12".repeat(32));
        let brief = "\"Rebase the lane.\"";

        let extra = valid_hire_content().replace(
            r#""brief":"Rebase the lane and run the gate.""#,
            r#""brief":"Rebase the lane and run the gate.","title":"nope""#,
        );
        assert!(
            decode_coding_session_lifecycle_command(&extra).is_err(),
            "an eighth key was accepted"
        );

        let missing = valid_hire_content().replace(r#""model":"claude-sonnet-4-6","#, "");
        assert!(
            decode_coding_session_lifecycle_command(&missing).is_err(),
            "a six-key hire was accepted"
        );

        for (label, content) in [
            (
                "null sessionRef",
                hire_content("null", &genesis, "\"builder\"", "null", "null", brief),
            ),
            (
                "null genesisRef",
                hire_content(&session, "null", "\"builder\"", "null", "null", brief),
            ),
            (
                "null role",
                hire_content(&session, &genesis, "null", "null", "null", brief),
            ),
            (
                "null brief",
                hire_content(&session, &genesis, "\"builder\"", "null", "null", "null"),
            ),
            (
                "empty brief",
                hire_content(&session, &genesis, "\"builder\"", "null", "null", "\"   \""),
            ),
            (
                "uppercase sessionRef",
                hire_content(
                    &format!("{:?}", session_reference().to_uppercase()),
                    &genesis,
                    "\"builder\"",
                    "null",
                    "null",
                    brief,
                ),
            ),
            (
                "short genesisRef",
                hire_content(
                    &session,
                    &format!("{:?}", "12".repeat(31)),
                    "\"builder\"",
                    "null",
                    "null",
                    brief,
                ),
            ),
        ] {
            assert!(
                decode_coding_session_lifecycle_command(&content).is_err(),
                "{label} was accepted"
            );
        }
    }

    /// A role is the same slug the seat contract already validates: no
    /// uppercase, no spaces, no underscores, never empty, never over
    /// [`MAX_ROLE_SLUG_BYTES`].
    #[test]
    fn refuses_a_hire_whose_role_is_not_a_slug() {
        let session = format!("{:?}", session_reference());
        let genesis = format!("{:?}", "12".repeat(32));
        for role in [
            "Builder",
            "build er",
            "build_er",
            "",
            &"b".repeat(MAX_ROLE_SLUG_BYTES + 1),
        ] {
            let content = hire_content(
                &session,
                &genesis,
                &format!("{role:?}"),
                "null",
                "null",
                "\"Rebase the lane.\"",
            );
            assert!(
                decode_coding_session_lifecycle_command(&content).is_err(),
                "role {role:?} was accepted"
            );
        }
        for role in ["lead", "verifier-secondary", "builder2"] {
            let content = hire_content(
                &session,
                &genesis,
                &format!("{role:?}"),
                "null",
                "null",
                "\"Rebase the lane.\"",
            );
            assert!(
                decode_coding_session_lifecycle_command(&content).is_ok(),
                "role {role:?} was refused"
            );
        }
    }

    /// A brief of `bytes` `b`s, decoded.
    fn decode_hire_with_brief(brief: &str) -> Result<CodingSessionLifecycleCommandPayload, String> {
        let session = format!("{:?}", session_reference());
        let genesis = format!("{:?}", "12".repeat(32));
        decode_coding_session_lifecycle_command(&hire_content(
            &session,
            &genesis,
            "\"builder\"",
            "null",
            "null",
            &serde_json::to_string(brief).expect("a JSON string"),
        ))
    }

    /// The brief becomes the seat's first turn *behind the host's prefix*, so
    /// its ceiling is the initial turn's minus that prefix, and one byte past
    /// it is refused by a sentence that names both numbers.
    #[test]
    fn refuses_a_brief_past_the_effective_hire_ceiling() {
        assert_eq!(CODING_SESSION_HIRE_BRIEF_PREFIX.len(), 16);
        assert_eq!(MAX_LIFECYCLE_HIRE_BRIEF_BYTES, 12_272);

        assert!(
            decode_hire_with_brief(&"b".repeat(MAX_LIFECYCLE_HIRE_BRIEF_BYTES)).is_ok(),
            "a brief at the ceiling exactly is accepted"
        );

        let over = MAX_LIFECYCLE_HIRE_BRIEF_BYTES + 1;
        let error = decode_hire_with_brief(&"b".repeat(over))
            .expect_err("one byte past the ceiling must be refused");
        assert_eq!(
            error,
            "coding-session lifecycle command action.brief exceeds 12272 bytes (got 12273): a \
             hire's brief becomes the seat's first turn behind the host's 16-byte \
             \"[From the lead] \" prefix, so its ceiling is the initial-turn ceiling minus that \
             prefix"
        );

        // The window the narrowing closes: briefs the host has always thrown
        // on when it built the create are now refused on the wire.
        for bytes in [
            MAX_LIFECYCLE_HIRE_BRIEF_BYTES + 1,
            MAX_LIFECYCLE_INITIAL_TURN_BYTES,
        ] {
            assert!(
                decode_hire_with_brief(&"b".repeat(bytes)).is_err(),
                "a {bytes}-byte brief names a seat no host can create"
            );
        }
    }

    /// The ceiling counts UTF-8 bytes, not characters: a brief of multi-byte
    /// characters that fits by count and not by length is refused.
    #[test]
    fn a_hire_brief_is_measured_in_bytes_not_characters() {
        // 3,068 four-byte bees = 12,272 bytes exactly.
        let at_ceiling = "🐝".repeat(MAX_LIFECYCLE_HIRE_BRIEF_BYTES / 4);
        assert_eq!(at_ceiling.len(), MAX_LIFECYCLE_HIRE_BRIEF_BYTES);
        assert_eq!(
            at_ceiling.chars().count(),
            MAX_LIFECYCLE_HIRE_BRIEF_BYTES / 4
        );
        assert!(decode_hire_with_brief(&at_ceiling).is_ok());

        let over = format!("{at_ceiling}🐝");
        assert_eq!(over.chars().count(), MAX_LIFECYCLE_HIRE_BRIEF_BYTES / 4 + 1);
        let error = decode_hire_with_brief(&over).expect_err("12,276 bytes is over the ceiling");
        assert!(
            error.contains("action.brief exceeds 12272 bytes (got 12276)"),
            "the refusal must count bytes: {error}"
        );
    }

    /// The narrowing is the hire's alone: a create's `initialTurn` keeps the
    /// full ceiling, which is the number the brief's is derived *from*.
    #[test]
    fn the_hire_narrowing_does_not_leak_into_the_create() {
        let mut payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { initial_turn, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *initial_turn = Some("t".repeat(MAX_LIFECYCLE_INITIAL_TURN_BYTES));
        assert!(
            payload.validate().is_ok(),
            "an initialTurn at 12288 bytes is still accepted"
        );

        let CodingSessionLifecycleAction::SessionCreate { initial_turn, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *initial_turn = Some("t".repeat(MAX_LIFECYCLE_INITIAL_TURN_BYTES + 1));
        assert_eq!(
            payload.validate(),
            Err("action.initialTurn exceeds 12288 bytes".to_owned())
        );
    }

    /// A role whose every installed identity is already seated is a different
    /// refusal from a role this computer has never installed, so it carries a
    /// different code. Until 2026-08-28 both shipped as `HIRE_NO_IDENTITY`
    /// and a lead was told to "install team roles" about a role it already
    /// held (item 88(h), item 89).
    #[test]
    fn a_busy_role_and_an_absent_one_are_different_refusal_codes() {
        assert!(
            HIRE_REFUSAL_CODES.contains(&"HIRE_ROLE_BUSY"),
            "codes: {HIRE_REFUSAL_CODES:?}"
        );
        assert!(
            HIRE_REFUSAL_CODES.contains(&"HIRE_NO_IDENTITY"),
            "codes: {HIRE_REFUSAL_CODES:?}"
        );
        let mut seen = HIRE_REFUSAL_CODES.to_vec();
        seen.sort_unstable();
        let unique = seen.len();
        seen.dedup();
        assert_eq!(unique, seen.len(), "a refusal code is listed twice");
    }

    // ── the routing contract (Brian's ruling of 2026-08-30) ───────────────
    //
    // Two shapes, not one. A hire carries the routing REQUEST; a create and a
    // kind:44223 carry the routing RECORD. Until 2026-08-30 they were one
    // type, `bee sessions hire` emitted the record on a hire, the desktop
    // host accepted only the request — and every routed hire was classified
    // malformed and dropped without a word (ledger draft 97). The tests below
    // are the shape of that fix.

    /// Repository root, for the shared fixtures three implementations pin to.
    fn repo_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
    }

    fn hire_request_fixture() -> Value {
        serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("testdata/routing/hire-request-fixture.json"),
            )
            .expect("read testdata/routing/hire-request-fixture.json"),
        )
        .expect("the hire-request fixture is JSON")
    }

    fn create_record_fixture() -> Value {
        serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("testdata/routing/create-record-fixture.json"),
            )
            .expect("read testdata/routing/create-record-fixture.json"),
        )
        .expect("the create-record fixture is JSON")
    }

    /// The exact eight-key hire action a routed lead signs.
    fn routed_hire_content(routing_json: &str) -> String {
        let session = session_reference();
        let genesis = "12".repeat(32);
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"hire-1","action":{{"type":"session.hire","sessionRef":"{session}","genesisRef":"{genesis}","role":"builder","providerInstanceRef":"claude-primary","model":"sonnet","brief":"Rebase the lane and run the gate.","routing":{routing_json}}}}}"#
        )
    }

    /// The complete record a router fills in, in wire order.
    fn full_routing_json() -> &'static str {
        r#"{"class":"builder","tier":"standard","risk":{"impact":3,"uncertainty":3,"irreversibility":2,"score":18},"profile":null,"chosen":{"provider":"claude-primary","model":"sonnet","effort":"medium"},"runnerUp":null,"reason":"claude-primary/sonnet cleared the builder gate and is the cheapest expected accepted completion","reviewRequired":false,"reviewReasons":[],"challengerSample":false,"override":null,"registryVersion":1,"catalogRevision":7}"#
    }

    /// The smallest honest request: a class and a risk triple, nothing else.
    fn question_only_json() -> &'static str {
        r#"{"class":"builder","risk":{"impact":3,"uncertainty":3,"irreversibility":2}}"#
    }

    /// Every request in the shared fixture decodes on a hire and round-trips
    /// key-for-key. The CLI's emitter and the desktop's parser are pinned to
    /// the same file, so the three agree by construction rather than by
    /// somebody remembering to keep them in step.
    #[test]
    fn every_request_in_the_shared_fixture_is_accepted_on_a_hire() {
        let fixture = hire_request_fixture();
        let requests = fixture["requests"].as_array().expect("requests");
        assert_eq!(requests.len(), 3, "the contract names three requests");
        for case in requests {
            let name = case["name"].as_str().expect("name");
            let routing = &case["routing"];
            let content = routed_hire_content(&routing.to_string());
            let decoded = decode_coding_session_lifecycle_command(&content)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            let CodingSessionLifecycleAction::SessionHire { routing: got, .. } = &decoded.action
            else {
                panic!("{name}: expected a hire")
            };
            let got = got.as_ref().unwrap_or_else(|| panic!("{name}: no routing"));
            // Key-for-key, both directions: an extra key is refused by
            // `deny_unknown_fields` above, and a dropped one shows up here.
            assert_eq!(
                &serde_json::to_value(got).expect("reserialize"),
                routing,
                "{name}"
            );
        }
    }

    /// The keys a request writes, and the order it writes them in. A parser
    /// that reads by position, a diff a human reads — both depend on it.
    #[test]
    fn a_request_writes_its_keys_in_the_documented_order() {
        let fixture = hire_request_fixture();
        let order: Vec<String> = fixture["keyOrder"]
            .as_array()
            .expect("keyOrder")
            .iter()
            .map(|key| key.as_str().expect("key").to_owned())
            .collect();
        // Every optional key at once, so the order is checked over all seven.
        let request = HireRoutingRequest {
            class: "architect".to_owned(),
            risk: crate::coding_session_routing::Risk {
                impact: 5,
                uncertainty: 4,
                irreversibility: 4,
            },
            profile: Some([("taste".to_owned(), 4.5)].into_iter().collect()),
            r#override: Some(crate::coding_session_routing::RoutingOverride {
                model: "gpt-5.6-sol[high]".to_owned(),
                effort: None,
                because: "the lead asked for Sol by name".to_owned(),
            }),
            challenger_sample: true,
            review_flags: vec!["contractChange".to_owned()],
            proposed: None,
        };
        let wire = serde_json::to_string(&request).expect("serialize");
        let mut cursor = 0usize;
        for key in &order {
            if key == "proposed" {
                continue;
            }
            let needle = format!("\"{key}\":");
            let at = wire
                .find(&needle)
                .unwrap_or_else(|| panic!("{key} missing from {wire}"));
            assert!(at >= cursor, "{key} is out of order in {wire}");
            cursor = at;
        }
        // And `risk` carries three keys, never the record's `score`.
        assert!(
            wire.contains(r#""risk":{"impact":5,"uncertainty":4,"irreversibility":4}"#),
            "{wire}"
        );
    }

    /// The 2026-08-30 09:52 drop, as a test. The record on a hire is refused,
    /// and the refusal NAMES the key that does not belong — anything less
    /// leaves a lead re-sending the same request forever.
    #[test]
    fn the_record_on_a_hire_is_refused_by_the_key_that_does_not_belong() {
        let fixture = hire_request_fixture();
        for case in fixture["rejected"].as_array().expect("rejected") {
            let name = case["name"].as_str().expect("name");
            let key = case["offendingKey"].as_str().expect("offendingKey");
            let content = routed_hire_content(&case["routing"].to_string());
            let error = decode_coding_session_lifecycle_command(&content)
                .expect_err(&format!("{name} must be refused"));
            assert!(
                error.contains(key),
                "{name}: the refusal must name {key:?}, said {error:?}"
            );
            // And it must point at the way out, not merely at the mistake.
            assert!(
                error.contains("bee sessions route"),
                "{name}: the refusal must say how to fix it, said {error:?}"
            );
        }
    }

    /// The request shape is refused on a create for the same reason in
    /// reverse: a create that carried only the question would put a decision
    /// on the wire that nobody made.
    #[test]
    fn the_request_on_a_create_is_refused_by_the_key_that_does_not_belong() {
        let fixture = create_record_fixture();
        for case in fixture["rejected"].as_array().expect("rejected") {
            let name = case["name"].as_str().expect("name");
            let key = case["offendingKey"].as_str().expect("offendingKey");
            let content = seated_create_content(&case["routing"].to_string());
            let error = decode_coding_session_lifecycle_command(&content)
                .expect_err(&format!("{name} must be refused"));
            assert!(
                error.contains(key),
                "{name}: the refusal must name {key:?}, said {error:?}"
            );
        }
    }

    /// Every record in the shared fixture decodes on a create and round-trips
    /// key-for-key, including the fourteenth key `proposedDisagreement`.
    #[test]
    fn every_record_in_the_shared_fixture_is_accepted_on_a_create() {
        let fixture = create_record_fixture();
        let records = fixture["records"].as_array().expect("records");
        assert_eq!(records.len(), 3, "the contract names three records");
        let mut disagreements = 0;
        for case in records {
            let name = case["name"].as_str().expect("name");
            let routing = &case["routing"];
            let content = seated_create_content(&routing.to_string());
            let decoded = decode_coding_session_lifecycle_command(&content)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            let CodingSessionLifecycleAction::SessionCreate { routing: got, .. } = &decoded.action
            else {
                panic!("{name}: expected a create")
            };
            let got = got.as_ref().unwrap_or_else(|| panic!("{name}: no routing"));
            assert!(got.is_complete(), "{name}: a create carries the answer");
            if got.proposed_disagreement.is_some() {
                disagreements += 1;
            }
            assert_eq!(
                &serde_json::to_value(got).expect("reserialize"),
                routing,
                "{name}"
            );
        }
        assert_eq!(
            disagreements, 1,
            "one fixture record must disclose a disagreement, or the key is untested"
        );
    }

    /// A hire may carry only the question, and it round-trips byte-exact.
    #[test]
    fn a_hire_may_carry_the_question_alone_and_round_trips_byte_exact() {
        let content = routed_hire_content(question_only_json());
        let decoded = decode_coding_session_lifecycle_command(&content).expect("a routed hire");
        let CodingSessionLifecycleAction::SessionHire { routing, .. } = &decoded.action else {
            panic!("expected a hire")
        };
        let routing = routing.as_ref().expect("a routing request");
        assert_eq!(routing.class, "builder");
        assert_eq!(routing.risk.score(), 18);
        assert_eq!(
            serde_json::to_string(&decoded).expect("reserialize"),
            content
        );
    }

    /// The seven-key hire keeps working: a lead that routes nothing is still
    /// a lead making a request.
    #[test]
    fn the_unrouted_seven_key_hire_is_still_accepted() {
        let session = session_reference();
        let genesis = "12".repeat(32);
        let content = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"hire-1","action":{{"type":"session.hire","sessionRef":"{session}","genesisRef":"{genesis}","role":"builder","providerInstanceRef":null,"model":null,"brief":"Rebase the lane and run the gate."}}}}"#
        );
        let decoded = decode_coding_session_lifecycle_command(&content).expect("an unrouted hire");
        let CodingSessionLifecycleAction::SessionHire { routing, .. } = &decoded.action else {
            panic!("expected a hire")
        };
        assert_eq!(*routing, None);
        // …and `routing` is omitted rather than written as an explicit null,
        // so the bytes an old consumer sees are unchanged.
        assert_eq!(
            serde_json::to_string(&decoded).expect("reserialize"),
            content
        );
    }

    /// The seated, routed create content the record tests decode.
    fn seated_create_content(routing_json: &str) -> String {
        let session = session_reference();
        let genesis = "12".repeat(32);
        let authority = "ab".repeat(32);
        let actor = "cd".repeat(32);
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"sessionRef":"{session}","genesisRef":"{genesis}","providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{authority}","model":"sonnet","title":null,"initialTurn":null,"actor":"{actor}","role":"builder","routing":{routing_json}}}}}"#
        )
    }

    /// A create may carry the record on every historical form, seated or not —
    /// that is how the answer reaches the seat's own row.
    #[test]
    fn a_create_may_carry_a_routing_record_on_every_form() {
        let record: Value = serde_json::from_str(full_routing_json()).expect("routing");
        for base in CREATE_ACTION_FORMS {
            for seated in [false, true] {
                let mut action = serde_json::Map::new();
                for key in *base {
                    let value = match *key {
                        "type" => json!("session.create"),
                        "providerInstanceRef" => json!("claude-primary"),
                        "providerAuthorityPubkey" => json!("ab".repeat(32)),
                        "sessionRef" => json!(session_reference()),
                        "genesisRef" => json!("12".repeat(32)),
                        _ => Value::Null,
                    };
                    action.insert((*key).to_owned(), value);
                }
                if seated {
                    action.insert("actor".into(), json!("cd".repeat(32)));
                    action.insert("role".into(), json!("builder"));
                }
                action.insert("routing".into(), record.clone());
                let content = json!({
                    "schema": CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
                    "commandId": "create-1",
                    "action": Value::Object(action),
                })
                .to_string();
                let decoded =
                    decode_coding_session_lifecycle_command(&content).expect("a routed create");
                let CodingSessionLifecycleAction::SessionCreate { routing, .. } = &decoded.action
                else {
                    panic!("expected a create")
                };
                assert!(routing.as_ref().expect("routing").is_complete());
            }
        }
    }

    /// A create carrying only the question is refused: a create records the
    /// decision, and a null `chosen` there would claim one nobody made.
    #[test]
    fn a_create_carrying_an_incomplete_record_is_refused() {
        let incomplete = full_routing_json().replace(
            r#""chosen":{"provider":"claude-primary","model":"sonnet","effort":"medium"}"#,
            r#""chosen":null"#,
        );
        let error = decode_coding_session_lifecycle_command(&seated_create_content(&incomplete))
            .expect_err("an incomplete record on a create is refused");
        assert!(error.contains("chosen"), "{error}");
    }

    /// The record is validated, not merely carried: an effort the router is
    /// forbidden to purchase is refused at the wire.
    #[test]
    fn a_create_naming_an_effort_above_high_is_refused() {
        let routing = full_routing_json().replace(r#""effort":"medium""#, r#""effort":"xhigh""#);
        let error = decode_coding_session_lifecycle_command(&seated_create_content(&routing))
            .expect_err("xhigh is human override only");
        assert!(error.contains("human override only"), "{error}");
    }

    /// A risk score that does not equal its own factors is a record that
    /// disagrees with itself.
    #[test]
    fn a_routing_record_whose_score_contradicts_its_factors_is_refused() {
        let routing = full_routing_json().replace(r#""score":18"#, r#""score":19"#);
        let error = decode_coding_session_lifecycle_command(&seated_create_content(&routing))
            .expect_err("a self-contradicting score is refused");
        assert!(error.contains("score"), "{error}");
    }

    /// An unknown key inside `routing` is refused rather than dropped: a
    /// consumer that ignored one would compute a different digest from the
    /// signer's.
    #[test]
    fn an_unknown_routing_key_is_refused_rather_than_ignored() {
        let routing = full_routing_json().replace(
            r#""class":"builder""#,
            r#""class":"builder","surprise":true"#,
        );
        assert!(decode_coding_session_lifecycle_command(&seated_create_content(&routing)).is_err());
        let request = question_only_json().replace(
            r#""class":"builder""#,
            r#""class":"builder","surprise":true"#,
        );
        assert!(decode_coding_session_lifecycle_command(&routed_hire_content(&request)).is_err());
    }

    /// A review flag nobody defined is refused by name, never dropped: a
    /// trigger the wire swallows is a review the lead believes it asked for
    /// and did not get.
    #[test]
    fn an_unknown_review_flag_is_refused_by_name() {
        let request = question_only_json().replace(
            r#""irreversibility":2}"#,
            r#""irreversibility":2},"reviewFlags":["looksHard"]"#,
        );
        let error = decode_coding_session_lifecycle_command(&routed_hire_content(&request))
            .expect_err("an unknown review flag is refused");
        assert!(error.contains("looksHard"), "{error}");
    }

    /// `HIRE_NO_ROUTE` is the honest answer when nothing clears the bar: not a
    /// silent fallback to the smartest model, and not a generic malformed.
    #[test]
    fn no_route_is_a_refusal_code_of_its_own() {
        assert!(
            HIRE_REFUSAL_CODES.contains(&"HIRE_NO_ROUTE"),
            "codes: {HIRE_REFUSAL_CODES:?}"
        );
    }

    /// A project session whose host holds no agent of that project for the
    /// role is refused with its own code, never answered by borrowing an agent
    /// of another project that happens to hold the same role name, and never
    /// folded into `HIRE_NO_IDENTITY`, whose remedy ("install team roles")
    /// would not name the project association that is missing.
    #[test]
    fn no_project_agent_is_a_refusal_code_of_its_own() {
        assert!(
            HIRE_REFUSAL_CODES.contains(&"HIRE_NO_PROJECT_AGENT"),
            "codes: {HIRE_REFUSAL_CODES:?}"
        );
    }

    /// `HIRE_MALFORMED` is the answer to the failure that produced this
    /// contract: a hire whose `routing` did not parse used to be dropped in
    /// silence. A request that gets no answer is the bug, not the parse.
    #[test]
    fn malformed_is_a_refusal_code_of_its_own() {
        assert!(
            HIRE_REFUSAL_CODES.contains(&"HIRE_MALFORMED"),
            "codes: {HIRE_REFUSAL_CODES:?}"
        );
    }
}
