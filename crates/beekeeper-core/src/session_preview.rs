//! NIP-SP: the session preview announce (kind 30626), the `page` redaction
//! rule every preview kind shares, and the owner fold.
//!
//! A preview runs in the desktop app on the machine running the agent. That
//! desktop announces it with an addressable 30626 (`d` = the umbrella
//! sessionRef), republished on open, close, share toggle and origin change.
//! **The earliest open announce with no later close from its own signer owns
//! the preview** ([`resolve_preview_owner`]); the relay uses the same fold to
//! decide whose 24321 frames to accept for `surface=preview`.
//!
//! The full URL never leaves the machine: a loopback or `file:` page travels
//! as `local:` plus its path, a public page as origin plus path, never with a
//! query, fragment, port or credentials ([`redact_page_url`],
//! [`validate_page`]).

use nostr::{Event, PublicKey};
use uuid::Uuid;

use crate::coding_session_title::parse_coding_session_target_key;
use crate::kind::{event_kind_u32, KIND_SESSION_PREVIEW_ANNOUNCE};
use crate::surface_watch::{
    match_ordered_tags, parse_channel, parse_dimensions, parse_pubkey, refuse_host_local,
    refuse_host_local_tags, required_value, slot_value, validate_canonical_uuid, TagSlot,
};

/// Exact `spa-v` tag value.
pub const SESSION_PREVIEW_TAG_VERSION: &str = "1";
/// Maximum UTF-8 byte length of a preview `title`.
pub const MAX_PREVIEW_TITLE_BYTES: usize = 200;
/// Producer rule: republish an announce at most this often.
pub const PREVIEW_ANNOUNCE_MIN_INTERVAL_SECS: u64 = 5;
/// Maximum byte length of a `page` value.
pub const MAX_PREVIEW_PAGE_BYTES: usize = 1024;
/// Prefix of a redacted loopback or `file:` page.
pub const LOCAL_PAGE_PREFIX: &str = "local:";

/// Whether the preview is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewStatus {
    /// The preview is open on the announcing machine.
    Open,
    /// The preview was closed.
    Closed,
}

impl PreviewStatus {
    /// The exact `status` tag value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}

/// How remote viewers can see the preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewStream {
    /// Live 24321 frames while someone watches.
    Frames,
    /// Dated 44253 snapshots only.
    Snapshots,
}

impl PreviewStream {
    /// The exact `stream` tag value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Frames => "frames",
            Self::Snapshots => "snapshots",
        }
    }
}

/// A structurally valid kind 30626 announce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPreviewAnnounce {
    /// The session channel (`h`).
    pub channel_id: Uuid,
    /// The umbrella sessionRef (`d`).
    pub session_ref: String,
    /// `status`.
    pub status: PreviewStatus,
    /// `cs-target`: the execution that opened it, absent when a person did.
    pub target_key: Option<String>,
    /// `provider`: that machine's provider/host key, when known.
    pub provider: Option<PublicKey>,
    /// `page`, already redacted. Present on an open announce, absent on a
    /// closed one: a close says only that the preview closed.
    pub page: Option<String>,
    /// `title` (may be empty). Present on an open announce, absent on a
    /// closed one.
    pub title: Option<String>,
    /// `viewport` width.
    pub viewport_width: u32,
    /// `viewport` height.
    pub viewport_height: u32,
    /// `stream`.
    pub stream: PreviewStream,
}

const ANNOUNCE_LAYOUT: [TagSlot; 11] = [
    TagSlot::required("h"),
    TagSlot::required("d"),
    TagSlot::required("spa-v"),
    TagSlot::required("status"),
    TagSlot::optional("cs-target"),
    TagSlot::optional("provider"),
    // Required on `open`, forbidden on `closed` (checked after matching).
    TagSlot::optional("page"),
    TagSlot::optional("title"),
    TagSlot::required("viewport"),
    TagSlot::required("stream"),
    TagSlot::required("input"),
];

