//! NIP-SW: the surface watch (kind 24320) and surface frame (kind 24321)
//! pair shared by the session preview (SV-33) and the device surface (SV-34),
//! plus the helpers every shared-observation kind validates with.
//!
//! A `surface` tag says which surface an event is about (`preview` or
//! `device`) and `d` names the surface instance: the umbrella sessionRef for a
//! preview, the opaque 16-hex `sdv-slot` for a device
//! ([`crate::session_device::device_slot_id`]).
//!
//! Watching is shaped like NIP-ST: watchers send `watch` on mount and every
//! [`SURFACE_WATCH_KEEPALIVE_MS`], the producer expires a watcher after
//! [`SURFACE_WATCH_EXPIRY_MS`] and stops capturing when no watcher is live.
//! The relay is stateless about watchers; it delivers a watch only to its
//! `p` (the producer) and its author, and it accepts a frame only from the
//! announced producer of `(h, surface, d)`.
//!
//! **Host-local facts never travel** ([`refuse_host_local`]): every validator
//! in the four shared-observation modules refuses UUID-shaped runs (a
//! simulator UDID) and home, temp and simulator paths in tag values, so a
//! producer that forgets to redact is refused rather than published.
//! Producers redact first with [`redact_free_text`].

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use nostr::PublicKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::kind::{event_kind_u32, KIND_SURFACE_FRAME, KIND_SURFACE_WATCH};

/// `surface` tag value for the session preview (Browser surface).
pub const SURFACE_PREVIEW: &str = "preview";
/// `surface` tag value for the device surface.
pub const SURFACE_DEVICE: &str = "device";
/// Maximum UTF-8 byte length of a watch event's content.
pub const MAX_SURFACE_WATCH_CONTENT_BYTES: usize = 1024;
/// Maximum byte length of a frame event's base64 content (the relay cap).
pub const MAX_SURFACE_FRAME_CONTENT_BYTES: usize = 200 * 1024;
/// How often a watcher re-sends `watch` while the surface is on screen.
pub const SURFACE_WATCH_KEEPALIVE_MS: u64 = 15_000;
/// How long a producer keeps a watcher alive without a keepalive.
pub const SURFACE_WATCH_EXPIRY_MS: u64 = 45_000;
/// Allowed distance between an event's `created_at` and the relay clock.
pub const SURFACE_EVENT_FRESHNESS_SECS: u64 = 300;
/// Relay ceiling for watch events per (community, key) per second.
pub const SURFACE_WATCH_RATE_PER_SEC: u32 = 10;
/// Relay ceiling for frame events per (community, key) per second.
pub const SURFACE_FRAME_RATE_PER_SEC: u32 = 2;
/// Producer budget: frames per minute per surface, whatever the cadence.
pub const SURFACE_FRAME_MAX_PER_MIN: u32 = 20;
/// Producer budget: base64 bytes of one preview frame.
pub const PREVIEW_FRAME_BUDGET_BYTES: usize = 96 * 1024;
/// Producer budget: base cadence of preview frames.
pub const PREVIEW_FRAME_BASE_CADENCE_MS: u64 = 2_000;
/// Producer budget: longest edge of a preview frame, in pixels.
pub const PREVIEW_FRAME_MAX_LONG_EDGE: u32 = 1280;
/// Producer budget: base64 bytes of one device frame.
pub const DEVICE_FRAME_BUDGET_BYTES: usize = 200 * 1024;
/// Producer budget: base cadence of device frames.
pub const DEVICE_FRAME_BASE_CADENCE_MS: u64 = 3_000;
/// Producer budget: longest edge of a device frame, in pixels.
pub const DEVICE_FRAME_MAX_LONG_EDGE: u32 = 900;
/// Smallest `cadence-ms` a frame may declare.
pub const MIN_FRAME_CADENCE_MS: u64 = 500;
/// Largest `cadence-ms` a frame may declare.
pub const MAX_FRAME_CADENCE_MS: u64 = 60_000;
/// How long the relay may reuse a resolved frame authority.
pub const SURFACE_AUTHORITY_CACHE_SECS: u64 = 10;
/// Producer rule: at most one `snapshot` request honoured per watcher per
/// this interval.
pub const SURFACE_SNAPSHOT_REQUEST_MIN_INTERVAL_MS: u64 = 10_000;
/// Largest pixel edge a `dim`/`viewport` value may carry.
pub const MAX_SURFACE_DIMENSION: u32 = 8192;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const DEVICE_SLOT_HEX_LEN: usize = 16;

