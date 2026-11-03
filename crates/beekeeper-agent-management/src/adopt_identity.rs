//! Bring one existing identity under this Host, once.
//!
//! The Host daemon never reads a Keychain (`beekeeper_host::identity`), and that
//! stays true: this runs in a short-lived `bee host manage adopt-identity`, signed
//! by the owner. It takes one key through [`AgentKeySource`], checks what it got,
//! and writes the same custody a Host-minted agent has. It never prints, logs or
//! journals a key, and the source keeps its copy.
//!
//! Three steps stay separate. Custody is here. Setup names the adopted identity in
//! its spec. The route comes from the setup's ordinary start.

use std::collections::BTreeSet;
use std::path::Path;

use beekeeper_core::managed_agent::ManagedAgentEventContent;
use beekeeper_host_core::agent_setup::{
    list_setups, read_adoption, AdoptionRecord, AgentSetupSpec, SetupAccess, ADOPTION_FILE,
};
use beekeeper_host_core::config::HostConfig;
use beekeeper_host_core::{atomic_write, ownership};
use nostr::Keys;
use serde_json::{json, Value};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::management::configuration::current_head;
use crate::management::local::{self, actor_custody, fail};
use crate::role_install::CANONICAL_ROLE_PROMPT;
use crate::{Context, Error, RelayTransport};

/// The exact signed definition an adoption replaced, kept beside custody.
pub(crate) const PRE_ADOPTION_DEFINITION: &str = "pre-adoption-definition.json";

/// Why a key source could not answer. Carries no detail, on purpose: an error
/// from a store that holds secrets is not something to pass along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySourceFailure {
    /// The store refused, or could not be reached.
    Unavailable,
    /// The store answered with something other than the expected shape.
    Malformed,
    /// This platform has no such store.
    Unsupported,
}

/// Where one existing identity's key comes from.
pub trait AgentKeySource {
    /// The key held for exactly this actor, if any. Nothing else may be returned.
    fn agent_key(&self, actor: &str) -> Result<Option<Zeroizing<String>>, KeySourceFailure>;
}

fn refused(code: &str, message: impl std::fmt::Display) -> Error {
    Error::Refused(format!("{code}: {message}"))
}

/// This identity's adoption record, strictly. An unreadable one refuses: it never
/// reads as "not adopted", and never as "no exclusions".
pub(crate) fn adoption(state: &Path, actor: &str) -> Result<Option<AdoptionRecord>, Error> {
    read_adoption(state, actor).map_err(|error| refused("ADOPTION_RECORD_UNREADABLE", error))
}

/// Every path in this custody must be a real file or directory of the state
/// directory's own user: a symlink or a foreign file is never written through.
fn guard_paths(state: &Path, actor: &str) -> Result<(), Error> {
    #[cfg(unix)]
    let expected = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(state).map_err(fail)?.uid()
    };
    let actors = state.join("actors");
    let dir = actors.join(actor);
    for path in [
        actors.clone(),
        dir.clone(),
        dir.join("key"),
        dir.join("auth-tag.json"),
        dir.join(ADOPTION_FILE),
    ] {
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(refused("CUSTODY_UNSAFE", "a custody path is a symlink"))
            }
            Ok(_meta) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if _meta.uid() != expected {
                        return Err(refused(
                            "CUSTODY_UNSAFE",
                            "a custody path belongs to another user",
                        ));
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(refused("CUSTODY_UNSAFE", "a custody path is unreadable")),
        }
    }
    Ok(())
}

/// What custody already holds for this actor. Anything inconsistent refuses, and
/// the original stays.
fn held_key(dir: &Path, actor: &str) -> Result<Option<Zeroizing<String>>, Error> {
    let path = dir.join("key");
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(refused("CUSTODY_CONFLICT", "existing key is unreadable")),
    };
    #[cfg(unix)]
    let broad = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o077 != 0
    };
    #[cfg(not(unix))]
    let broad = false;
    if !meta.is_file() || broad {
        return Err(refused(
            "CUSTODY_CONFLICT",
            "existing key is not an owner-only file",
        ));
    }
    let key = Zeroizing::new(
        std::fs::read_to_string(&path)
            .map_err(|_| refused("CUSTODY_CONFLICT", "existing key is unreadable"))?,
    );
    let keys = Keys::parse(key.trim())
        .map_err(|_| refused("CUSTODY_CONFLICT", "existing key is invalid"))?;
    if keys.public_key().to_hex() != actor {
        return Err(refused(
            "CUSTODY_CONFLICT",
            "existing key belongs to another identity",
        ));
    }
    Ok(Some(key))
}

