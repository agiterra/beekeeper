//! NIP-IA identity archival commands.
//!
//! These commands let the desktop:
//!
//! - resolve a viewee's NIP-OA owner via their live `kind:0` (gates the
//!   "Archive" button when the current user is the owner-of-agent),
//! - submit `kind:9035` archive and `kind:9036` unarchive requests (consent
//!   path is selected by the relay; we just build the wire form),
//! - read the relay's `kind:13535` archive snapshot to drive UI flair.
//!
//! Spec: `docs/nips/NIP-IA.md`. The relay performs full authorization —
//! see §Owner-of-Agent Requests and §Relay Processing Algorithm.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    app_state::AppState,
    events,
    relay::{
        classify_request_error, query_relay, relay_http_base_url, relay_ws_url_with_override,
        submit_event, SubmitEventResponse,
    },
};

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Read `target`'s live `kind:0` event and extract the first valid NIP-OA
/// `auth` tag plus the verified owner pubkey.
///
/// Mirrors the verification the relay will do (per spec gotcha #3: the
/// preimage subject is the *target* pubkey, not the request signer). The
/// `buzz-sdk` lives on nostr 0.36; the desktop is on 0.37, so we bridge
/// via hex round-trip exactly like `relay::build_profile_event` does.
pub(crate) fn extract_oa_owner(target_kind0: &nostr::Event) -> Option<(String, [String; 4])> {
    let target_hex = target_kind0.pubkey.to_hex();
    let target_compat = nostr::PublicKey::from_hex(&target_hex).ok()?;

    for tag in target_kind0.tags.iter() {
        let slice = tag.as_slice();
        if slice.first().map(String::as_str) != Some("auth") || slice.len() != 4 {
            continue;
        }
        let json = serde_json::to_string(slice).ok()?;
        match buzz_sdk_pkg::nip_oa::verify_auth_tag(&json, &target_compat) {
            Ok(owner) => {
                let raw: [String; 4] = [
                    slice[0].clone(),
                    slice[1].clone(),
                    slice[2].clone(),
                    slice[3].clone(),
                ];
                return Some((owner.to_hex(), raw));
            }
            Err(_) => continue,
        }
    }
    None
}

pub(crate) async fn fetch_kind0(
    state: &AppState,
    pubkey: &str,
) -> Result<Option<nostr::Event>, String> {
    let events = query_relay(
        state,
        &[serde_json::json!({
            "kinds": [0],
            "authors": [pubkey.to_ascii_lowercase()],
            "limit": 1,
        })],
    )
    .await?;
    Ok(events.into_iter().next())
}

// ── Owner-of-agent resolution ───────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct OwnerOfAgent {
    /// Owner pubkey (hex) recovered from the viewee's verified NIP-OA `auth` tag.
    pub owner: String,
    /// True iff `owner` equals the current user's pubkey. Lets the frontend
    /// gate the "Archive" button without a second round-trip.
    pub is_me: bool,
}

/// Resolve `target`'s NIP-OA owner by reading its live `kind:0` and verifying
/// the embedded `auth` tag. Returns `None` if the target has no kind:0, no
/// `auth` tag, or the tag fails verification.
///
/// This is what gates the owner-path archive button: the frontend calls this,
/// and if `is_me == true`, shows the button.
#[tauri::command]
pub async fn resolve_oa_owner(
    target_pubkey: String,
    state: State<'_, AppState>,
) -> Result<Option<OwnerOfAgent>, String> {
    let Some(kind0) = fetch_kind0(&state, &target_pubkey).await? else {
        return Ok(None);
    };

    let Some((owner_hex, _tag)) = extract_oa_owner(&kind0) else {
        return Ok(None);
    };

    let my_pubkey = {
        let keys = state.keys.lock().map_err(|e| e.to_string())?;
        keys.public_key().to_hex()
    };

    Ok(Some(OwnerOfAgent {
        is_me: my_pubkey.eq_ignore_ascii_case(&owner_hex),
        owner: owner_hex,
    }))
}