/// Path fragments that only ever name a place on one machine.
const HOST_LOCAL_FRAGMENTS: [&str; 7] = [
    "/Users/",
    "/home/",
    "/var/folders/",
    "/private/var/",
    "DerivedData",
    "CoreSimulator",
    "file://",
];

/// Which surface an observation event is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    /// The session's Browser preview; `d` is the umbrella sessionRef.
    Preview,
    /// The session's device; `d` is the opaque `sdv-slot`.
    Device,
}

impl Surface {
    /// The exact `surface` tag value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preview => SURFACE_PREVIEW,
            Self::Device => SURFACE_DEVICE,
        }
    }

    /// Parse a `surface` tag value; anything but the two known values is `None`.
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            SURFACE_PREVIEW => Some(Self::Preview),
            SURFACE_DEVICE => Some(Self::Device),
            _ => None,
        }
    }
}

/// Validate a surface instance key (`d`): a lowercase canonical UUID
/// sessionRef for a preview, 16 lowercase hex for a device slot.
pub fn validate_surface_key(surface: Surface, key: &str) -> Result<(), String> {
    match surface {
        Surface::Preview => validate_canonical_uuid(key, "preview d (sessionRef)"),
        Surface::Device => {
            if key.len() == DEVICE_SLOT_HEX_LEN && is_lower_hex(key) {
                Ok(())
            } else {
                Err("device d must be the 16-hex sdv-slot".into())
            }
        }
    }
}

/// Refuse a value that carries a host-local fact: a UUID-shaped run (a
/// simulator UDID, in either case) or a home, temp, simulator or `file://`
/// path. `field` names the value in the error.
pub fn refuse_host_local(field: &str, value: &str) -> Result<(), String> {
    if contains_uuid_shaped_run(value) {
        return Err(format!(
            "{field} carries a UUID-shaped value; host-local identifiers never go on the relay"
        ));
    }
    refuse_host_local_path(field, value)
}

/// Whether `value` contains an 8-4-4-4-12 hex run, in either case.
pub fn contains_uuid_shaped_run(value: &str) -> bool {
    uuid_shaped_runs(value).next().is_some()
}

