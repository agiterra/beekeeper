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
//! The donor contract required every session to name a Buzz project. Here a
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
use crate::coding_session_payload::ACTOR_ROLE_PAIR;
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
    /// Create a new provider session, optionally bound to a Buzz project.
    #[serde(rename = "session.create")]
    SessionCreate {
        /// Optional Buzz project reference (`30621:<owner>:<d>`).
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
        /// Required capability-advertised provider instance reference.
        provider_instance_ref: String,
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
        /// Provider instance the host should run the seat on, or `null` to
        /// take the host policy's default. Written explicitly either way.
        provider_instance_ref: Option<String>,
        /// Model the host should run the seat on, or `null` to take the
        /// chosen identity's own. Written explicitly either way.
        model: Option<String>,
        /// The brief. It becomes the seat's first turn verbatim (the host
        /// prefixes it), so it is required and non-empty:
        /// 1..=[`MAX_LIFECYCLE_INITIAL_TURN_BYTES`] bytes.
        brief: String,
    },
    /// Reattach a disconnected, non-stopped execution as a new generation.
    #[serde(rename = "session.resume")]
    SessionResume {
        /// Exact previous generation being resumed.
        session: CodingSessionTarget,
        /// Signing pubkey of the provider authority that owns the target.
        provider_authority_pubkey: String,
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
                    provider_instance_ref,
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
            }
            CodingSessionLifecycleAction::SessionHire {
                session_ref,
                genesis_ref,
                role,
                provider_instance_ref,
                model,
                brief,
            } => {
                validate_session_ref(session_ref)?;
                validate_event_id_hex("action.genesisRef", genesis_ref)?;
                validate_role_slug(role)?;
                validate_optional(
                    provider_instance_ref,
                    "action.providerInstanceRef",
                    MAX_LIFECYCLE_REFERENCE_BYTES,
                )?;
                validate_optional(model, "action.model", MAX_LIFECYCLE_REFERENCE_BYTES)?;
                validate_required(brief, "action.brief", MAX_LIFECYCLE_INITIAL_TURN_BYTES)?;
            }
            CodingSessionLifecycleAction::SessionResume {
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
            let seated = action.get("actor").is_some();
            let mut forms: Vec<Vec<&str>> = Vec::with_capacity(6);
            for base in CREATE_ACTION_FORMS {
                let mut seated_form = base.to_vec();
                seated_form.push("actor");
                seated_form.push("role");
                forms.push(base.to_vec());
                forms.push(seated_form);
            }
            let forms: Vec<&[&str]> = forms
                .iter()
                .filter(|form| form.contains(&"actor") == seated)
                .map(Vec::as_slice)
                .collect();
            require_exact_field_forms(action, &forms, "action")?;
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
        Some("session.hire") => require_exact_fields(action, HIRE_ACTION_FORM, "action")?,
        Some("session.resume" | "session.stop") => require_exact_fields(
            action,
            &["type", "session", "providerAuthorityPubkey"],
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

/// The one accepted `session.hire` key set. Unlike the create, a hire has no
/// historical forms to keep valid — it is new with the relay that validates
/// it — so exactly these seven keys are accepted, nothing else.
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
/// act on it rather than retry blindly. `HIRE_NO_IDENTITY` is the one whose
/// remedy is the operator's ("install team roles"); `HIRE_MODEL_NOT_OFFERED`
/// and `HIRE_STALE` are facts about the request the lead can fix by itself;
/// the rest are policy.
pub const HIRE_REFUSAL_CODES: &[&str] = &[
    // Hiring is switched off for this host.
    "HIRE_OFF",
    // The role is not on the host's allowed-roles list.
    "HIRE_ROLE_NOT_ALLOWED",
    // The umbrella already holds the host's maximum live seats.
    "HIRE_LIMIT",
    // No installed managed agent holds this home role and is free.
    "HIRE_NO_IDENTITY",
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
mod tests {
    use super::*;

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
                provider_instance_ref: "claude-primary".into(),
                provider_authority_pubkey: "ab".repeat(32),
                model: Some("claude-sonnet-4-6".into()),
                title: Some("Advance Buzz live sessions".into()),
                initial_turn: Some("Start with the highest priority task.".into()),
                actor: None,
                role: None,
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
        } = &decoded.action
        else {
            panic!("expected a hire action")
        };
        assert_eq!(session_ref, &session_reference());
        assert_eq!(genesis_ref, &"12".repeat(32));
        assert_eq!(role, "builder");
        assert_eq!(provider_instance_ref.as_deref(), Some("claude-primary"));
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

    /// The brief is the seat's whole first turn, so it takes the same ceiling
    /// an `initialTurn` does and one byte past it is refused.
    #[test]
    fn refuses_a_brief_past_the_initial_turn_ceiling() {
        let session = format!("{:?}", session_reference());
        let genesis = format!("{:?}", "12".repeat(32));
        for (bytes, expected_ok) in [
            (MAX_LIFECYCLE_INITIAL_TURN_BYTES, true),
            (MAX_LIFECYCLE_INITIAL_TURN_BYTES + 1, false),
        ] {
            let content = hire_content(
                &session,
                &genesis,
                "\"builder\"",
                "null",
                "null",
                &format!("{:?}", "b".repeat(bytes)),
            );
            assert_eq!(
                decode_coding_session_lifecycle_command(&content).is_ok(),
                expected_ok,
                "a {bytes}-byte brief"
            );
        }
    }
}
