//! Desktop signing adapter for a protected private NIP-AP snapshot, never publication.
use super::super::*;
use crate::managed_agents::persona_events::{
    build_persona_event, persona_content_hash, persona_d_tag, persona_event_content,
};
use crate::managed_agents::retention::{
    active_retention_scope, get_retained_event, open_retention_db,
};
use beekeeper_agent_management::PrivateConfiguration;
use beekeeper_host_core::atomic_write;
use nostr::Event;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn private_event_matches(event: &Event, owner: nostr::PublicKey, id: &str, content: &str) -> bool {
    event.pubkey == owner
        && event.kind.as_u16() == 30175
        && event.verify().is_ok()
        && event.content == content
        && {
            let mut coordinates = event
                .tags
                .iter()
                .filter(|tag| tag.as_slice().first().is_some_and(|key| key == "d"));
            coordinates
                .next()
                .is_some_and(|tag| tag.as_slice() == ["d", id])
                && coordinates.next().is_none()
        }
        && !event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["shared", "true"])
}
struct PinAuthority<'a> {
    actor: &'a str,
    relay: &'a str,
    id: &'a str,
    coordinate: &'a str,
    source_hash: &'a str,
    owner: nostr::PublicKey,
    content: &'a str,
}
fn read_cached_pin(path: &std::path::Path, authority: &PinAuthority<'_>) -> Result<Value, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.file_type().is_file() || meta.len() > 1024 * 1024 {
        return Err("private persona pin custody invalid".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("private persona pin permissions unsafe".into());
        }
    }
    let value: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let event: Event = serde_json::from_value(
        value
            .get("event")
            .cloned()
            .ok_or("private persona pin event absent")?,
    )
    .map_err(|e| e.to_string())?;
    if value.get("schema").and_then(Value::as_str) != Some("beekeeper-private-persona/v1")
        || value.get("actorPubkey").and_then(Value::as_str) != Some(authority.actor)
        || value.get("relayUrl").and_then(Value::as_str) != Some(authority.relay)
        || value.get("personaId").and_then(Value::as_str) != Some(authority.id)
        || value.get("sourceHash").and_then(Value::as_str) != Some(authority.source_hash)
        || value
            .get("personaCoordinate")
            .and_then(Value::as_str)
            .unwrap_or(authority.id)
            != authority.coordinate
        || !private_event_matches(
            &event,
            authority.owner,
            authority.coordinate,
            authority.content,
        )
    {
        return Err("private persona pin conflicts with current source".into());
    }
    Ok(value)
}
/// Attach only after the original source/configuration CAS has succeeded.
/// Retry reuses exact locally retained/cached signed bytes; the shared writer binds30177.
pub(crate) fn attach_private_persona(
    app: &AppHandle,
    state: &AppState,
    record: &ManagedAgentRecord,
    relay: &str,
    private: &mut PrivateConfiguration,
) -> Result<(), String> {
    let Some(id) = record.persona_id.as_deref() else {
        return Ok(());
    };
    let personas = load_personas(app)?;
    let mut matches = personas.iter().filter(|persona| persona.id == id);
    let persona = matches
        .next()
        .ok_or("private persona source unavailable; repair local definition")?;
    if matches.next().is_some() {
        return Err("private persona source is ambiguous".into());
    }
    if persona.shared {
        return Ok(());
    }
    let scope = active_retention_scope(app, state)?;
    let canonical = beekeeper_host_core::record::canonical_relay_key(relay);
    if beekeeper_host_core::record::canonical_relay_key(&scope.relay_url) != canonical {
        return Err("community changed before private persona snapshot".into());
    }
    let actor =
        nostr::PublicKey::from_hex(&record.pubkey).map_err(|_| "private persona actor invalid")?;
    let owner = beekeeper_sdk_pkg::nip_oa::verify_auth_tag(
        record
            .auth_tag
            .as_deref()
            .ok_or("actor custody attestation unavailable")?,
        &actor,
    )
    .map_err(|_| "private persona actor attestation invalid")?;
    if owner != scope.owner_keys.public_key() {
        return Err("private persona signing owner differs from actor custody".into());
    }
    let d = persona_d_tag(persona);
    let projection = persona_event_content(persona);
    let content = serde_json::to_string(&projection).map_err(|e| e.to_string())?;
    let hash = persona_content_hash(&projection);
    // Internal IDs (for example builtin:fizz) are not NIP-AP coordinates.
    // A mapped coordinate is authorized only by an affirmative pinned hash.
    if (d != id && record.persona_source_version.as_deref() != Some(hash.as_str()))
        || record
            .persona_source_version
            .as_deref()
            .is_some_and(|source| source != hash)
    {
        return Err("private persona differs from the pinned source; reconcile the definition explicitly before updating execution".into());
    }
    // A content-addressed protected pin is immutable across operation retries.
    // It does not modify original importer stages or public retention/publication.
    let key = hex::encode(Sha256::digest(format!(
        "{}\0{}\0{}\0{}\0{}",
        owner.to_hex(),
        canonical,
        record.pubkey,
        id,
        hash
    )));
    let directory =
        crate::managed_agents::managed_agents_base_dir(app)?.join("private-persona-pins");
    atomic_write::create_dir_all_restricted(&directory)?;
    let path = directory.join(format!("{key}.json"));
    let ctx = super::super::context()?;
    let _lock = beekeeper_host_core::ownership::management_lock(&ctx.home, ctx.instance)?;
    let snapshot = match std::fs::symlink_metadata(&path) {
        Ok(_) => read_cached_pin(
            &path,
            &PinAuthority {
                actor: &record.pubkey,
                relay: &canonical,
                id,
                coordinate: &d,
                source_hash: &hash,
                owner,
                content: &content,
            },
        )?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let conn = open_retention_db(&scope.db_path)?;
            let retained = get_retained_event(&conn, 30175, &owner.to_hex(), &d)?
                .and_then(|row| serde_json::from_str::<Event>(&row.raw_event).ok())
                .filter(|event| private_event_matches(event, owner, &d, &content));
            let event = match retained {
                Some(event) => event,
                None => build_persona_event(persona)?
                    .sign_with_keys(&scope.owner_keys)
                    .map_err(|e| e.to_string())?,
            };
            let value = json!({"schema":"beekeeper-private-persona/v1","actorPubkey":record.pubkey,"relayUrl":canonical,"personaId":id,"personaCoordinate":d,"sourceHash":hash,"event":event});
            atomic_write::atomic_write_json_restricted(
                &path,
                &serde_json::to_vec(&value).map_err(|e| e.to_string())?,
            )?;
            value
        }
        Err(error) => return Err(error.to_string()),
    };
    private.record["private_persona"] = snapshot;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_signer_projection_rejects_shared_wrong_owner_or_changed_content() {
        let keys = nostr::Keys::generate();
        let other = nostr::Keys::generate();
        let make = |shared| {
            let mut tags = vec![nostr::Tag::parse(["d", "private"]).unwrap()];
            if shared {
                tags.push(nostr::Tag::parse(["shared", "true"]).unwrap());
            }
            nostr::EventBuilder::new(nostr::Kind::Custom(30175), "private instructions")
                .tags(tags)
                .sign_with_keys(&keys)
                .unwrap()
        };
        let event = make(false);
        assert!(private_event_matches(
            &event,
            keys.public_key(),
            "private",
            "private instructions"
        ));
        assert!(!private_event_matches(
            &event,
            other.public_key(),
            "private",
            "private instructions"
        ));
        assert!(!private_event_matches(
            &event,
            keys.public_key(),
            "private",
            "changed instructions"
        ));
        assert!(!private_event_matches(
            &make(true),
            keys.public_key(),
            "private",
            "private instructions"
        ));
    }
    #[test]
    fn immutable_private_pin_recovery_reuses_exact_signed_bytes_and_rejects_source_drift() {
        let keys = nostr::Keys::generate();
        let event = nostr::EventBuilder::new(nostr::Kind::Custom(30175), "private content")
            .tags([nostr::Tag::parse(["d", "private"]).unwrap()])
            .sign_with_keys(&keys)
            .unwrap();
        let value = json!({"schema":"beekeeper-private-persona/v1","actorPubkey":"actor","relayUrl":"wss://fixture.example","personaId":"private","sourceHash":"hash-a","event":event});
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pin.json");
        atomic_write::atomic_write_json_restricted(&path, &serde_json::to_vec(&value).unwrap())
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let authority = PinAuthority {
            actor: "actor",
            relay: "wss://fixture.example",
            id: "private",
            coordinate: "private",
            source_hash: "hash-a",
            owner: keys.public_key(),
            content: "private content",
        };
        let first = read_cached_pin(&path, &authority).unwrap();
        let recovered = read_cached_pin(&path, &authority).unwrap();
        assert_eq!(first, recovered);
        assert_eq!(first["event"]["id"], json!(event.id.to_hex()));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let changed = PinAuthority {
            source_hash: "hash-b",
            ..authority
        };
        assert!(read_cached_pin(&path, &changed).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[cfg(unix)]
    #[test]
    fn cached_private_pin_rejects_unsafe_permissions_and_symlink_custody() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pin.json");
        std::fs::write(&path, b"{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let keys = nostr::Keys::generate();
        let scope = PinAuthority {
            actor: "actor",
            relay: "wss://fixture.example",
            id: "private",
            coordinate: "private",
            source_hash: "hash",
            owner: keys.public_key(),
            content: "private content",
        };
        assert!(read_cached_pin(&path, &scope).is_err());
        let link = dir.path().join("link.json");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(read_cached_pin(&link, &scope).is_err());
    }
    #[test]
    fn mapped_private_pin_keeps_internal_id_and_refuses_coordinate_drift_on_retry() {
        let keys = nostr::Keys::generate();
        for (id, coordinate) in [
            ("builtin:fizz", "builtin-fizz"),
            ("definition-uuid", "reviewer"),
        ] {
            let event = nostr::EventBuilder::new(nostr::Kind::Custom(30175), "private content")
                .tags([nostr::Tag::parse(["d", coordinate]).unwrap()])
                .sign_with_keys(&keys)
                .unwrap();
            let value = json!({"schema":"beekeeper-private-persona/v1", "actorPubkey":"actor", "relayUrl":"wss://fixture.example", "personaId":id, "personaCoordinate":coordinate, "sourceHash":"hash-a", "event":event});
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("pin.json");
            atomic_write::atomic_write_json_restricted(&path, &serde_json::to_vec(&value).unwrap())
                .unwrap();
            let before = std::fs::read(&path).unwrap();
            let authority = PinAuthority {
                actor: "actor",
                relay: "wss://fixture.example",
                id,
                coordinate,
                source_hash: "hash-a",
                owner: keys.public_key(),
                content: "private content",
            };
            assert_eq!(read_cached_pin(&path, &authority).unwrap(), value);
            assert_eq!(read_cached_pin(&path, &authority).unwrap(), value);
            let changed = PinAuthority {
                coordinate: "different",
                ..authority
            };
            assert!(read_cached_pin(&path, &changed).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }
}