/// Redact free text before signing: UUID-shaped runs become `…`, and a
/// host-local path is reduced to `~/…/<last segment>`. Producers run titles,
/// alt text and reasons through this so [`refuse_host_local`] never fires on
/// honest input.
pub fn redact_free_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut cursor = 0;
    for (start, end) in uuid_shaped_runs(value).collect::<Vec<_>>() {
        out.push_str(&value[cursor..start]);
        out.push('…');
        cursor = end;
    }
    out.push_str(&value[cursor..]);
    out.split(' ')
        .map(redact_path_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_path_word(word: &str) -> String {
    if HOST_LOCAL_FRAGMENTS
        .iter()
        .any(|fragment| word.contains(fragment))
    {
        let last = word
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default();
        format!("~/…/{last}")
    } else {
        word.to_owned()
    }
}

/// Byte ranges of every UUID-shaped run in `value`.
fn uuid_shaped_runs(value: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    const LEN: usize = 36;
    let bytes = value.as_bytes();
    let mut index = 0;
    std::iter::from_fn(move || {
        while index + LEN <= bytes.len() {
            let start = index;
            index += 1;
            let mut at = start;
            let mut ok = true;
            for (group, width) in GROUPS.iter().enumerate() {
                if group > 0 {
                    if bytes[at] != b'-' {
                        ok = false;
                        break;
                    }
                    at += 1;
                }
                if !bytes[at..at + width].iter().all(u8::is_ascii_hexdigit) {
                    ok = false;
                    break;
                }
                at += width;
            }
            if ok {
                index = start + LEN;
                return Some((start, start + LEN));
            }
        }
        None
    })
}

/// Validate a lowercase canonical hyphenated UUID.
pub(crate) fn validate_canonical_uuid(value: &str, field: &str) -> Result<(), String> {
    match Uuid::parse_str(value) {
        Ok(parsed) if parsed.to_string() == value => Ok(()),
        _ => Err(format!("{field} must be a lowercase canonical UUID")),
    }
}

/// Parse an `h` channel tag value.
pub(crate) fn parse_channel(value: &str) -> Result<Uuid, String> {
    validate_canonical_uuid(value, "h")?;
    Uuid::parse_str(value).map_err(|_| "h must be a lowercase canonical UUID".into())
}

/// Whether every byte is a lowercase hex digit (and there is at least one).
pub(crate) fn is_lower_hex(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Parse a 64-lowercase-hex pubkey.
pub(crate) fn parse_pubkey(value: &str, field: &str) -> Result<PublicKey, String> {
    if value.len() != 64 || !is_lower_hex(value) {
        return Err(format!("{field} must be a 64-hex pubkey"));
    }
    PublicKey::from_hex(value).map_err(|_| format!("{field} is not a valid pubkey"))
}

/// Require `len` lowercase hex digits.
pub(crate) fn require_hex(value: &str, len: usize, field: &str) -> Result<(), String> {
    if value.len() == len && is_lower_hex(value) {
        Ok(())
    } else {
        Err(format!("{field} must be {len} lowercase hex digits"))
    }
}

/// Parse a canonical decimal integer (no sign, no leading zeros) ≤ 2^53−1.
pub(crate) fn parse_decimal(value: &str, field: &str) -> Result<u64, String> {
    let canonical = !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'));
    let parsed = if canonical {
        value.parse::<u64>().ok()
    } else {
        None
    };
    match parsed {
        Some(number) if number <= MAX_SAFE_INTEGER => Ok(number),
        _ => Err(format!("{field} must be a canonical decimal integer")),
    }
}

/// Parse a `WxH` dimension, each side 1..=[`MAX_SURFACE_DIMENSION`].
pub fn parse_dimensions(value: &str, field: &str) -> Result<(u32, u32), String> {
    let malformed = || format!("{field} must be WxH");
    let (width, height) = value.split_once('x').ok_or_else(malformed)?;
    let width = parse_decimal(width, field).map_err(|_| malformed())?;
    let height = parse_decimal(height, field).map_err(|_| malformed())?;
    let bound = u64::from(MAX_SURFACE_DIMENSION);
    if width == 0 || height == 0 || width > bound || height > bound {
        return Err(format!("{field} sides must be 1..={MAX_SURFACE_DIMENSION}"));
    }
    // Both sides are bounded by MAX_SURFACE_DIMENSION, which fits in u32.
    Ok((width as u32, height as u32))
}

/// Refuse an event whose `created_at` is more than
/// [`SURFACE_EVENT_FRESHNESS_SECS`] from `now` (both Unix seconds).
pub fn check_surface_freshness(created_at: u64, now: u64) -> Result<(), String> {
    if created_at.abs_diff(now) > SURFACE_EVENT_FRESHNESS_SECS {
        Err("surface event timestamp outside the ±5 minute freshness window".into())
    } else {
        Ok(())
    }
}

/// One position in an exact ordered tag list.
pub(crate) struct TagSlot {
    /// Tag name.
    pub name: &'static str,
    /// Whether the tag must be present.
    pub required: bool,
    /// Exact number of fields, name included.
    pub arity: usize,
}

impl TagSlot {
    pub(crate) const fn required(name: &'static str) -> Self {
        Self {
            name,
            required: true,
            arity: 2,
        }
    }

    pub(crate) const fn optional(name: &'static str) -> Self {
        Self {
            name,
            required: false,
            arity: 2,
        }
    }

    pub(crate) const fn optional_arity(name: &'static str, arity: usize) -> Self {
        Self {
            name,
            required: false,
            arity,
        }
    }
}

/// Match `tags` against an exact ordered layout: each slot appears at most
/// once, in order, optional slots may be skipped, and nothing else may
/// appear. Returns one entry per slot.
pub(crate) fn match_ordered_tags<'a>(
    tags: &[&'a [String]],
    layout: &[TagSlot],
    label: &str,
) -> Result<Vec<Option<&'a [String]>>, String> {
    let mut matched = Vec::with_capacity(layout.len());
    let mut index = 0;
    for slot in layout {
        match tags.get(index) {
            Some(tag) if tag.first().map(String::as_str) == Some(slot.name) => {
                if tag.len() != slot.arity {
                    return Err(format!(
                        "{label} tag {} must have exactly {} fields",
                        slot.name, slot.arity
                    ));
                }
                matched.push(Some(*tag));
                index += 1;
            }
            _ if slot.required => {
                return Err(format!(
                    "{label} tag {} is missing or out of order",
                    slot.name
                ));
            }
            _ => matched.push(None),
        }
    }
    if index != tags.len() {
        return Err(format!(
            "{label} carries an unknown, duplicate or out-of-order tag"
        ));
    }
    Ok(matched)
}

/// Run the host-local rule over every tag value. Tags named in `uuid_exempt`
/// skip only the UUID-shape half of it: the `h` channel, a sessionRef `d`, and
/// identifiers minted by Beekeeper or a runtime (`cs-target` embeds a runtime
/// session id, which is often a UUID). Path fragments are refused everywhere.
pub(crate) fn refuse_host_local_tags(
    tags: &[&[String]],
    uuid_exempt: &[&str],
) -> Result<(), String> {
    for tag in tags {
        let Some(name) = tag.first() else { continue };
        let exempt = uuid_exempt.contains(&name.as_str());
        for value in tag.iter().skip(1) {
            if exempt {
                refuse_host_local_path(name, value)?;
            } else {
                refuse_host_local(name, value)?;
            }
        }
    }
    Ok(())
}

/// The path half of [`refuse_host_local`].
pub(crate) fn refuse_host_local_path(field: &str, value: &str) -> Result<(), String> {
    if let Some(fragment) = HOST_LOCAL_FRAGMENTS
        .iter()
        .find(|fragment| value.contains(**fragment))
    {
        return Err(format!(
            "{field} carries a host-local path ({fragment}); paths never go on the relay"
        ));
    }
    Ok(())
}

/// The value of a matched slot (field 1).
pub(crate) fn slot_value(slot: Option<&[String]>) -> Option<&str> {
    slot.and_then(|tag| tag.get(1)).map(String::as_str)
}

/// The value of a matched required slot.
pub(crate) fn required_value<'a>(slots: &[Option<&'a [String]>], index: usize) -> &'a str {
    slot_value(slots.get(index).copied().flatten()).unwrap_or_default()
}

