//! NIP-SW § Snapshot: the surface snapshot record (kind 44253), one record for
//! both the preview and the device surface.
//!
//! A snapshot is a dated, durable picture on relay media: who took it (the
//! signer — the capturing producer), on which machine (`provider`, a key,
//! never a hostname), when (`taken-at`), at whose request (`p` marker
//! `requested-by`), and at which commit (`commit`, present only when the
//! producer knows it; readers say "commit not recorded" otherwise). One card
//! renderer, one fold and one verdict token serve both surfaces:
//! `snapshot:<44253 id>`, with `preview:<id>` accepted as an alias
//! ([`parse_snapshot_evidence_token`]).
//!
//! The relay validates structure and the host-local rule only; whether the
//! signer was the surface's producer is the reader's check.

use nostr::{Event, EventId, PublicKey};
use uuid::Uuid;

use crate::kind::{event_kind_u32, KIND_SURFACE_SNAPSHOT};
use crate::session_preview::{validate_page, validate_preview_title};
use crate::surface_watch::{
    match_ordered_tags, parse_channel, parse_decimal, parse_dimensions, parse_pubkey,
    refuse_host_local, refuse_host_local_tags, require_hex, required_value, slot_value,
    validate_surface_key, Surface, TagSlot,
};

/// Exact `ssn-v` tag value.
pub const SURFACE_SNAPSHOT_TAG_VERSION: &str = "1";
/// Maximum UTF-8 byte length of a snapshot's alt-text content.
pub const MAX_SURFACE_SNAPSHOT_ALT_BYTES: usize = 1024;
/// Maximum byte length of the `url` tag.
pub const MAX_SURFACE_SNAPSHOT_URL_BYTES: usize = 1024;
/// Verdict evidence token prefix for a snapshot.
pub const SNAPSHOT_EVIDENCE_PREFIX: &str = "snapshot:";
/// Accepted alias of [`SNAPSHOT_EVIDENCE_PREFIX`] (the preview spec's form).
pub const SNAPSHOT_EVIDENCE_ALIAS_PREFIX: &str = "preview:";
/// Image types a snapshot may point at.
pub const SURFACE_SNAPSHOT_MIMES: [&str; 3] = ["image/png", "image/jpeg", "image/webp"];

/// Why the snapshot exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceSnapshotType {
    /// A plain snapshot.
    Snapshot,
    /// The image of an annotation steer (`e` marker `annotation`).
    Annotation,
}

impl SurfaceSnapshotType {
    /// The exact `ssn-type` tag value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Snapshot => "snapshot",
            Self::Annotation => "annotation",
        }
    }
}

/// What the optional `e` tag references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotReferenceMarker {
    /// The 44254 device command this snapshot answers.
    Command,
    /// The 44220 annotation steer this image belongs to.
    Annotation,
}

impl SnapshotReferenceMarker {
    /// The exact marker (field 4 of the `e` tag).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Annotation => "annotation",
        }
    }
}

/// The working-tree commit a snapshot was taken at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotCommit {
    /// 40 lowercase hex.
    pub sha: String,
    /// Whether the working tree had uncommitted changes.
    pub dirty: bool,
}

/// A structurally valid kind 44253 surface snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceSnapshot {
    /// The session channel (`h`).
    pub channel_id: Uuid,
    /// `ssn-type`.
    pub snapshot_type: SurfaceSnapshotType,
    /// Which surface.
    pub surface: Surface,
    /// The surface instance key (`d`).
    pub key: String,
    /// `x`: sha256 of the blob, 64 lowercase hex.
    pub sha256: String,
    /// `url`: where the blob lives on relay media.
    pub url: String,
    /// `m`.
    pub mime: String,
    /// `dim` width.
    pub width: u32,
    /// `dim` height.
    pub height: u32,
    /// `taken-at`, Unix milliseconds.
    pub taken_at_ms: u64,
    /// `provider`: the machine's provider/host key.
    pub provider: PublicKey,
    /// `p` marker `requested-by`.
    pub requested_by: Option<PublicKey>,
    /// `commit`.
    pub commit: Option<SnapshotCommit>,
    /// `e`: the referenced event and its marker.
    pub reference: Option<(EventId, SnapshotReferenceMarker)>,
    /// `page` (preview only).
    pub page: Option<String>,
    /// `title` (preview only).
    pub title: Option<String>,
    /// Alt text.
    pub alt: String,
}