impl SessionPreviewAnnounce {
    /// The exact ordered tags of this announce.
    pub fn tags(&self) -> Vec<Vec<String>> {
        let mut rows: Vec<Vec<String>> = vec![
            vec!["h".into(), self.channel_id.to_string()],
            vec!["d".into(), self.session_ref.clone()],
            vec!["spa-v".into(), SESSION_PREVIEW_TAG_VERSION.into()],
            vec!["status".into(), self.status.as_str().into()],
        ];
        if let Some(target) = &self.target_key {
            rows.push(vec!["cs-target".into(), target.clone()]);
        }
        if let Some(provider) = &self.provider {
            rows.push(vec!["provider".into(), provider.to_hex()]);
        }
        if let Some(page) = &self.page {
            rows.push(vec!["page".into(), page.clone()]);
        }
        if let Some(title) = &self.title {
            rows.push(vec!["title".into(), title.clone()]);
        }
        rows.push(vec![
            "viewport".into(),
            format!("{}x{}", self.viewport_width, self.viewport_height),
        ]);
        rows.push(vec!["stream".into(), self.stream.as_str().into()]);
        rows.push(vec!["input".into(), "synthetic".into()]);
        rows
    }
}

/// Validate a kind 30626 announce from its tags and content (which must be
/// empty).
pub fn validate_session_preview_announce_parts(
    tags: &[&[String]],
    content: &str,
) -> Result<SessionPreviewAnnounce, String> {
    if !content.is_empty() {
        return Err("preview announce content must be empty".into());
    }
    let slots = match_ordered_tags(tags, &ANNOUNCE_LAYOUT, "preview announce")?;
    refuse_host_local_tags(tags, &["h", "d", "cs-target"])?;
    let channel_id = parse_channel(required_value(&slots, 0))?;
    let session_ref = required_value(&slots, 1);
    validate_canonical_uuid(session_ref, "preview announce d (sessionRef)")?;
    if required_value(&slots, 2) != SESSION_PREVIEW_TAG_VERSION {
        return Err("unsupported preview announce spa-v".into());
    }
    let status = match required_value(&slots, 3) {
        "open" => PreviewStatus::Open,
        "closed" => PreviewStatus::Closed,
        _ => return Err("preview announce status must be open or closed".into()),
    };
    let target_key = slot_value(slots[4])
        .map(|key| parse_coding_session_target_key(key).map(|_| key.to_owned()))
        .transpose()?;
    let provider = slot_value(slots[5])
        .map(|value| parse_pubkey(value, "provider"))
        .transpose()?;
    let page = slot_value(slots[6]);
    let title = slot_value(slots[7]);
    match status {
        PreviewStatus::Open => {
            validate_page(page.ok_or("an open preview announce needs a page")?)?;
            validate_preview_title(title.ok_or("an open preview announce needs a title")?)?;
        }
        PreviewStatus::Closed => {
            if page.is_some() || title.is_some() {
                return Err("a closed preview announce carries no page or title".into());
            }
        }
    }
    let (viewport_width, viewport_height) =
        parse_dimensions(required_value(&slots, 8), "viewport")?;
    let stream = match required_value(&slots, 9) {
        "frames" => PreviewStream::Frames,
        "snapshots" => PreviewStream::Snapshots,
        _ => return Err("preview announce stream must be frames or snapshots".into()),
    };
    if required_value(&slots, 10) != "synthetic" {
        return Err("preview announce input must be synthetic".into());
    }
    Ok(SessionPreviewAnnounce {
        channel_id,
        session_ref: session_ref.to_owned(),
        status,
        target_key,
        provider,
        page: page.map(str::to_owned),
        title: title.map(str::to_owned),
        viewport_width,
        viewport_height,
        stream,
    })
}

/// [`validate_session_preview_announce_parts`] over a signed event, checking
/// its kind.
pub fn validate_session_preview_announce_envelope(
    event: &Event,
) -> Result<SessionPreviewAnnounce, String> {
    if event_kind_u32(event) != KIND_SESSION_PREVIEW_ANNOUNCE {
        return Err("event is not a preview announce (kind 30626)".into());
    }
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    validate_session_preview_announce_parts(&tags, &event.content)
}

/// Validate a preview `title`: one line, at most [`MAX_PREVIEW_TITLE_BYTES`],
/// no host-local facts. Empty is allowed.
pub fn validate_preview_title(title: &str) -> Result<(), String> {
    if title.len() > MAX_PREVIEW_TITLE_BYTES {
        return Err(format!(
            "preview title exceeds {MAX_PREVIEW_TITLE_BYTES} bytes"
        ));
    }
    if title.chars().any(char::is_control) {
        return Err("preview title must be one line of text".into());
    }
    refuse_host_local("title", title)
}