/// What the attestation says, if there is one.
fn held_attestation(dir: &Path, actor: &str, owner: &str) -> Result<Option<String>, Error> {
    let path = dir.join("auth-tag.json");
    let auth = match std::fs::read_to_string(&path) {
        Ok(auth) => auth,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(refused(
                "CUSTODY_CONFLICT",
                "existing attestation is unreadable",
            ))
        }
    };
    let public = nostr::PublicKey::from_hex(actor).map_err(fail)?;
    let attester = beekeeper_sdk::nip_oa::verify_auth_tag(&auth, &public)
        .map_err(|_| refused("CUSTODY_CONFLICT", "existing attestation is invalid"))?;
    if attester.to_hex() != owner {
        return Err(refused(
            "CUSTODY_CONFLICT",
            "existing attestation names another owner",
        ));
    }
    Ok(Some(auth))
}

/// The custody step, with no relay: safe to repeat at any point of a crash. The
/// source is asked only when no key is held here.
pub(crate) fn establish_custody(
    state: &Path,
    actor: &str,
    owner: &Keys,
    source: &impl AgentKeySource,
) -> Result<bool, Error> {
    crate::validate_hex64(actor)?;
    let owner_hex = owner.public_key().to_hex();
    guard_paths(state, actor)?;
    let dir = state.join("actors").join(actor);
    let held = held_key(&dir, actor)?;
    let attestation = held_attestation(&dir, actor, &owner_hex)?;
    let from_source = held.is_none();
    let key = match held {
        Some(key) => key,
        None => {
            let key = source
                .agent_key(actor)
                .map_err(|failure| match failure {
                    KeySourceFailure::Unsupported => refused(
                        "KEYCHAIN_UNSUPPORTED",
                        "this platform has no such key store",
                    ),
                    KeySourceFailure::Malformed => refused(
                        "KEYCHAIN_MALFORMED",
                        "the key store's entry is not readable",
                    ),
                    KeySourceFailure::Unavailable => refused(
                        "KEYCHAIN_UNAVAILABLE",
                        "the key store refused or could not be reached",
                    ),
                })?
                .ok_or_else(|| {
                    refused(
                        "KEYCHAIN_ENTRY_ABSENT",
                        format!("the key store holds no key for {actor}"),
                    )
                })?;
            let keys = Keys::parse(key.trim())
                .map_err(|_| refused("KEY_IDENTITY_MISMATCH", "the stored key is not a key"))?;
            if keys.public_key().to_hex() != actor {
                return Err(refused(
                    "KEY_IDENTITY_MISMATCH",
                    "the stored key does not derive this identity",
                ));
            }
            key
        }
    };
    let auth = match attestation {
        Some(auth) => auth,
        None => {
            let public = nostr::PublicKey::from_hex(actor).map_err(fail)?;
            beekeeper_sdk::nip_oa::compute_auth_tag(owner, &public, "").map_err(fail)?
        }
    };
    actor_custody(state, actor, Some((key.trim(), &auth)))?;
    Ok(from_source)
}

/// An identity this Host already runs is not adopted: its live work would read as
/// old. Only an interrupted or finished adoption gets past this, so it runs before
/// any key is read.
fn refuse_enrolled(context: &Context, config: &HostConfig, actor: &str) -> Result<(), Error> {
    let state = &config.provider_state_dir;
    if adoption(state, actor)?.is_some() {
        return Ok(());
    }
    let owned = ownership::for_pair(&context.home, config.instance, actor, &config.relay_url)
        .map_err(|_| refused("IDENTITY_ALREADY_ENROLLED", "its ownership cannot be read"))?
        .is_some();
    let (setups, unreadable) = list_setups(state);
    if !unreadable.is_empty() {
        return Err(refused(
            "IDENTITY_ALREADY_ENROLLED",
            "a setup on this Host cannot be read, so this identity may already have one",
        ));
    }
    let in_setup = setups.iter().any(|setup| {
        setup.agent_pubkey.as_deref() == Some(actor)
            || setup.spec.existing_actor.as_deref() == Some(actor)
    });
    let enrolled = state
        .join("actors")
        .join(actor)
        .join("enrollment.json")
        .symlink_metadata()
        .is_ok();
    if owned || in_setup || enrolled {
        return Err(refused(
            "IDENTITY_ALREADY_ENROLLED",
            "this Host already runs this identity, so there is nothing to adopt",
        ));
    }
    Ok(())
}