const SNAPSHOT_LAYOUT: [TagSlot; 16] = [
    TagSlot::required("h"),
    TagSlot::required("ssn-v"),
    TagSlot::required("ssn-type"),
    TagSlot::required("surface"),
    TagSlot::required("d"),
    TagSlot::required("x"),
    TagSlot::required("url"),
    TagSlot::required("m"),
    TagSlot::required("dim"),
    TagSlot::required("taken-at"),
    TagSlot::required("provider"),
    TagSlot::optional_arity("p", 4),
    TagSlot::optional_arity("commit", 3),
    TagSlot::optional_arity("e", 4),
    TagSlot::optional("page"),
    TagSlot::optional("title"),
];

impl SurfaceSnapshot {
    /// The exact ordered tags of this snapshot.
    pub fn tags(&self) -> Vec<Vec<String>> {
        let pair = |name: &str, value: String| vec![name.to_owned(), value];
        let mut rows = vec![
            pair("h", self.channel_id.to_string()),
            pair("ssn-v", SURFACE_SNAPSHOT_TAG_VERSION.into()),
            pair("ssn-type", self.snapshot_type.as_str().into()),
            pair("surface", self.surface.as_str().into()),
            pair("d", self.key.clone()),
            pair("x", self.sha256.clone()),
            pair("url", self.url.clone()),
            pair("m", self.mime.clone()),
            pair("dim", format!("{}x{}", self.width, self.height)),
            pair("taken-at", self.taken_at_ms.to_string()),
            pair("provider", self.provider.to_hex()),
        ];
        if let Some(requester) = &self.requested_by {
            rows.push(vec![
                "p".into(),
                requester.to_hex(),
                String::new(),
                "requested-by".into(),
            ]);
        }
        if let Some(commit) = &self.commit {
            rows.push(vec![
                "commit".into(),
                commit.sha.clone(),
                if commit.dirty { "dirty" } else { "clean" }.into(),
            ]);
        }
        if let Some((id, marker)) = &self.reference {
            rows.push(vec![
                "e".into(),
                id.to_hex(),
                String::new(),
                marker.as_str().into(),
            ]);
        }
        if let Some(page) = &self.page {
            rows.push(pair("page", page.clone()));
        }
        if let Some(title) = &self.title {
            rows.push(pair("title", title.clone()));
        }
        rows
    }
}