fn tag_rows(rows: &[(&str, &str)]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|(name, value)| vec![(*name).to_owned(), (*value).to_owned()])
        .collect()
}

/// What a watcher asks the producer for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceWatchAction {
    /// Start or keep watching (sent on mount and every keepalive).
    Watch,
    /// Stop watching now.
    Stop,
    /// Send a full frame now.
    Resync,
    /// Publish one fresh 44253 snapshot naming this watcher.
    Snapshot,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WatchContent {
    action: SurfaceWatchAction,
}

/// A structurally valid kind 24320 surface watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceWatch {
    /// The session channel (`h`).
    pub channel_id: Uuid,
    /// Which surface.
    pub surface: Surface,
    /// The surface instance key (`d`).
    pub key: String,
    /// The producer the watch is addressed to (`p`).
    pub producer: PublicKey,
    /// What the watcher asks for.
    pub action: SurfaceWatchAction,
}

const WATCH_LAYOUT: [TagSlot; 4] = [
    TagSlot::required("h"),
    TagSlot::required("surface"),
    TagSlot::required("d"),
    TagSlot::required("p"),
];

impl SurfaceWatch {
    /// The exact ordered tags of this watch.
    pub fn tags(&self) -> Vec<Vec<String>> {
        tag_rows(&[
            ("h", &self.channel_id.to_string()),
            ("surface", self.surface.as_str()),
            ("d", &self.key),
            ("p", &self.producer.to_hex()),
        ])
    }

    /// The exact content of this watch.
    pub fn content(&self) -> String {
        serde_json::json!({ "action": self.action }).to_string()
    }
}