// ── Archive / unarchive requests ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveRequest {
    pub target_pubkey: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub replaced_by: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnarchiveRequest {
    pub target_pubkey: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Submit a `kind:9035` archive request to the relay. Consent path is selected
/// by the relay — we just attach the owner-of-agent `auth` tag when the live
/// `kind:0` proves we own the target, so the relay can choose the `owner`
/// path. Self and admin paths require no auth tag.
#[tauri::command]
pub async fn archive_identity(
    req: ArchiveRequest,
    state: State<'_, AppState>,
) -> Result<SubmitEventResponse, String> {
    let auth_tag = maybe_owner_auth_tag(&state, &req.target_pubkey).await?;
    let auth_ref = auth_tag.as_ref();

    let builder = events::build_archive_identity_request(
        &req.target_pubkey,
        &req.content,
        req.reason.as_deref(),
        req.replaced_by.as_deref(),
        auth_ref,
    )?;
    submit_event(builder, &state).await
}

/// Submit a `kind:9036` unarchive request to the relay.
#[tauri::command]
pub async fn unarchive_identity(
    req: UnarchiveRequest,
    state: State<'_, AppState>,
) -> Result<SubmitEventResponse, String> {
    let auth_tag = maybe_owner_auth_tag(&state, &req.target_pubkey).await?;
    let auth_ref = auth_tag.as_ref();

    let builder = events::build_unarchive_identity_request(
        &req.target_pubkey,
        &req.content,
        req.reason.as_deref(),
        auth_ref,
    )?;
    submit_event(builder, &state).await
}

/// If the current user is the verified NIP-OA owner of `target`, return the
/// `auth` tag elements (label, owner, conditions, sig) for attachment to a
/// 9035/9036 request. Otherwise return `None` (self / admin / no-path).
///
/// The relay independently re-fetches the target's live `kind:0` and verifies
/// against it; this tag is intent + freshness evidence, not the authority.
async fn maybe_owner_auth_tag(
    state: &AppState,
    target_pubkey: &str,
) -> Result<Option<[String; 4]>, String> {
    let my_pubkey = {
        let keys = state.keys.lock().map_err(|e| e.to_string())?;
        keys.public_key().to_hex()
    };

    // Self path: never attach auth (spec §Self Requests: if actor==target and
    // an `auth` tag is also present, relay MUST treat it as self).
    if my_pubkey.eq_ignore_ascii_case(target_pubkey) {
        return Ok(None);
    }

    let Some(kind0) = fetch_kind0(state, target_pubkey).await? else {
        return Ok(None);
    };
    let Some((owner_hex, raw_tag)) = extract_oa_owner(&kind0) else {
        return Ok(None);
    };

    if !owner_hex.eq_ignore_ascii_case(&my_pubkey) {
        return Ok(None);
    }
    Ok(Some(raw_tag))
}

// ── Archive snapshot ────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ArchivedIdentitiesSnapshot {
    /// Lowercase hex pubkeys present in the latest relay-signed `kind:13535`.
    pub archived: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RelayInformationDocument {
    #[serde(default, rename = "self")]
    self_: Option<String>,
    /// NIP-11 `software_commit` (finding 32,
    /// `review-2026-09-01/LIVE-RUN-TeamRolesV1.md`): the relay's own
    /// disclosed build commit, or the literal `unknown`. Absent entirely on a
    /// relay predating that field — `#[serde(default)]` reads that the same
    /// as an explicit `unknown` would.
    #[serde(default)]
    software_commit: Option<String>,
    /// NIP-11 `software_commit_count`: `git rev-list --count` of
    /// `software_commit`, or `null`/absent when the relay could not
    /// determine one.
    ///
    /// Deserialized leniently on purpose. The whole document is parsed with
    /// a fallback below, so *any* strict field turns one malformed value
    /// into a total parse failure and silently blanks `software_commit`
    /// too — regressing the "Relay build" line that already ships. A relay
    /// sending a string, a float, or a negative here loses only the count.
    #[serde(default, deserialize_with = "lenient_commit_count")]
    software_commit_count: Option<u32>,
    /// NIP-11 `build_time`, an RFC 3339 UTC stamp or `unknown`.
    #[serde(default, deserialize_with = "lenient_string")]
    build_time: Option<String>,
    /// NIP-11 `software`: the repository this relay was built from. Used to
    /// refuse comparing ordinals across different repositories.
    #[serde(default, deserialize_with = "lenient_string")]
    software: Option<String>,
}

/// A `u32` that degrades to `None` rather than failing the document.
///
/// Also refuses `0`: `rev-list --count` of a real commit is at least 1, so a
/// `0` could only come from a broken pipeline — and unlike `None` it would
/// silently take part in a consumer's subtraction and read as agreement.
fn lenient_commit_count<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(serde_json::Value::deserialize(deserializer)
        .ok()
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
        .filter(|count| *count >= 1))
}