/// Record, once, which sessions this identity already had. Called with the
/// management lock held. Only a record that does not exist is ever written.
fn write_baseline(state: &Path, actor: &str, owner: &str) -> Result<usize, Error> {
    if let Some(held) = adoption(state, actor)? {
        if held.owner != owner {
            return Err(refused(
                "CUSTODY_CONFLICT",
                "the adoption record names another owner",
            ));
        }
        return Ok(held.prior_sessions.len());
    }
    let prior = sessions_of(state, actor)?;
    atomic_write::atomic_write_json_restricted(
        &state.join("actors").join(actor).join(ADOPTION_FILE),
        &serde_json::to_vec_pretty(&json!({
            "schema": "beekeeper-adopted-identity/v1",
            "actor": actor,
            "owner": owner,
            "priorSessions": prior,
            "adoptedAt": chrono::Utc::now().to_rfc3339(),
        }))
        .map_err(fail)?,
    )
    .map_err(fail)?;
    Ok(prior.len())
}

fn sessions_of(state: &Path, actor: &str) -> Result<BTreeSet<String>, Error> {
    let bytes = match std::fs::read(state.join("state.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(fail(error)),
    };
    let snapshot: Value = serde_json::from_slice(&bytes).map_err(|_| {
        refused(
            "PROVIDER_STATE_UNREADABLE",
            "the provider's state is malformed",
        )
    })?;
    Ok(snapshot["sessions"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, session)| session["actor"] == actor)
        .map(|(id, _)| id.clone())
        .collect())
}

/// An identity on the relay's archived list is not adopted. Only ever refuses, so
/// it asks for no more trust in the list than the relay's own word.
async fn refuse_archived(client: &impl RelayTransport, actor: &str) -> Result<(), Error> {
    let rows = client
        .query_all(json!({"kinds":[beekeeper_core::kind::KIND_IA_ARCHIVED_LIST],"limit":1}))
        .await?;
    let archived = rows.iter().any(|row| {
        row["tags"].as_array().is_some_and(|tags| {
            tags.iter()
                .any(|tag| tag[0] == "p" && tag[1].as_str() == Some(actor))
        })
    });
    if archived {
        return Err(refused("IDENTITY_ARCHIVED", "this identity is archived"));
    }
    Ok(())
}

/// Adopt one identity's custody. Starts nothing, and spends no turn.
pub async fn adopt(
    context: &Context,
    client: &impl RelayTransport,
    actor: &str,
    source: &impl AgentKeySource,
) -> Result<Value, Error> {
    crate::validate_hex64(actor)?;
    let config = crate::hire_source::host(context, client)?;
    let owner = local::owner(client)?;
    let _lock = local::management_lock(context)?;
    current_head(client, actor).await.map_err(|_| {
        refused(
            "IDENTITY_NOT_OWNED",
            "no definition of it is signed by this owner",
        )
    })?;
    refuse_archived(client, actor).await?;
    refuse_enrolled(context, &config, actor)?;
    let state = &config.provider_state_dir;
    let read_store = establish_custody(state, actor, client.keys(), source)?;
    let prior = write_baseline(state, actor, &owner)?;
    Ok(json!({
        "actor": actor,
        "custody": "held",
        "keyStoreRead": read_store,
        "priorSessions": prior,
        "route": "none: setup and its start install it",
    }))
}