/// Validate a kind 24320 watch from its tags and content.
pub fn validate_surface_watch_parts(
    tags: &[&[String]],
    content: &str,
) -> Result<SurfaceWatch, String> {
    if content.len() > MAX_SURFACE_WATCH_CONTENT_BYTES {
        return Err(format!(
            "surface watch content exceeds {MAX_SURFACE_WATCH_CONTENT_BYTES} bytes"
        ));
    }
    let slots = match_ordered_tags(tags, &WATCH_LAYOUT, "surface watch")?;
    let (channel_id, surface, key) = parse_surface_head(&slots, tags)?;
    let producer = parse_pubkey(required_value(&slots, 3), "surface watch p")?;
    let parsed: WatchContent = serde_json::from_str(content)
        .map_err(|_| "surface watch content must be {\"action\": watch|stop|resync|snapshot}")?;
    Ok(SurfaceWatch {
        channel_id,
        surface,
        key,
        producer,
        action: parsed.action,
    })
}

/// [`validate_surface_watch_parts`] over a signed event, checking its kind.
pub fn validate_surface_watch_envelope(event: &nostr::Event) -> Result<SurfaceWatch, String> {
    if event_kind_u32(event) != KIND_SURFACE_WATCH {
        return Err("event is not a surface watch (kind 24320)".into());
    }
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    validate_surface_watch_parts(&tags, &event.content)
}

/// Parse the shared `h`, `surface`, `d` head of a watch or frame (slots 0–2)
/// and apply the host-local rule to every other tag.
fn parse_surface_head(
    slots: &[Option<&[String]>],
    tags: &[&[String]],
) -> Result<(Uuid, Surface, String), String> {
    let channel_id = parse_channel(required_value(slots, 0))?;
    let surface = Surface::from_wire(required_value(slots, 1))
        .ok_or_else(|| "surface must be preview or device".to_owned())?;
    let key = required_value(slots, 2);
    validate_surface_key(surface, key)?;
    let exempt: &[&str] = match surface {
        Surface::Preview => &["h", "d"],
        Surface::Device => &["h"],
    };
    refuse_host_local_tags(tags, exempt)?;
    Ok((channel_id, surface, key.to_owned()))
}

/// What one frame event carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceFrameType {
    /// A picture: content is a base64 JPEG.
    Frame,
    /// Sharing is paused (hidden, or turned off); content is empty.
    Paused,
    /// The surface closed; content is empty.
    End,
}

impl SurfaceFrameType {
    /// The exact `t` tag value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Frame => "frame",
            Self::Paused => "paused",
            Self::End => "end",
        }
    }

    fn from_wire(value: &str) -> Option<Self> {
        match value {
            "frame" => Some(Self::Frame),
            "paused" => Some(Self::Paused),
            "end" => Some(Self::End),
            _ => None,
        }
    }
}

/// The validated header of a kind 24321 surface frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceFrameHeader {
    /// The session channel (`h`).
    pub channel_id: Uuid,
    /// Which surface.
    pub surface: Surface,
    /// The surface instance key (`d`).
    pub key: String,
    /// `t`.
    pub frame_type: SurfaceFrameType,
    /// `seq`, strictly increasing within an epoch.
    pub seq: u64,
    /// `epoch`; a new epoch resets `seq`.
    pub epoch: u64,
    /// `cadence-ms`, the pacing in force (the UI says it).
    pub cadence_ms: u64,
    /// `dim` width in pixels.
    pub width: u32,
    /// `dim` height in pixels.
    pub height: u32,
    /// `captured-at`, Unix milliseconds.
    pub captured_at_ms: u64,
    /// `actor`: who sent input in the last five seconds, if anyone.
    pub actor: Option<PublicKey>,
    /// `commit`: the 40-hex working-tree head, when the producer knows it.
    pub commit: Option<String>,
}

const FRAME_LAYOUT: [TagSlot; 11] = [
    TagSlot::required("h"),
    TagSlot::required("surface"),
    TagSlot::required("d"),
    TagSlot::required("t"),
    TagSlot::required("seq"),
    TagSlot::required("epoch"),
    TagSlot::required("cadence-ms"),
    TagSlot::required("dim"),
    TagSlot::required("captured-at"),
    TagSlot::optional("actor"),
    TagSlot::optional("commit"),
];