/// A string that degrades to `None` rather than failing the document.
fn lenient_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(serde_json::Value::deserialize(deserializer)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned)))
}

/// What a relay discloses about the build serving a request.
///
/// Every field is independently optional: a relay predating any of them, or
/// one that could not determine its own, answers `null` rather than being
/// treated as unreachable.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayBuildIdentity {
    /// Full 40-hex `software_commit`, lowercased, or `None`.
    pub commit: Option<String>,
    /// `software_commit_count`, or `None`.
    pub commit_count: Option<u32>,
    /// `build_time`, or `None`.
    pub build_time: Option<String>,
    /// `software` — the repository URL the relay names.
    pub software: Option<String>,
}

pub(crate) async fn fetch_relay_self(state: &AppState) -> Result<Option<String>, String> {
    let relay_url = relay_ws_url_with_override(state);
    let http_url = relay_http_base_url(&relay_url);
    let response = state
        .http_client
        .get(&http_url)
        .header("Accept", "application/nostr+json")
        .send()
        .await
        .map_err(|e| classify_request_error(&e))?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let doc = response
        .json::<RelayInformationDocument>()
        .await
        .map_err(|_| "relay returned malformed NIP-11 document".to_string())?;

    let Some(relay_self) = doc.self_.map(|value| value.to_ascii_lowercase()) else {
        return Ok(None);
    };

    if relay_self.len() == 64 && relay_self.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(Some(relay_self))
    } else {
        Ok(None)
    }
}

/// Read a relay's own disclosed build commit (NIP-11 `software_commit`,
/// finding 32 — `review-2026-09-01/LIVE-RUN-TeamRolesV1.md`).
///
/// Takes `relay_url` explicitly rather than reading the *active* community
/// off `state` — same shape as [`crate::commands::workspace::fetch_workspace_icon`]
/// — because the caller (`EditCommunityDialog.tsx`) can be editing a
/// community that is not the one currently active, and this must report on
/// the relay being edited, not whichever one happens to be live.
///
/// Returns the full 40-hex commit when the relay advertises a well-formed
/// one, or `None` for every other case — unreachable relay, malformed
/// document, a relay predating this field entirely, or a value that is not a
/// plausible commit. `None` is disclosed by the UI as `unknown`, the same
/// literal the relay itself would use; this function never invents or
/// truncates — truncation to 8 hex for display is the caller's job, because a
/// command's return value should stay the precise fact.
#[tauri::command]
pub async fn get_relay_build_commit(
    relay_url: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    Ok(get_relay_build_identity(relay_url, state).await?.commit)
}

/// The full build identity the relay at `relay_url` discloses over NIP-11.
///
/// One fetch and one parse shared with [`get_relay_build_commit`], which is a
/// projection of this. An unreachable relay, a non-success status, or a
/// document that cannot be parsed at all yields an all-`None` identity rather
/// than an error: not knowing is a disclosed answer here, not a failure, and
/// the caller renders it as "unknown".
#[tauri::command]
pub async fn get_relay_build_identity(
    relay_url: String,
    state: State<'_, AppState>,
) -> Result<RelayBuildIdentity, String> {
    let http_url = relay_http_base_url(&relay_url);
    let Ok(response) = state
        .http_client
        .get(&http_url)
        .header("Accept", "application/nostr+json")
        .send()
        .await
    else {
        return Ok(RelayBuildIdentity::default());
    };
    if !response.status().is_success() {
        return Ok(RelayBuildIdentity::default());
    }
    let doc = response
        .json::<RelayInformationDocument>()
        .await
        .unwrap_or_default();
    Ok(relay_build_identity_from_doc(doc))
}

/// Validate a parsed NIP-11 document into an identity.
///
/// Pure, so the "a malformed count must not cost us the commit" contract is
/// testable without a live relay.
fn relay_build_identity_from_doc(doc: RelayInformationDocument) -> RelayBuildIdentity {
    let commit = doc
        .software_commit
        .map(|value| value.to_ascii_lowercase())
        .filter(|commit| {
            commit.len() == 40
                && commit
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        });
    RelayBuildIdentity {
        // A count with no usable commit describes nothing — the same
        // coupling the relay enforces when it stamps the pair.
        commit_count: commit.as_ref().and_then(|_| doc.software_commit_count),
        commit,
        build_time: doc.build_time,
        software: doc.software,
    }
}