/// The actor's own profile must carry this owner's attestation: Stop needs it, and
/// without it an adopted agent could start and then not be stopped.
async fn require_attested_profile(
    client: &impl RelayTransport,
    actor: &str,
    owner: &str,
) -> Result<(), Error> {
    let rows = client
        .query_all(json!({"kinds":[0],"authors":[actor],"limit":1}))
        .await?;
    let profile = rows
        .into_iter()
        .filter_map(|row| serde_json::from_value::<nostr::Event>(row).ok())
        .find(|event| event.pubkey.to_hex() == actor && event.verify().is_ok())
        .ok_or_else(|| refused("PROFILE_UNATTESTED", "this identity has no signed profile"))?;
    let tag = profile
        .tags
        .iter()
        .find(|tag| tag.as_slice().first().is_some_and(|name| name == "auth"))
        .ok_or_else(|| refused("PROFILE_UNATTESTED", "its profile has no owner attestation"))?;
    let tag = serde_json::to_string(tag.as_slice()).map_err(fail)?;
    let attester = beekeeper_sdk::nip_oa::verify_auth_tag(&tag, &profile.pubkey)
        .map_err(|_| refused("PROFILE_UNATTESTED", "its profile attestation is invalid"))?;
    if attester.to_hex() != owner {
        return Err(refused(
            "PROFILE_UNATTESTED",
            "its profile is attested by another owner",
        ));
    }
    Ok(())
}

/// What a setup naming an existing identity requires. Everything that would refuse
/// later is checked here, before any definition changes.
pub(crate) async fn verify(
    context: &Context,
    config: &HostConfig,
    client: &impl RelayTransport,
    owner: &str,
    actor: &str,
    spec: &AgentSetupSpec,
) -> Result<(), Error> {
    crate::validate_hex64(actor)?;
    let state = &config.provider_state_dir;
    guard_paths(state, actor)?;
    let record = adoption(state, actor)?
        .ok_or_else(|| refused("NOT_ADOPTED", "run `bee host manage adopt-identity` first"))?;
    if record.owner != owner {
        return Err(refused(
            "CUSTODY_CONFLICT",
            "the adoption record names another owner",
        ));
    }
    let (_, auth) = actor_custody(state, actor, None)?;
    let public = nostr::PublicKey::from_hex(actor).map_err(fail)?;
    let attester = beekeeper_sdk::nip_oa::verify_auth_tag(&auth, &public).map_err(fail)?;
    if attester.to_hex() != owner {
        return Err(refused(
            "CUSTODY_CONFLICT",
            "custody is attested by another owner",
        ));
    }
    if let Some(held) =
        ownership::for_pair(&context.home, config.instance, actor, &config.relay_url)
            .map_err(fail)?
    {
        if !ownership::is_modern_enrollment(&held) {
            return Err(refused(
                "HISTORICAL_ENROLLMENT",
                "this identity's ownership row is historical",
            ));
        }
    }
    let enrollment = state.join("actors").join(actor).join("enrollment.json");
    if let Ok(bytes) = std::fs::read(&enrollment) {
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| refused("HISTORICAL_ENROLLMENT", "its enrollment is malformed"))?;
        if value.get("nativeContextRequired").and_then(Value::as_bool) == Some(true)
            || value
                .get("nativeImportOperation")
                .is_some_and(|field| !field.is_null())
        {
            return Err(refused(
                "HISTORICAL_ENROLLMENT",
                "its imported enrollment is retired",
            ));
        }
    }
    let head = current_head(client, actor).await?;
    let definition: ManagedAgentEventContent = serde_json::from_str(&head.content).map_err(fail)?;
    let digest = beekeeper_core::project_agent_association::project_agent_digest(&spec.project);
    if definition.name != spec.display_name
        || definition.home_role.as_deref() != Some(spec.role.as_str())
        || !(definition.project_digest == digest || definition.project_withdrawn)
    {
        return Err(refused(
            "DEFINITION_MISMATCH",
            "this identity's definition has another name, role or project",
        ));
    }
    if !matches!(spec.access, SetupAccess::OwnerOnly)
        || definition.respond_to != beekeeper_core::managed_agent::RespondTo::OwnerOnly
    {
        return Err(refused(
            "DEFINITION_MISMATCH",
            "adopting an identity keeps it owner only",
        ));
    }
    require_attested_profile(client, actor, owner).await?;
    refuse_archived(client, actor).await
}