/// Validate a redacted `page` value: `local:/<path>` with no host, port,
/// query or fragment, or `http(s)://<public host>[/<path>]` with no port,
/// credentials, query or fragment.
pub fn validate_page(page: &str) -> Result<(), String> {
    if page.is_empty() || page.len() > MAX_PREVIEW_PAGE_BYTES {
        return Err(format!("page must be 1..={MAX_PREVIEW_PAGE_BYTES} bytes"));
    }
    if page.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("page must not contain whitespace or control characters".into());
    }
    if page.contains('?') || page.contains('#') {
        return Err("page must not carry a query or fragment".into());
    }
    refuse_host_local("page", page)?;
    if let Some(path) = page.strip_prefix(LOCAL_PAGE_PREFIX) {
        if !path.starts_with('/') || path.starts_with("//") {
            return Err("a local page is local: followed by an absolute path only".into());
        }
        return Ok(());
    }
    let rest = page
        .strip_prefix("https://")
        .or_else(|| page.strip_prefix("http://"))
        .ok_or_else(|| "page must be local:/<path> or an http(s) origin and path".to_owned())?;
    let host = rest.split('/').next().unwrap_or_default();
    if host.is_empty() || host.contains('@') || host.contains(':') {
        return Err("page host must be a bare public host (no port or credentials)".into());
    }
    if is_private_host(host) {
        return Err("a loopback or private host travels as local:/<path>".into());
    }
    Ok(())
}

/// Redact a full preview URL to the `page` value that may travel, or `None`
/// when nothing safe remains (an unparseable URL, or a non-web scheme).
///
/// Loopback, private-network and `file:` pages become `local:` plus the path
/// (for `file:`, only the last path segment); public pages keep scheme, host
/// and path. Query, fragment, port and credentials are always dropped.
pub fn redact_page_url(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    let redacted = match url.scheme() {
        "file" => {
            let last = url
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|segment| !segment.is_empty())
                .unwrap_or("index.html");
            format!("{LOCAL_PAGE_PREFIX}/{last}")
        }
        "http" | "https" => {
            let host = url.host_str()?.to_ascii_lowercase();
            let host = host.trim_start_matches('[').trim_end_matches(']');
            if is_private_host(host) {
                format!("{LOCAL_PAGE_PREFIX}{}", url.path())
            } else {
                format!("{}://{}{}", url.scheme(), host, url.path())
            }
        }
        _ => return None,
    };
    validate_page(&redacted).ok().map(|()| redacted)
}

fn is_private_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    if host == "::1" || host == "0.0.0.0" {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        return ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified();
    }
    if let Ok(ip) = host.parse::<std::net::Ipv6Addr>() {
        return ip.is_loopback() || ip.is_unspecified() || (ip.segments()[0] & 0xfe00) == 0xfc00;
    }
    false
}

/// Fold the 30626 announces of one `(h, d)` into the preview's owner.
///
/// Each signer's newest valid announce for the pair counts; among those that
/// are `open`, the earliest `created_at` wins (tie: the lower event id).
/// Announces for another channel or sessionRef, and invalid ones, are
/// ignored. `None` means nobody holds the preview open.
pub fn resolve_preview_owner(
    announces: &[Event],
    channel_id: Uuid,
    session_ref: &str,
) -> Option<PublicKey> {
    let mut newest: std::collections::BTreeMap<PublicKey, (&Event, PreviewStatus)> =
        std::collections::BTreeMap::new();
    for event in announces {
        let Ok(announce) = validate_session_preview_announce_envelope(event) else {
            continue;
        };
        if announce.channel_id != channel_id || announce.session_ref != session_ref {
            continue;
        }
        let replace = newest.get(&event.pubkey).is_none_or(|(current, _)| {
            (event.created_at, std::cmp::Reverse(event.id))
                > (current.created_at, std::cmp::Reverse(current.id))
        });
        if replace {
            newest.insert(event.pubkey, (event, announce.status));
        }
    }
    newest
        .values()
        .filter(|(_, status)| *status == PreviewStatus::Open)
        .min_by_key(|(event, _)| (event.created_at, event.id))
        .map(|(event, _)| event.pubkey)
}

#[cfg(test)]
#[path = "session_preview_tests.rs"]
mod tests;