fn archived_pubkeys_from_snapshot(snapshot: &nostr::Event) -> Vec<String> {
    snapshot
        .tags
        .iter()
        .filter_map(|t| {
            let slice = t.as_slice();
            if slice.first().map(String::as_str) == Some("p") && slice.len() >= 2 {
                let pk = slice[1].to_ascii_lowercase();
                if pk.len() == 64 && pk.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(pk);
                }
            }
            None
        })
        .collect()
}

/// Read the relay's latest valid `kind:13535` archive snapshot. The frontend
/// caches this and tests membership client-side to drive the "Archived" flair.
///
/// Per NIP-IA §Client Behavior and §Snapshot and Delta Consistency, only a
/// snapshot signed by the relay identity advertised in NIP-11 `self` can affect
/// archive state. If the relay has no stable `self`, fail open with an empty
/// snapshot rather than trusting unauthenticated relay-authoritative state.
#[tauri::command]
pub async fn list_archived_identities(
    state: State<'_, AppState>,
) -> Result<ArchivedIdentitiesSnapshot, String> {
    let Some(relay_self) = fetch_relay_self(&state).await? else {
        return Ok(ArchivedIdentitiesSnapshot { archived: vec![] });
    };

    let events = query_relay(
        &state,
        &[serde_json::json!({
            "authors": [relay_self.clone()],
            "kinds": [13535],
            "limit": 1,
        })],
    )
    .await?;

    let Some(snapshot) = events.into_iter().next() else {
        return Ok(ArchivedIdentitiesSnapshot { archived: vec![] });
    };

    // Defense-in-depth: the filter should already restrict author, but the
    // client must still reject malformed or wrongly signed relay state.
    if !snapshot.verify_id() || !snapshot.verify_signature() {
        return Ok(ArchivedIdentitiesSnapshot { archived: vec![] });
    }
    if !snapshot.pubkey.to_hex().eq_ignore_ascii_case(&relay_self) {
        return Ok(ArchivedIdentitiesSnapshot { archived: vec![] });
    }

    Ok(ArchivedIdentitiesSnapshot {
        archived: archived_pubkeys_from_snapshot(&snapshot),
    })
}