/// What applying this setup will change in the identity's definition, in words, or
/// `None` when it already says this.
pub(crate) async fn definition_change(
    client: &impl RelayTransport,
    actor: &str,
    selection: &str,
) -> Result<Option<String>, Error> {
    let head = current_head(client, actor).await?;
    let definition: ManagedAgentEventContent = serde_json::from_str(&head.content).map_err(fail)?;
    if definition.persona_id.is_none()
        && definition.model.as_deref() == Some(selection)
        && definition.system_prompt.as_deref() == Some(CANONICAL_ROLE_PROMPT)
    {
        return Ok(None);
    }
    Ok(Some(format!(
        "Applying publishes one owner-signed revision over definition {}: persona {} (version {}) \
         gives way to the project's role pack, and the model goes from {} to {selection}. The prior \
         definition is kept on this Host, signed, as {PRE_ADOPTION_DEFINITION}.",
        head.id.to_hex(),
        definition.persona_id.as_deref().unwrap_or("none"),
        definition.persona_source_version.as_deref().unwrap_or("none"),
        definition.model.as_deref().unwrap_or("none"),
    )))
}

/// Keep the exact signed definition being replaced, once. Publishing waits on this.
fn keep_prior_definition(state: &Path, actor: &str, head: &nostr::Event) -> Result<(), Error> {
    guard_paths(state, actor)?;
    let path = state
        .join("actors")
        .join(actor)
        .join(PRE_ADOPTION_DEFINITION);
    match std::fs::symlink_metadata(&path) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err(refused(
                "DEFINITION_NOT_KEPT",
                "the prior definition's copy cannot be read",
            ))
        }
    }
    atomic_write::atomic_write_json_restricted(
        &path,
        &serde_json::to_vec_pretty(head).map_err(fail)?,
    )
    .map_err(|_| {
        refused(
            "DEFINITION_NOT_KEPT",
            "the prior definition could not be kept, so nothing was published",
        )
    })
}

/// Make the definition say what the setup will run, through the existing
/// conditional update, so readback compares something real. A persona link owns
/// prompt and model, and a linked persona from another app cannot be proved here,
/// so the link gives way to the project's role pack: an owner-signed revision, with
/// the earlier ones kept on the relay. Name, role, access and project stay. Repeating
/// is a no-op.
pub(crate) async fn pin_model(
    context: &Context,
    client: &impl RelayTransport,
    actor: &str,
    selection: &str,
    install_operation: Uuid,
) -> Result<(), Error> {
    let head = current_head(client, actor).await?;
    let mut definition: ManagedAgentEventContent =
        serde_json::from_str(&head.content).map_err(fail)?;
    if definition.persona_id.is_none()
        && definition.model.as_deref() == Some(selection)
        && definition.system_prompt.as_deref() == Some(CANONICAL_ROLE_PROMPT)
    {
        return Ok(());
    }
    let config = crate::hire_source::host(context, client)?;
    keep_prior_definition(&config.provider_state_dir, actor, &head)?;
    definition.persona_id = None;
    definition.persona_source_version = None;
    definition.system_prompt = Some(CANONICAL_ROLE_PROMPT.to_string());
    definition.model = Some(selection.to_string());
    crate::update(
        &crate::UpdateRequest {
            operation_id: Uuid::new_v5(&install_operation, b"adopted-definition-model"),
            agent_pubkey: actor.to_string(),
            expected_revision: head.id.to_hex(),
            display_name: None,
            system_prompt: None,
            model: None,
            home: None,
            public_definition: Some(definition),
            private_config: None,
        },
        client,
        context,
    )
    .await
    .map(|_| ())
}

/// What Stop may act on. It finds an agent's work across the relay, which for an
/// adopted identity can include other providers' and pre-adoption sessions. Only a
/// session this Host's provider holds for this actor, from after the adoption, is
/// this Host's to stop. An identity never adopted here is unchanged. Returns the rows
/// and how many were left alone. Unreadable state refuses: nothing is stopped on a
/// guess.
pub(crate) fn stoppable(
    context: &Context,
    actor: &str,
    rows: Vec<crate::lifecycle::Responsibility>,
) -> Result<(Vec<crate::lifecycle::Responsibility>, usize), Error> {
    let config = HostConfig::load(&beekeeper_host_core::layout::host_config_path(
        &context.home,
        context.instance,
    ))
    .map_err(|_| {
        refused(
            "ADOPTION_RECORD_UNREADABLE",
            "this Host's configuration cannot be read",
        )
    })?;
    let Some(config) = config else {
        return Ok((rows, 0));
    };
    let state = &config.provider_state_dir;
    let Some(record) = adoption(state, actor)? else {
        return Ok((rows, 0));
    };
    let current = sessions_of(state, actor).map_err(|_| {
        refused(
            "ADOPTION_RECORD_UNREADABLE",
            "this Host's provider state cannot be read",
        )
    })?;
    let total = rows.len();
    let kept: Vec<_> = rows
        .into_iter()
        .filter(|row| is_stoppable(&row.target.session_id, &current, &record.prior_sessions))
        .collect();
    let excluded = total - kept.len();
    Ok((kept, excluded))
}

