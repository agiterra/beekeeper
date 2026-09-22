//! Point the built-in personas at the avatars this build ships.
//!
//! Split out of `migration.rs` — every other migration in that list already
//! lives in its own module (`fold`, `backfill`, `pollen`, `team_suffix`), and
//! this was the last one still inline in a file long past the repository's
//! 1000-line ceiling.
//!
//! It rewrites a stored avatar only when the one on the record is a *known
//! legacy* blob, matched by hash rather than by URL shape, so an avatar a
//! person chose is never replaced by a shipped default.

use std::path::Path;

use sha2::{Digest, Sha256};
use tauri::Manager;

pub(super) struct LegacyBuiltInAvatar<'a> {
    pub(super) persona_id: &'a str,
    pub(super) data_url_sha256: &'a str,
    // Catalog-created agents persist the blob descriptor for the PNG after
    // `sanitize_image_for_upload` re-encodes it, not the decoded source bytes.
    pub(super) sanitized_media_sha256: &'a str,
    pub(super) persona_content_hash: &'a str,
}

pub(super) const LEGACY_BUILTIN_AVATARS: &[LegacyBuiltInAvatar<'static>] = &[
    LegacyBuiltInAvatar {
        persona_id: "builtin:fizz",
        data_url_sha256: "2771b8c9c46aa3c8ac1c4d2acfa23fa9ba35b79c4b1694554e923081e3b8b4d0",
        sanitized_media_sha256: "1a4964ff4cf6c499df1a77a941c211c7d1e7ef755f1c395bc9a3b0f2878114a6",
        persona_content_hash: "b36381d042c8eb5c786a1a692c7ba5a47ae129b9972a1473b64d8fe03f4817c1",
    },
    LegacyBuiltInAvatar {
        persona_id: "builtin:honey",
        data_url_sha256: "1979e54ef77fc94ec688170bd74dade35c563e7fcc82bb0714c672dfb018eab9",
        sanitized_media_sha256: "0e0ed9a35d4050bdd290aa8138d5ab811f222549f6acc3cee40a7feb65933e1f",
        persona_content_hash: "9c9b6b11f1cdd56ba645de02213c562e59c3690bf3f217f74a85df8e6575fd06",
    },
    LegacyBuiltInAvatar {
        persona_id: "builtin:bumble",
        data_url_sha256: "c08cf3b8b4c3f8721df6143367ababdebae8f913b9c654401ba74bb3d233655b",
        sanitized_media_sha256: "9f798c61f8965b80beb808f505feb0a5b33726545188ea8212cc9ab22d05f0b6",
        persona_content_hash: "544a73f9106a3c8848b0f308b7a8b6f95077ac8deccdb9ed5552caa833d66c95",
    },
];

struct LegacyAvatarMatch<'a> {
    persona_id: String,
    metadata: &'a LegacyBuiltInAvatar<'a>,
    was_uploaded: bool,
}

/// Refresh the prior seeded avatar on built-in definitions and linked agent
/// instances while preserving any avatar the user customized. Matching by the
/// exact data URL or content-addressed upload digest makes the migration
/// idempotent and avoids relying on timestamps or other persona fields the
/// user may also have edited.
pub(super) fn refresh_builtin_agent_avatars(app: &tauri::AppHandle) {
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let path = dir.join("agents/managed-agents.json");
    if path.exists() {
        refresh_builtin_agent_avatars_in_file(
            &path,
            LEGACY_BUILTIN_AVATARS,
            &crate::util::now_iso(),
        );
    }
}