impl SurfaceFrameHeader {
    /// The exact ordered tags of this frame.
    pub fn tags(&self) -> Vec<Vec<String>> {
        let mut rows = tag_rows(&[
            ("h", &self.channel_id.to_string()),
            ("surface", self.surface.as_str()),
            ("d", &self.key),
            ("t", self.frame_type.as_str()),
            ("seq", &self.seq.to_string()),
            ("epoch", &self.epoch.to_string()),
            ("cadence-ms", &self.cadence_ms.to_string()),
            ("dim", &format!("{}x{}", self.width, self.height)),
            ("captured-at", &self.captured_at_ms.to_string()),
        ]);
        if let Some(actor) = &self.actor {
            rows.push(vec!["actor".into(), actor.to_hex()]);
        }
        if let Some(commit) = &self.commit {
            rows.push(vec!["commit".into(), commit.clone()]);
        }
        rows
    }
}

/// Validate a kind 24321 frame from its tags and content.
///
/// A `frame` must carry standard padded base64 of a JPEG (it begins `/9j/`),
/// 1..=[`MAX_SURFACE_FRAME_CONTENT_BYTES`]; `paused` and `end` carry nothing.
pub fn validate_surface_frame_parts(
    tags: &[&[String]],
    content: &str,
) -> Result<SurfaceFrameHeader, String> {
    if content.len() > MAX_SURFACE_FRAME_CONTENT_BYTES {
        return Err(format!(
            "surface frame content exceeds {MAX_SURFACE_FRAME_CONTENT_BYTES} bytes"
        ));
    }
    let slots = match_ordered_tags(tags, &FRAME_LAYOUT, "surface frame")?;
    let (channel_id, surface, key) = parse_surface_head(&slots, tags)?;
    let frame_type = SurfaceFrameType::from_wire(required_value(&slots, 3))
        .ok_or_else(|| "surface frame t must be frame, paused or end".to_owned())?;
    let seq = parse_decimal(required_value(&slots, 4), "seq")?;
    let epoch = parse_decimal(required_value(&slots, 5), "epoch")?;
    let cadence_ms = parse_decimal(required_value(&slots, 6), "cadence-ms")?;
    if !(MIN_FRAME_CADENCE_MS..=MAX_FRAME_CADENCE_MS).contains(&cadence_ms) {
        return Err(format!(
            "cadence-ms must be {MIN_FRAME_CADENCE_MS}..={MAX_FRAME_CADENCE_MS}"
        ));
    }
    let (width, height) = parse_dimensions(required_value(&slots, 7), "dim")?;
    let captured_at_ms = parse_decimal(required_value(&slots, 8), "captured-at")?;
    let actor = slot_value(slots[9])
        .map(|value| parse_pubkey(value, "actor"))
        .transpose()?;
    let commit = slot_value(slots[10])
        .map(|value| require_hex(value, 40, "commit").map(|()| value.to_owned()))
        .transpose()?;
    match frame_type {
        SurfaceFrameType::Frame => validate_jpeg_base64(content)?,
        SurfaceFrameType::Paused | SurfaceFrameType::End if !content.is_empty() => {
            return Err("paused and end frames carry no content".into());
        }
        SurfaceFrameType::Paused | SurfaceFrameType::End => {}
    }
    Ok(SurfaceFrameHeader {
        channel_id,
        surface,
        key,
        frame_type,
        seq,
        epoch,
        cadence_ms,
        width,
        height,
        captured_at_ms,
        actor,
        commit,
    })
}

/// [`validate_surface_frame_parts`] over a signed event, checking its kind.
pub fn validate_surface_frame_envelope(event: &nostr::Event) -> Result<SurfaceFrameHeader, String> {
    if event_kind_u32(event) != KIND_SURFACE_FRAME {
        return Err("event is not a surface frame (kind 24321)".into());
    }
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    validate_surface_frame_parts(&tags, &event.content)
}

/// Encode JPEG bytes as frame content (standard padded base64).
pub fn encode_frame_content(jpeg: &[u8]) -> String {
    STANDARD.encode(jpeg)
}

fn validate_jpeg_base64(content: &str) -> Result<(), String> {
    // "/9j/" is the base64 of the JPEG start-of-image marker FF D8 FF.
    let shaped = content.starts_with("/9j/")
        && content.len().is_multiple_of(4)
        && content
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='));
    if shaped {
        Ok(())
    } else {
        Err("surface frame content must be standard base64 of a JPEG".into())
    }
}

#[cfg(test)]
#[path = "surface_watch_tests.rs"]
mod tests;