/// A Start that aged out found the identity gone from its channel. The setup keeps
/// its identity, so the way forward is the owner's: re-add it, then Retry.
pub(crate) fn start_membership_unknown() -> Error {
    refused(
        "MEMBERSHIP_UNKNOWN",
        "this identity is not in the channel, and the owner may have removed it, so it was not \
         added again. If it should be there, add it with `bee channels add-member`, then Retry",
    )
}

/// Where a read of this identity's own start looks. A bad record refuses.
pub(crate) fn discovery_for(
    context: &Context,
    actor: &str,
) -> Result<crate::lifecycle::Discovery, Error> {
    use crate::lifecycle::Discovery;
    let config = HostConfig::load(&beekeeper_host_core::layout::host_config_path(
        &context.home,
        context.instance,
    ))
    .map_err(|_| {
        refused(
            "ADOPTION_RECORD_UNREADABLE",
            "this Host's configuration cannot be read",
        )
    })?;
    let adopted = match config {
        Some(config) => adoption(&config.provider_state_dir, actor)?.is_some(),
        None => false,
    };
    Ok(if adopted {
        Discovery::NamedChannels
    } else {
        Discovery::Membership
    })
}

/// An allowlist: held here for this actor, and not from before the adoption.
fn is_stoppable(session: &str, current: &BTreeSet<String>, prior: &BTreeSet<String>) -> bool {
    current.contains(session) && !prior.contains(session)
}

/// A journaled install that has aged out is never re-signed for an existing
/// identity: the owner may have changed its definition or removed it since. Only if
/// both still read as the journal left them may a new attempt go ahead. What cannot
/// be told apart stays unknown.
pub(crate) fn may_retry_install(
    journaled_head: Option<&str>,
    current_head: &str,
    member_now: bool,
    journal_added_member: bool,
) -> Result<(), Error> {
    if journaled_head.is_some_and(|head| head != current_head) {
        return Err(refused(
            "OWNER_CHANGED_DEFINITION",
            "the definition has changed since this install was signed, so it was not replaced. \
             Cancel this setup and plan again",
        ));
    }
    if journal_added_member && !member_now {
        return Err(refused(
            "MEMBERSHIP_UNKNOWN",
            "this identity is not in the channel: that add may never have landed, or the owner \
             may have removed it, so it was not added again. Cancel this setup and plan again",
        ));
    }
    Ok(())
}

/// After a confirmed start, record an adopted identity's ownership, once.
pub(crate) fn record_first_start(
    home: &Path,
    config: &HostConfig,
    operation: &local::Operation,
    operation_id: Uuid,
    target: &beekeeper_core::coding_session_command::CodingSessionTarget,
) -> Result<(), Error> {
    if adoption(&config.provider_state_dir, &operation.actor_pubkey)?.is_none() {
        return Ok(());
    }
    let held = ownership::for_pair(
        home,
        config.instance,
        &operation.actor_pubkey,
        &config.relay_url,
    )
    .map_err(fail)?;
    if held.is_some() {
        return Ok(());
    }
    let revision = operation
        .events
        .first()
        .ok_or_else(|| fail("confirmed start has no definition"))?
        .id
        .to_hex();
    ownership::record_adopted_enrollment(
        home,
        config.instance,
        &operation.actor_pubkey,
        &config.relay_url,
        &operation_id.to_string(),
        &revision,
        target.clone(),
    )
    .map_err(fail)
}

#[cfg(all(test, unix))]
#[path = "adopt_identity_tests.rs"]
mod tests;