/// Read the active relay's NIP-11 `self` pubkey (its own signing key, hex).
///
/// A public, unauthenticated document read reused by the moderation UI to tell
/// whether a DM peer is the relay identity (a moderation DM). Fails open: an
/// unreachable relay, a document without `self`, or a malformed value all
/// return `None`, and callers must treat that as "not the relay" — the disable
/// is an affordance, not enforcement, so a false negative is the safe failure.
#[tauri::command]
pub async fn get_relay_self(state: State<'_, AppState>) -> Result<Option<String>, String> {
    fetch_relay_self(&state).await
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    /// Build a fake `kind:0` with a valid NIP-OA auth tag for a fresh owner.
    fn kind0_with_auth(agent: &Keys, owner: &Keys) -> nostr::Event {
        // Compute auth tag via buzz-sdk (nostr 0.36) and bridge.
        let agent_hex = agent.public_key().to_hex();
        let agent_compat = nostr::PublicKey::from_hex(&agent_hex).unwrap();
        let owner_compat_secret =
            nostr::SecretKey::from_slice(owner.secret_key().as_secret_bytes()).unwrap();
        let owner_compat_keys = nostr::Keys::new(owner_compat_secret);
        let tag_json =
            buzz_sdk_pkg::nip_oa::compute_auth_tag(&owner_compat_keys, &agent_compat, "")
                .expect("compute_auth_tag");
        let compat_tag = buzz_sdk_pkg::nip_oa::parse_auth_tag(&tag_json).unwrap();
        let tag = Tag::parse(compat_tag.as_slice()).unwrap();
        EventBuilder::new(Kind::Metadata, "{}")
            .tags([tag])
            .sign_with_keys(agent)
            .unwrap()
    }

    #[test]
    fn extract_oa_owner_returns_owner_for_valid_tag() {
        let owner = Keys::generate();
        let agent = Keys::generate();
        let kind0 = kind0_with_auth(&agent, &owner);

        let (recovered, raw) = extract_oa_owner(&kind0).expect("auth tag should verify");
        assert_eq!(recovered, owner.public_key().to_hex());
        assert_eq!(raw[0], "auth");
        assert_eq!(raw[1], owner.public_key().to_hex());
        // conditions empty by construction
        assert_eq!(raw[2], "");
        assert_eq!(raw[3].len(), 128);
    }

    #[test]
    fn extract_oa_owner_ignores_kind0_without_auth_tag() {
        let agent = Keys::generate();
        let kind0 = EventBuilder::new(Kind::Metadata, "{}")
            .sign_with_keys(&agent)
            .unwrap();
        assert!(extract_oa_owner(&kind0).is_none());
    }

    #[test]
    fn archived_pubkeys_from_snapshot_accepts_only_valid_p_tags() {
        let relay = Keys::generate();
        let valid = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let uppercase = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
        let snapshot = EventBuilder::new(Kind::Custom(13535), "")
            .tags([
                Tag::parse(["-"]).unwrap(),
                Tag::parse(["p", valid]).unwrap(),
                Tag::parse(["p", uppercase]).unwrap(),
                Tag::parse(["p", "not-hex"]).unwrap(),
            ])
            .sign_with_keys(&relay)
            .unwrap();

        let expected = vec![valid.to_string(), uppercase.to_ascii_lowercase()];
        assert_eq!(archived_pubkeys_from_snapshot(&snapshot), expected);
    }

    #[test]
    fn relay_information_document_reads_nip11_self_field() {
        let doc: RelayInformationDocument = serde_json::from_str(
            r#"{"name":"test relay","self":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
        )
        .expect("NIP-11 document");

        assert_eq!(
            doc.self_.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        );
    }

    /// Spec test-vector regression for gotcha #3: the NIP-OA preimage subject
    /// is the *target/agent* pubkey, not the request signer. The vectors in
    /// `docs/nips/NIP-IA.md` §Test Vectors fix concrete values; verifying the
    /// vector's `auth` tag under the vector's agent pubkey MUST yield the
    /// vector's owner pubkey. If our `extract_oa_owner` ever stops using the
    /// agent pubkey as the preimage subject, this test fails loudly.
    #[test]
    fn extract_oa_owner_matches_nip_ia_test_vector() {
        // From docs/nips/NIP-IA.md §Test Vectors → "NIP-OA auth tag".
        const AGENT_HEX: &str = "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";
        const OWNER_HEX: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
        const CONDITIONS: &str = "kind=1&created_at<1713957000";
        const SIG: &str = "8b7df2575caf0a108374f8471722b233c53f9ff827a8b0f91861966c3b9dd5cb2e189eae9f49d72187674c2f5bd244145e10ff86c9f257ffe65a1ee5f108b369";

        // We don't have the agent's secret key (it's `0x...02` in the spec, but
        // we don't need to re-sign a kind:0 — we just need a kind:0 whose
        // `pubkey` is AGENT_HEX and whose tags carry this auth tag). Sign with
        // a *different* agent and then construct an unsigned-event-shaped
        // struct ourselves. nostr 0.37 doesn't easily allow forging `pubkey`
        // mismatched with the signing key, so we build via the public
        // constructor that requires a key — and for THIS test, the kind:0
        // signature is not checked (we only call extract_oa_owner which reads
        // the event's pubkey field and the auth tag bytes).
        let agent_secret = nostr::SecretKey::from_hex(
            "0000000000000000000000000000000000000000000000000000000000000002",
        )
        .unwrap();
        let agent_keys = nostr::Keys::new(agent_secret);
        assert_eq!(agent_keys.public_key().to_hex(), AGENT_HEX);

        let auth_tag = nostr::Tag::parse(["auth", OWNER_HEX, CONDITIONS, SIG]).unwrap();
        let kind0 = EventBuilder::new(Kind::Metadata, "{}")
            .tags([auth_tag])
            .sign_with_keys(&agent_keys)
            .unwrap();

        let (owner, raw) = extract_oa_owner(&kind0).expect("spec vector should verify");
        assert_eq!(owner, OWNER_HEX);
        assert_eq!(raw[1], OWNER_HEX);
        assert_eq!(raw[2], CONDITIONS);
        assert_eq!(raw[3], SIG);
    }

    /// Regression: the frontend sends the request payload in camelCase
    /// (`targetPubkey`, `replacedBy`); these structs MUST deserialize it.
    /// Without `#[serde(rename_all = "camelCase")]` the archive/unarchive
    /// commands fail to deserialize at runtime — a failure the e2e mock hides
    /// because it returns before parsing the payload. Red-if-broken guard.
    #[test]
    fn archive_request_deserializes_camel_case_payload() {
        let req: ArchiveRequest = serde_json::from_str(
            r#"{"targetPubkey":"abc","content":"bye","reason":"bot-rebuilt","replacedBy":"def"}"#,
        )
        .expect("camelCase archive payload must deserialize");
        assert_eq!(req.target_pubkey, "abc");
        assert_eq!(req.content, "bye");
        assert_eq!(req.reason.as_deref(), Some("bot-rebuilt"));
        assert_eq!(req.replaced_by.as_deref(), Some("def"));

        // Minimal payload (only the required field) still deserializes.
        let minimal: UnarchiveRequest =
            serde_json::from_str(r#"{"targetPubkey":"abc"}"#).expect("minimal payload");
        assert_eq!(minimal.target_pubkey, "abc");
        assert_eq!(minimal.content, "");
        assert!(minimal.reason.is_none());
    }

    const RELAY_SHA: &str = "42dd921d831c483e6e16111491b39947b4cf1f86";

    fn parse_doc(json: &str) -> RelayBuildIdentity {
        relay_build_identity_from_doc(
            serde_json::from_str::<RelayInformationDocument>(json).expect("document parses"),
        )
    }

    /// The regression this whole lenient-deserializer shape exists to
    /// prevent. The document is parsed with a fallback, so one strict field
    /// would turn a malformed count into a total parse failure and blank
    /// `software_commit` — silently killing `EditCommunityDialog`'s shipped
    /// "Relay build" line for anyone whose relay sent something odd.
    #[test]
    fn a_malformed_commit_count_never_costs_us_the_commit() {
        for junk in [
            r#""40312""#, // a string
            "1e9",        // a float
            "-1",
            "0",
            "null",
            "[]",
            "{}",
            "true",
        ] {
            let identity = parse_doc(&format!(
                r#"{{"software_commit":"{RELAY_SHA}","software_commit_count":{junk}}}"#
            ));
            assert_eq!(
                identity.commit.as_deref(),
                Some(RELAY_SHA),
                "the commit must survive a {junk} count"
            );
            assert_eq!(
                identity.commit_count, None,
                "and the count is refused: {junk}"
            );
        }
    }

    #[test]
    fn a_well_formed_document_is_read_whole() {
        let identity = parse_doc(&format!(
            r#"{{"software":"https://github.com/agiterra/beekeeper",
                 "software_commit":"{RELAY_SHA}",
                 "software_commit_count":3291,
                 "build_time":"2026-09-03T02:51:29Z"}}"#
        ));
        assert_eq!(identity.commit.as_deref(), Some(RELAY_SHA));
        assert_eq!(identity.commit_count, Some(3291));
        assert_eq!(identity.build_time.as_deref(), Some("2026-09-03T02:51:29Z"));
        assert_eq!(
            identity.software.as_deref(),
            Some("https://github.com/agiterra/beekeeper")
        );
    }

    #[test]
    fn a_relay_predating_every_field_reads_as_unknown_not_as_unreachable() {
        let identity = parse_doc(r#"{"name":"Beekeeper Relay"}"#);
        assert_eq!(identity, RelayBuildIdentity::default());
    }

    #[test]
    fn an_unknown_commit_takes_its_count_with_it() {
        // `unknown` is the relay's disclosed non-answer, not a commit. A
        // count beside it describes a history we cannot name, so carrying it
        // would invite a comparison against nothing.
        let identity = parse_doc(r#"{"software_commit":"unknown","software_commit_count":3291}"#);
        assert_eq!(identity.commit, None);
        assert_eq!(identity.commit_count, None);
    }

    #[test]
    fn a_short_or_uppercase_commit_is_refused_but_lowercase_is_normalized() {
        assert_eq!(parse_doc(r#"{"software_commit":"42dd921d8"}"#).commit, None);
        assert_eq!(
            parse_doc(&format!(
                r#"{{"software_commit":"{}"}}"#,
                RELAY_SHA.to_ascii_uppercase()
            ))
            .commit
            .as_deref(),
            Some(RELAY_SHA)
        );
    }
}