/// Validate a kind 44253 snapshot from its tags and alt-text content.
pub fn validate_surface_snapshot_parts(
    tags: &[&[String]],
    content: &str,
) -> Result<SurfaceSnapshot, String> {
    if content.len() > MAX_SURFACE_SNAPSHOT_ALT_BYTES {
        return Err(format!(
            "surface snapshot alt text exceeds {MAX_SURFACE_SNAPSHOT_ALT_BYTES} bytes"
        ));
    }
    refuse_host_local("snapshot alt text", content)?;
    let slots = match_ordered_tags(tags, &SNAPSHOT_LAYOUT, "surface snapshot")?;
    let channel_id = parse_channel(required_value(&slots, 0))?;
    if required_value(&slots, 1) != SURFACE_SNAPSHOT_TAG_VERSION {
        return Err("unsupported surface snapshot ssn-v".into());
    }
    let snapshot_type = match required_value(&slots, 2) {
        "snapshot" => SurfaceSnapshotType::Snapshot,
        "annotation" => SurfaceSnapshotType::Annotation,
        _ => return Err("ssn-type must be snapshot or annotation".into()),
    };
    let surface = Surface::from_wire(required_value(&slots, 3))
        .ok_or_else(|| "surface must be preview or device".to_owned())?;
    let key = required_value(&slots, 4);
    validate_surface_key(surface, key)?;
    let exempt: &[&str] = match surface {
        Surface::Preview => &["h", "d"],
        Surface::Device => &["h"],
    };
    refuse_host_local_tags(tags, exempt)?;
    let sha256 = required_value(&slots, 5);
    require_hex(sha256, 64, "x")?;
    let url = required_value(&slots, 6);
    validate_blob_url(url, sha256)?;
    let mime = required_value(&slots, 7);
    if !SURFACE_SNAPSHOT_MIMES.contains(&mime) {
        return Err("m must be image/png, image/jpeg or image/webp".into());
    }
    let (width, height) = parse_dimensions(required_value(&slots, 8), "dim")?;
    let taken_at_ms = parse_decimal(required_value(&slots, 9), "taken-at")?;
    let provider = parse_pubkey(required_value(&slots, 10), "provider")?;
    let requested_by = slots[11]
        .map(|tag| {
            if tag[2].is_empty() && tag[3] == "requested-by" {
                parse_pubkey(&tag[1], "requested-by")
            } else {
                Err("p must be [p, <hex>, \"\", requested-by]".into())
            }
        })
        .transpose()?;
    let commit = slots[12]
        .map(|tag| {
            require_hex(&tag[1], 40, "commit")?;
            let dirty = match tag[2].as_str() {
                "dirty" => true,
                "clean" => false,
                _ => return Err("commit third field must be clean or dirty".to_owned()),
            };
            Ok(SnapshotCommit {
                sha: tag[1].clone(),
                dirty,
            })
        })
        .transpose()?;
    let reference = slots[13]
        .map(|tag| {
            require_hex(&tag[1], 64, "e")?;
            let id = EventId::from_hex(&tag[1]).map_err(|_| "e is not an event id".to_owned())?;
            let marker = match (tag[2].as_str(), tag[3].as_str()) {
                ("", "command") => SnapshotReferenceMarker::Command,
                ("", "annotation") => SnapshotReferenceMarker::Annotation,
                _ => return Err("e must be [e, <id>, \"\", command|annotation]".to_owned()),
            };
            Ok((id, marker))
        })
        .transpose()?;
    if snapshot_type == SurfaceSnapshotType::Annotation
        && !matches!(reference, Some((_, SnapshotReferenceMarker::Annotation)))
    {
        return Err(
            "an annotation snapshot must reference its steer with e marker annotation".into(),
        );
    }
    let page = slot_value(slots[14]).map(str::to_owned);
    let title = slot_value(slots[15]).map(str::to_owned);
    match surface {
        Surface::Preview => {
            let Some(page) = &page else {
                return Err("a preview snapshot must carry page".into());
            };
            validate_page(page)?;
            if let Some(title) = &title {
                validate_preview_title(title)?;
            }
        }
        Surface::Device if page.is_some() || title.is_some() => {
            return Err("a device snapshot carries no page or title".into());
        }
        Surface::Device => {}
    }
    Ok(SurfaceSnapshot {
        channel_id,
        snapshot_type,
        surface,
        key: key.to_owned(),
        sha256: sha256.to_owned(),
        url: url.to_owned(),
        mime: mime.to_owned(),
        width,
        height,
        taken_at_ms,
        provider,
        requested_by,
        commit,
        reference,
        page,
        title,
        alt: content.to_owned(),
    })
}

/// [`validate_surface_snapshot_parts`] over a signed event, checking its kind.
pub fn validate_surface_snapshot_envelope(event: &Event) -> Result<SurfaceSnapshot, String> {
    if event_kind_u32(event) != KIND_SURFACE_SNAPSHOT {
        return Err("event is not a surface snapshot (kind 44253)".into());
    }
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    validate_surface_snapshot_parts(&tags, &event.content)
}

fn validate_blob_url(url: &str, sha256: &str) -> Result<(), String> {
    if url.len() > MAX_SURFACE_SNAPSHOT_URL_BYTES
        || !(url.starts_with("https://") || url.starts_with("http://"))
        || url.contains('?')
        || url.contains('#')
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err("url must be an http(s) URL with no query or fragment".into());
    }
    if !url.contains(sha256) {
        return Err("url must name the blob by its x hash".into());
    }
    Ok(())
}

/// Parse a verdict evidence token naming a snapshot: `snapshot:<64 hex>` or
/// the alias `preview:<64 hex>`. Anything else is `None`.
pub fn parse_snapshot_evidence_token(token: &str) -> Option<EventId> {
    let id = token
        .strip_prefix(SNAPSHOT_EVIDENCE_PREFIX)
        .or_else(|| token.strip_prefix(SNAPSHOT_EVIDENCE_ALIAS_PREFIX))?;
    require_hex(id, 64, "snapshot id").ok()?;
    EventId::from_hex(id).ok()
}

/// The canonical evidence token for a snapshot id.
pub fn snapshot_evidence_token(id: &EventId) -> String {
    format!("{SNAPSHOT_EVIDENCE_PREFIX}{}", id.to_hex())
}

#[cfg(test)]
#[path = "surface_snapshot_tests.rs"]
mod tests;
