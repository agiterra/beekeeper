//! `bee terminals` — NIP-ST shared terminals: list announces (kind 30623),
//! manage a session's roster, and send remote input (kind 24312).
//!
//! Roster mutations are read-modify-writes of the caller's **own** announce
//! head (`kinds:[30623] + authors:[self] + #d:[session-id]`), bumped to
//! `created_at = head + 1` — never wall-clock — exactly like the project
//! head mutations in `projects.rs`. Input events are ephemeral and go over
//! the WebSocket path (`publish_ephemeral_event`); the relay accepts them
//! only from the session owner or a roster collaborator and delivers them
//! only to the owner, who independently re-verifies the sender before any
//! byte reaches the PTY.
//!
//! Every relay filter carries explicit `kinds` — an open-ended query trips
//! the relay's p-gate and returns 403.

use base64::Engine as _;
use buzz_core::kind::{KIND_SHELL_INPUT, KIND_SHELL_SESSION, SHELL_ROLES};
use nostr::{Event, EventBuilder, Kind, Tag, Timestamp};
use serde_json::{json, Value};

use crate::client::BuzzClient;
use buzz_sdk::build_delete_addressable;

use crate::commands::parse_write_response;
use crate::commands::repos::next_replaceable_created_at;
use crate::error::CliError;
use crate::validate::validate_lower_hex64;

/// Maximum raw bytes per kind:24312 input event. The relay caps content at
/// 8 KiB of base64; 6 KiB of raw bytes encodes to exactly that budget.
const MAX_INPUT_CHUNK_RAW_BYTES: usize = 6 * 1024;

/// Maximum roster entries on one announce (owner never listed).
const SHELL_ROSTER_CAP: usize = 64;

// ── Small helpers ─────────────────────────────────────────────────────────────

fn tag_name(tag: &Tag) -> Option<&str> {
    tag.as_slice().first().map(String::as_str)
}

fn make_tag(parts: &[&str]) -> Result<Tag, CliError> {
    Tag::parse(parts.iter().copied())
        .map_err(|e| CliError::Other(format!("tag construction failed: {e}")))
}

/// Validate a session id: 1–64 chars of `[A-Za-z0-9-]`, matching the relay's
/// announce-envelope rule.
fn validate_session_id(session_id: &str) -> Result<(), CliError> {
    if session_id.is_empty()
        || session_id.len() > 64
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(CliError::Usage(format!(
            "session id must be 1-64 characters of [A-Za-z0-9-]: {session_id:?}"
        )));
    }
    Ok(())
}

/// Validate a shared-terminal roster role.
fn validate_shell_role(role: &str) -> Result<(), CliError> {
    if !buzz_core::kind::is_valid_shell_role(role) {
        return Err(CliError::Usage(format!(
            "roster role must be one of {SHELL_ROLES:?} (got {role:?})"
        )));
    }
    Ok(())
}

/// Expand a `--project` argument into a canonical `30621:<owner>:<dtag>`
/// coordinate. A value with a colon must already be a full coordinate; a
/// bare slug is expanded with the caller as owner.
fn expand_project_arg(input: &str, caller_pubkey: &str) -> Result<String, CliError> {
    let candidate = if input.contains(':') {
        input.to_string()
    } else {
        format!("30621:{caller_pubkey}:{input}")
    };
    buzz_core::kind::normalize_project_coordinate(&candidate).ok_or_else(|| {
        CliError::Usage(format!(
            "--project must be a `30621:<owner-hex>:<dtag>` coordinate or a bare slug (got {input:?})"
        ))
    })
}

// ── Announce reads ────────────────────────────────────────────────────────────