pub(super) fn refresh_builtin_agent_avatars_in_file(
    path: &Path,
    legacy_avatars: &[LegacyBuiltInAvatar<'_>],
    now: &str,
) {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(mut records) = serde_json::from_str::<Vec<serde_json::Value>>(&contents) else {
        eprintln!(
            "buzz-desktop: refresh-builtin-agent-avatars: invalid JSON in {}",
            path.display()
        );
        return;
    };

    // Definitions must be migrated first so linked instances can advance from
    // the exact old persona hash to the exact new one. Only advance an instance
    // that was in sync before migration; a genuinely drifted instance keeps its
    // old source version and therefore keeps its out-of-date indicator.
    let mut persona_version_updates = std::collections::HashMap::new();
    let mut changed = false;
    for record in &mut records {
        let Some(legacy_match) = legacy_avatar_match(record, legacy_avatars) else {
            continue;
        };
        let persona_id = legacy_match.persona_id.clone();
        let is_definition = record
            .get("pubkey")
            .and_then(serde_json::Value::as_str)
            .is_some_and(str::is_empty)
            && record.get("slug").and_then(serde_json::Value::as_str) == Some(&persona_id);
        if !is_definition {
            continue;
        }
        let old_version = persona_version_from_record(record);
        if !replace_builtin_avatar(record, &persona_id, now) {
            continue;
        }
        changed = true;
        if let (Some(old_version), Some(new_version)) =
            (old_version, persona_version_from_record(record))
        {
            persona_version_updates.insert(persona_id, (old_version, new_version));
        }
    }

    for record in &mut records {
        let Some(legacy_match) = legacy_avatar_match(record, legacy_avatars) else {
            continue;
        };
        let persona_id = legacy_match.persona_id.clone();
        let legacy_avatar = record
            .get("avatar_url")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let is_linked_instance = record
            .get("pubkey")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|pubkey| !pubkey.is_empty())
            && record.get("persona_id").and_then(serde_json::Value::as_str) == Some(&persona_id);
        if !is_linked_instance {
            continue;
        }
        let version_update = persona_version_updates
            .get(&persona_id)
            .cloned()
            .or_else(|| {
                legacy_avatar.as_deref().and_then(|legacy_avatar| {
                    built_in_persona_version_update(&legacy_match, legacy_avatar, now)
                })
            });
        if !replace_builtin_avatar(record, &persona_id, now) {
            continue;
        }
        changed = true;
        if let Some((old_version, new_version)) = version_update {
            let source_was_current = record
                .get("persona_source_version")
                .and_then(serde_json::Value::as_str)
                == Some(old_version.as_str());
            if source_was_current {
                record["persona_source_version"] = serde_json::Value::String(new_version);
            }
        }
    }

    if changed {
        if let Ok(bytes) = serde_json::to_vec_pretty(&records) {
            if let Err(e) = crate::managed_agents::atomic_write_json_restricted(path, &bytes) {
                eprintln!("buzz-desktop: refresh-builtin-agent-avatars: {e}");
            }
        }
    }
}

fn legacy_avatar_match<'a>(
    record: &serde_json::Value,
    legacy_avatars: &'a [LegacyBuiltInAvatar<'a>],
) -> Option<LegacyAvatarMatch<'a>> {
    let persona_id = record
        .get("persona_id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| record.get("slug").and_then(serde_json::Value::as_str))?;
    let metadata = legacy_avatars
        .iter()
        .find(|legacy| legacy.persona_id == persona_id)?;
    let current_avatar = record
        .get("avatar_url")
        .and_then(serde_json::Value::as_str)?;
    let matches_data_url =
        hex::encode(Sha256::digest(current_avatar.as_bytes())) == metadata.data_url_sha256;
    let matches_uploaded_media = uploaded_media_sha256(current_avatar)
        .is_some_and(|sha256| sha256 == metadata.sanitized_media_sha256);
    (matches_data_url || matches_uploaded_media).then(|| LegacyAvatarMatch {
        persona_id: persona_id.to_string(),
        metadata,
        was_uploaded: matches_uploaded_media,
    })
}

fn uploaded_media_sha256(avatar_url: &str) -> Option<String> {
    let url = url::Url::parse(avatar_url).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let filename = url.path_segments()?.next_back()?;
    let (sha256, extension) = filename.rsplit_once('.')?;
    (!extension.is_empty()
        && sha256.len() == 64
        && sha256.bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then(|| sha256.to_string())
}

pub(super) fn persona_version_from_record(record: &serde_json::Value) -> Option<String> {
    let record: crate::managed_agents::ManagedAgentRecord =
        serde_json::from_value(record.clone()).ok()?;
    let definition = record.to_definition_view()?;
    Some(crate::managed_agents::persona_events::persona_content_hash(
        &crate::managed_agents::persona_events::persona_event_content(&definition),
    ))
}

fn built_in_persona_version_update(
    legacy_match: &LegacyAvatarMatch<'_>,
    legacy_avatar: &str,
    now: &str,
) -> Option<(String, String)> {
    let mut current =
        crate::managed_agents::built_in_persona_definition(&legacy_match.persona_id, now)?;
    let new_version = crate::managed_agents::persona_events::persona_content_hash(
        &crate::managed_agents::persona_events::persona_event_content(&current),
    );
    let old_version = if legacy_match.was_uploaded {
        legacy_match.metadata.persona_content_hash.to_string()
    } else {
        current.avatar_url = Some(legacy_avatar.to_string());
        crate::managed_agents::persona_events::persona_content_hash(
            &crate::managed_agents::persona_events::persona_event_content(&current),
        )
    };
    Some((old_version, new_version))
}

fn replace_builtin_avatar(record: &mut serde_json::Value, persona_id: &str, now: &str) -> bool {
    let Some(replacement) = crate::managed_agents::built_in_persona_avatar_url(persona_id) else {
        return false;
    };
    let Some(record) = record.as_object_mut() else {
        return false;
    };
    record.insert(
        "avatar_url".to_string(),
        serde_json::Value::String(replacement.to_string()),
    );
    record.insert(
        "updated_at".to_string(),
        serde_json::Value::String(now.to_string()),
    );
    true
}

#[cfg(test)]
#[path = "../migration_avatar_tests.rs"]
mod tests;