/// The roster of an announce as `(pubkey, role)` pairs, from its arity-4
/// `["p", <hex>, <hint>, <role>]` tags. Tags with an unknown or missing
/// role are skipped — mirroring `buzz_core::kind::shell_session_roster`.
fn roster_from_event_json(event: &Value) -> Vec<(String, String)> {
    let Some(tags) = event.get("tags").and_then(Value::as_array) else {
        return Vec::new();
    };
    tags.iter()
        .filter_map(|tag| {
            let parts = tag.as_array()?;
            if parts.first()?.as_str()? != "p" {
                return None;
            }
            let pubkey = parts.get(1)?.as_str()?.to_string();
            let role = parts.get(3)?.as_str()?;
            buzz_core::kind::is_valid_shell_role(role).then(|| (pubkey, role.to_string()))
        })
        .collect()
}

/// First value of the named tag in a raw event JSON value.
fn json_tag_value(event: &Value, name: &str) -> Option<String> {
    event
        .get("tags")?
        .as_array()?
        .iter()
        .filter_map(Value::as_array)
        .find(|parts| parts.first().and_then(Value::as_str) == Some(name))
        .and_then(|parts| parts.get(1))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Fetch one owner's announce head for a session id, as a signed `Event`
/// (used by the read-modify-write mutations).
async fn fetch_announce(
    client: &BuzzClient,
    owner_hex: &str,
    session_id: &str,
) -> Result<Option<Event>, CliError> {
    let filter = json!({
        "kinds": [KIND_SHELL_SESSION],
        "authors": [owner_hex],
        "#d": [session_id],
        "limit": 1,
    });
    let raw = client.query(&filter).await?;
    let mut events: Vec<Event> = serde_json::from_str(&raw)
        .map_err(|e| CliError::Other(format!("failed to parse relay response: {e}")))?;
    events.sort_by_key(|e| std::cmp::Reverse(e.created_at));
    Ok(events.into_iter().next())
}

/// Fetch one owner's announce head as raw JSON (used by the read commands,
/// which do not need to re-sign it).
async fn fetch_announce_json(
    client: &BuzzClient,
    owner_hex: &str,
    session_id: &str,
) -> Result<Option<Value>, CliError> {
    let filter = json!({
        "kinds": [KIND_SHELL_SESSION],
        "authors": [owner_hex],
        "#d": [session_id],
        "limit": 1,
    });
    let raw = client.query(&filter).await?;
    let mut events: Vec<Value> = serde_json::from_str(&raw)
        .map_err(|e| CliError::Other(format!("failed to parse relay response: {e}")))?;
    events.sort_by_key(|event| std::cmp::Reverse(event.get("created_at").and_then(Value::as_i64)));
    Ok(events.into_iter().next())
}

// ── Commands ─────────────────────────────────────────────────────────────────

/// `bee terminals list` — announces visible to the caller, optionally
/// scoped to one project coordinate.
pub async fn cmd_list(client: &BuzzClient, project: Option<&str>) -> Result<(), CliError> {
    let caller = client.keys().public_key().to_hex();
    let mut filter = json!({ "kinds": [KIND_SHELL_SESSION] });
    if let Some(project) = project {
        let coordinate = expand_project_arg(project, &caller)?;
        filter["#a"] = json!([coordinate]);
    }
    let events = client.query_all(filter).await?;

    let output: Vec<Value> = events
        .iter()
        .map(|event| {
            let roster: Vec<Value> = roster_from_event_json(event)
                .into_iter()
                .map(|(pubkey, role)| json!({ "pubkey": pubkey, "role": role }))
                .collect();
            json!({
                "owner": event.get("pubkey"),
                "sessionId": json_tag_value(event, "d"),
                "title": json_tag_value(event, "title"),
                "status": json_tag_value(event, "status"),
                "coordinate": json_tag_value(event, "a"),
                "roster": roster,
            })
        })
        .collect();
    println!("{}", Value::Array(output));
    Ok(())
}

/// Rebuild an announce head with a mutated roster: strips `auth` tags,
/// bumps `created_at = head + 1`, and enforces the roster cap.
fn rebuild_announce(head: &Event, tags: Vec<Tag>) -> Result<EventBuilder, CliError> {
    let roster_len = tags.iter().filter(|t| tag_name(t) == Some("p")).count();
    if roster_len > SHELL_ROSTER_CAP {
        return Err(CliError::Usage(format!(
            "roster is capped at {SHELL_ROSTER_CAP} members (would be {roster_len})"
        )));
    }
    let clean_tags: Vec<Tag> = tags
        .into_iter()
        .filter(|t| tag_name(t) != Some("auth"))
        .collect();
    let next_ts =
        next_replaceable_created_at(head.created_at.as_secs(), Timestamp::now().as_secs())
            .map(Timestamp::from)
            .ok_or_else(|| CliError::Other("announce timestamp cannot be advanced".into()))?;
    Ok(
        EventBuilder::new(Kind::Custom(KIND_SHELL_SESSION as u16), &head.content)
            .tags(clean_tags)
            .custom_created_at(next_ts),
    )
}

/// Fetch the caller's own announce head, or error with a clear message.
async fn fetch_own_announce(client: &BuzzClient, session_id: &str) -> Result<Event, CliError> {
    let caller = client.keys().public_key().to_hex();
    fetch_announce(client, &caller, session_id)
        .await?
        .ok_or_else(|| {
            CliError::NotFound(format!(
                "no shared-terminal announce of yours has session id {session_id:?} — \
                 only the session owner can manage its roster"
            ))
        })
}

/// `bee terminals invite` — add a pubkey to the caller's own announce
/// roster, or change their role.
pub async fn cmd_invite(
    client: &BuzzClient,
    session_id: &str,
    pubkey: &str,
    role: &str,
) -> Result<(), CliError> {
    validate_session_id(session_id)?;
    validate_lower_hex64("--pubkey", pubkey)?;
    validate_shell_role(role)?;
    if pubkey == client.keys().public_key().to_hex() {
        return Err(CliError::Usage(
            "the session owner signs the announce and is never listed on its roster".into(),
        ));
    }

    let head = fetch_own_announce(client, session_id).await?;
    // Replace any existing entry for this pubkey (re-invite = role change).
    let mut tags: Vec<Tag> = head
        .tags
        .iter()
        .filter(|t| {
            !(tag_name(t) == Some("p") && t.as_slice().get(1).map(String::as_str) == Some(pubkey))
        })
        .cloned()
        .collect();
    tags.push(make_tag(&["p", pubkey, "", role])?);

    let builder = rebuild_announce(&head, tags)?;
    let event = client.sign_event(builder)?;
    let raw = client.submit_event(event).await?;
    println!(
        "{}",
        parse_write_response(&raw, "announce changed concurrently; retry")?
    );
    Ok(())
}

/// `bee terminals revoke` — remove a pubkey from the caller's own announce
/// roster.
pub async fn cmd_revoke(
    client: &BuzzClient,
    session_id: &str,
    pubkey: &str,
) -> Result<(), CliError> {
    validate_session_id(session_id)?;
    validate_lower_hex64("--pubkey", pubkey)?;

    let head = fetch_own_announce(client, session_id).await?;
    let mut removed = false;
    let tags: Vec<Tag> = head
        .tags
        .iter()
        .filter(|t| {
            let is_target =
                tag_name(t) == Some("p") && t.as_slice().get(1).map(String::as_str) == Some(pubkey);
            removed |= is_target;
            !is_target
        })
        .cloned()
        .collect();
    if !removed {
        return Err(CliError::NotFound(format!(
            "pubkey {pubkey} is not on the roster of session {session_id:?}"
        )));
    }

    let builder = rebuild_announce(&head, tags)?;
    let event = client.sign_event(builder)?;
    let raw = client.submit_event(event).await?;
    println!(
        "{}",
        parse_write_response(&raw, "announce changed concurrently; retry")?
    );
    Ok(())
}

/// `bee terminals delete` — tombstone a shared-terminal announce.
///
/// A kind:5 carrying `["a", "30623:<owner>:<session-id>"]`. The relay's
/// addressable-deletion path soft-deletes the announce and drops its roster
/// projection (`delete_shell_session_acl`), so the 24310 watch and 24312
/// input gates stop honouring invites on a session that no longer exists
/// and fall back to project access alone.
///
/// Deliberately **not** a read-modify-write like `invite`/`revoke`: those
/// republish the head, and there is no head left to republish here. The
/// announce is therefore not fetched first — a delete that required the
/// announce to be readable would fail for exactly the case a project Owner
/// needs it, and the relay is the authority on whether the coordinate
/// exists either way.
///
/// What this does not do is reach the owner's machine. A PTY that is still
/// running keeps running; it simply becomes unshareable. Saying otherwise
/// in the output would be a claim this command cannot keep.
pub async fn cmd_delete(
    client: &BuzzClient,
    session_id: &str,
    owner: Option<&str>,
) -> Result<(), CliError> {
    validate_session_id(session_id)?;
    let owner_hex = match owner {
        Some(owner) => {
            validate_lower_hex64("--owner", owner)?;
            owner.to_string()
        }
        None => client.keys().public_key().to_hex(),
    };

    let builder = build_delete_addressable(KIND_SHELL_SESSION, &owner_hex, session_id)
        .map_err(|e| CliError::Usage(e.to_string()))?;
    let event = client.sign_event(builder)?;
    let raw = client.submit_event(event).await?;
    println!(
        "{}",
        parse_write_response(&raw, "no live announce matched that coordinate")?
    );
    Ok(())
}

/// `bee terminals roster` — print an announce's roster as
/// `[{pubkey, role}]`.
pub async fn cmd_roster(
    client: &BuzzClient,
    session_id: &str,
    owner: Option<&str>,
) -> Result<(), CliError> {
    validate_session_id(session_id)?;
    let owner_hex = match owner {
        Some(owner) => {
            validate_lower_hex64("--owner", owner)?;
            owner.to_string()
        }
        None => client.keys().public_key().to_hex(),
    };
    let announce = fetch_announce_json(client, &owner_hex, session_id)
        .await?
        .ok_or_else(|| {
            CliError::NotFound(format!(
                "no shared-terminal announce for session {session_id:?} by owner {owner_hex}"
            ))
        })?;
    let output: Vec<Value> = roster_from_event_json(&announce)
        .into_iter()
        .map(|(pubkey, role)| json!({ "pubkey": pubkey, "role": role }))
        .collect();
    println!("{}", Value::Array(output));
    Ok(())
}

/// Split raw input bytes into ≤6 KiB chunks, base64-encoded for kind:24312
/// content.
fn chunk_input_bytes(bytes: &[u8]) -> Vec<String> {
    bytes
        .chunks(MAX_INPUT_CHUNK_RAW_BYTES)
        .map(|chunk| base64::engine::general_purpose::STANDARD.encode(chunk))
        .collect()
}

/// `bee terminals send-input` — send raw bytes to a shared terminal.
///
/// The announce is fetched first: its `a` coordinate is what the relay binds
/// input to, and a closed session is refused locally with a clearer message
/// than the relay's. Chunks are published sequentially over the WebSocket
/// (kind 24312 is ephemeral; the relay rejects ephemeral kinds over HTTP)
/// and the relay's 20 events/s input budget comfortably covers them.
pub async fn cmd_send_input(
    client: &BuzzClient,
    session_id: &str,
    owner: &str,
    text: Option<&str>,
    use_stdin: bool,
) -> Result<(), CliError> {
    validate_session_id(session_id)?;
    validate_lower_hex64("--owner", owner)?;

    let bytes: Vec<u8> = match (text, use_stdin) {
        (Some(text), false) => text.as_bytes().to_vec(),
        (None, true) => {
            use std::io::Read;
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| CliError::Other(format!("failed to read stdin: {e}")))?;
            buf
        }
        // clap enforces exactly one of --text / --stdin; this is the
        // defense-in-depth arm for direct callers.
        _ => {
            return Err(CliError::Usage(
                "exactly one of --text or --stdin is required".into(),
            ))
        }
    };
    if bytes.is_empty() {
        return Err(CliError::Usage("refusing to send empty input".into()));
    }

    let announce = fetch_announce_json(client, owner, session_id)
        .await?
        .ok_or_else(|| {
            CliError::NotFound(format!(
                "no shared-terminal announce for session {session_id:?} by owner {owner}"
            ))
        })?;
    if json_tag_value(&announce, "status").as_deref() != Some("open") {
        return Err(CliError::Usage(format!(
            "session {session_id:?} is not open — input is refused"
        )));
    }
    let coordinate = json_tag_value(&announce, "a")
        .ok_or_else(|| CliError::Other("announce carries no project coordinate `a` tag".into()))?;

    let chunks = chunk_input_bytes(&bytes);
    let chunk_count = chunks.len();
    for content in chunks {
        let tags = vec![
            make_tag(&["d", session_id])?,
            make_tag(&["a", &coordinate])?,
            make_tag(&["p", owner])?,
        ];
        let builder = EventBuilder::new(Kind::Custom(KIND_SHELL_INPUT as u16), &content).tags(tags);
        let event = client.sign_event(builder)?;
        client.publish_ephemeral_event(event).await?;
    }

    println!(
        "{}",
        json!({
            "accepted": true,
            "sessionId": session_id,
            "owner": owner,
            "bytes": bytes.len(),
            "chunks": chunk_count,
        })
    );
    Ok(())
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

/// Route one `terminals` subcommand.
pub async fn dispatch(cmd: crate::TerminalsCmd, client: &BuzzClient) -> Result<(), CliError> {
    use crate::TerminalsCmd;
    match cmd {
        TerminalsCmd::List { project } => cmd_list(client, project.as_deref()).await,
        TerminalsCmd::Invite {
            session_id,
            pubkey,
            role,
        } => cmd_invite(client, &session_id, &pubkey, role.as_str()).await,
        TerminalsCmd::Revoke { session_id, pubkey } => {
            cmd_revoke(client, &session_id, &pubkey).await
        }
        TerminalsCmd::Delete { session_id, owner } => {
            cmd_delete(client, &session_id, owner.as_deref()).await
        }
        TerminalsCmd::Roster { session_id, owner } => {
            cmd_roster(client, &session_id, owner.as_deref()).await
        }
        TerminalsCmd::SendInput {
            session_id,
            owner,
            text,
            stdin,
        } => cmd_send_input(client, &session_id, &owner, text.as_deref(), stdin).await,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::Keys;

    const OWNER_HEX: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    // ── Validation ────────────────────────────────────────────────────────────

    #[test]
    fn session_id_accepts_uuid_shapes_and_rejects_traversal() {
        assert!(validate_session_id("a4f6c8e0-1111-2222-3333-444455556666").is_ok());
        assert!(validate_session_id("abc-123").is_ok());
        assert!(validate_session_id("").is_err());
        assert!(validate_session_id(&"a".repeat(65)).is_err());
        assert!(validate_session_id("../../etc/passwd").is_err());
    }

    // ── delete ────────────────────────────────────────────────────────────────

    /// The tombstone this command signs: a kind:5 naming exactly the
    /// announce's coordinate and nothing else. The relay routes on that `a`
    /// tag alone, so its shape is the whole contract.
    #[test]
    fn delete_builds_a_kind_5_naming_the_announce_coordinate() {
        let builder = build_delete_addressable(KIND_SHELL_SESSION, OWNER_HEX, "term-1")
            .expect("delete builder");
        let event = builder
            .sign_with_keys(&Keys::generate())
            .expect("sign tombstone");
        assert_eq!(event.kind.as_u16(), 5);
        let tags: Vec<Vec<String>> = event.tags.iter().map(|t| t.as_slice().to_vec()).collect();
        assert_eq!(
            tags,
            vec![vec!["a".to_string(), format!("30623:{OWNER_HEX}:term-1")]],
            "exactly one a tag, and no e tag — the relay refuses an event carrying both"
        );
    }

    /// A session id is interpolated straight into the coordinate, so the
    /// same traversal guard the roster commands use has to run here too.
    #[test]
    fn delete_rejects_a_session_id_that_could_escape_the_coordinate() {
        assert!(validate_session_id("../../etc/passwd").is_err());
        assert!(validate_session_id("term:1").is_err());
        assert!(validate_session_id("").is_err());
    }

    /// Deleting somebody else's terminal is the project-Owner case, and it
    /// has to address the coordinate to *their* key, not the caller's.
    #[test]
    fn delete_addresses_the_named_owner_not_the_caller() {
        let other = "b".repeat(64);
        let builder =
            build_delete_addressable(KIND_SHELL_SESSION, &other, "term-1").expect("builder");
        let event = builder.sign_with_keys(&Keys::generate()).expect("sign");
        let coord = event
            .tags
            .iter()
            .find_map(|t| (tag_name(t) == Some("a")).then(|| t.as_slice()[1].clone()));
        assert_eq!(coord, Some(format!("30623:{other}:term-1")));
    }

    #[test]
    fn shell_role_vocabulary_is_pinned() {
        assert!(validate_shell_role("collaborator").is_ok());
        assert!(validate_shell_role("viewer").is_ok());
        assert!(validate_shell_role("owner").is_err());
        assert!(validate_shell_role("").is_err());
    }

    #[test]
    fn expand_project_arg_expands_bare_slug_with_caller() {
        let coord = expand_project_arg("myproj", OWNER_HEX).unwrap();
        assert_eq!(coord, format!("30621:{OWNER_HEX}:myproj"));
    }

    #[test]
    fn expand_project_arg_passes_full_coordinate_and_rejects_garbage() {
        let full = format!("30621:{OWNER_HEX}:myproj");
        assert_eq!(expand_project_arg(&full, &"b".repeat(64)).unwrap(), full);
        assert!(expand_project_arg("30617:deadbeef:not-a-project", OWNER_HEX).is_err());
        assert!(expand_project_arg("", OWNER_HEX).is_err());
    }

    // ── Roster parsing ────────────────────────────────────────────────────────

    #[test]
    fn roster_reads_arity_four_tags_and_skips_unknown_roles() {
        let event = serde_json::json!({
            "tags": [
                ["d", "abc-123"],
                ["p", "c".repeat(64), "", "collaborator"],
                ["p", "d".repeat(64), "", "viewer"],
                ["p", "e".repeat(64), "", "owner"],
                ["p", "f".repeat(64)],
            ]
        });
        assert_eq!(
            roster_from_event_json(&event),
            vec![
                ("c".repeat(64), "collaborator".to_string()),
                ("d".repeat(64), "viewer".to_string()),
            ]
        );
    }

    // ── Announce rebuild ──────────────────────────────────────────────────────

    fn signed_announce(keys: &Keys, tags: Vec<Tag>, created_at: u64) -> Event {
        EventBuilder::new(Kind::Custom(KIND_SHELL_SESSION as u16), "")
            .tags(tags)
            .custom_created_at(Timestamp::from(created_at))
            .sign_with_keys(keys)
            .expect("sign")
    }

    fn base_tags(coord: &str) -> Vec<Tag> {
        vec![
            Tag::parse(["d", "abc-123"]).unwrap(),
            Tag::parse(["a", coord]).unwrap(),
            Tag::parse(["status", "open"]).unwrap(),
        ]
    }

    #[test]
    fn rebuild_announce_bumps_created_at_and_strips_auth() {
        let keys = Keys::generate();
        let coord = format!("30621:{OWNER_HEX}:myproj");
        let mut tags = base_tags(&coord);
        tags.push(Tag::parse(["auth", &"a".repeat(64), "kind=30623", &"b".repeat(128)]).unwrap());
        let head = signed_announce(&keys, tags.clone(), 1_700_000_000);

        let before = Timestamp::now().as_secs();
        let rebuilt = rebuild_announce(&head, tags)
            .unwrap()
            .sign_with_keys(&keys)
            .expect("sign");
        // Stamped now, not head + 1: a head older than the relay's ±15-minute
        // ingest window would otherwise be refused, and it always is.
        assert!(rebuilt.created_at.as_secs() >= before);
        assert!(rebuilt.created_at.as_secs() > head.created_at.as_secs());
        assert!(!rebuilt.tags.iter().any(|t| tag_name(t) == Some("auth")));
        // Non-roster tags survive verbatim.
        assert!(rebuilt.tags.iter().any(|t| tag_name(t) == Some("status")));
    }

    #[test]
    fn rebuild_announce_enforces_roster_cap() {
        let keys = Keys::generate();
        let coord = format!("30621:{OWNER_HEX}:myproj");
        let mut tags = base_tags(&coord);
        for i in 0..=SHELL_ROSTER_CAP {
            let pubkey = format!("{i:064x}");
            tags.push(Tag::parse(["p", &pubkey, "", "viewer"]).unwrap());
        }
        let head = signed_announce(&keys, base_tags(&coord), 1_700_000_000);
        assert!(rebuild_announce(&head, tags).is_err());
    }

    // ── Input chunking ────────────────────────────────────────────────────────

    #[test]
    fn input_under_the_chunk_limit_is_one_event() {
        let chunks = chunk_input_bytes(b"ls -la\n");
        assert_eq!(chunks.len(), 1);
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(&chunks[0])
                .unwrap(),
            b"ls -la\n"
        );
    }

    #[test]
    fn oversized_input_is_chunked_at_six_kib_and_reassembles() {
        let raw: Vec<u8> = (0..MAX_INPUT_CHUNK_RAW_BYTES * 2 + 17)
            .map(|i| (i % 251) as u8)
            .collect();
        let chunks = chunk_input_bytes(&raw);
        assert_eq!(chunks.len(), 3);
        // Every chunk's base64 stays within the relay's 8 KiB content cap.
        assert!(chunks.iter().all(|c| c.len() <= 8 * 1024));
        let reassembled: Vec<u8> = chunks
            .iter()
            .flat_map(|c| base64::engine::general_purpose::STANDARD.decode(c).unwrap())
            .collect();
        assert_eq!(reassembled, raw);
    }

    // ── No-network input validation ───────────────────────────────────────────

    fn discard_client() -> BuzzClient {
        BuzzClient::new("http://127.0.0.1:9".into(), Keys::generate(), None, None)
            .expect("client construction")
    }

    #[tokio::test]
    async fn invite_rejects_the_owner_before_any_network_call() {
        let client = discard_client();
        let own = client.keys().public_key().to_hex();
        let err = cmd_invite(&client, "abc-123", &own, "viewer")
            .await
            .expect_err("owner must not be invited");
        assert!(matches!(err, CliError::Usage(_)));
    }

    #[tokio::test]
    async fn send_input_refuses_empty_text_before_any_network_call() {
        let client = discard_client();
        let err = cmd_send_input(&client, "abc-123", OWNER_HEX, Some(""), false)
            .await
            .expect_err("empty input must fail");
        assert!(matches!(err, CliError::Usage(_)));
    }

    #[tokio::test]
    async fn send_input_rejects_uppercase_owner_before_any_network_call() {
        let client = discard_client();
        let upper = OWNER_HEX.to_uppercase();
        let err = cmd_send_input(&client, "abc-123", &upper, Some("ls"), false)
            .await
            .expect_err("uppercase owner must fail");
        assert!(matches!(err, CliError::Usage(_)));
    }
}
