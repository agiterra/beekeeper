//! `bee repos protect` against a repository you co-founded but did not
//! announce — the CLI half of lane L26's founder-signed rule record.
//!
//! Before this, `protect set` was a read-modify-write of the caller's **own**
//! kind:30617: a co-founder running it either got `NotFound` (the read was
//! scoped to `authors: [me]`) or, given the same repository id under their own
//! key, silently published a second repository. Finding 33's residual R2.
//!
//! Now the command reads the repository by id whoever announced it, and picks
//! the record it is entitled to write:
//!
//! * **the announcement**, when the caller signed it — unchanged, and still
//!   the path that preserves every other tag byte-for-byte;
//! * **a rule record** (kind 30625,
//!   [`beekeeper_core::repository_protection`]) otherwise — the caller's own
//!   addressable record for that repository, carrying only the rows they hold.
//!
//! Either way the command says which record it wrote, because "your rule is
//! live" and "your rule is live in a second record that the relay resolves
//! against the announcement" are different facts and the second one is the
//! one a person needs when they go looking for it.
//!
//! # Read-optional
//!
//! A repository with no rule record resolves to its announcement's rows
//! exactly as it always did. `protect list` never requires a record to exist,
//! and says which record carries each rule so a rule nobody can find is not
//! possible.

use beekeeper_core::git_perms::PROTECTION_RULE_CLEAR;
use beekeeper_core::kind::{KIND_GIT_REPO_ANNOUNCEMENT, KIND_GIT_REPO_PROTECTION};
use beekeeper_core::repository_founders::RepositoryFounders;
use beekeeper_core::repository_protection::{
    build_repository_protection, decode_repository_protection, repository_protection_d_tag,
    resolve_protection_layers, ProtectionLayer, ProtectionRecordSource, ResolvedProtection,
};
use nostr::{Event, EventBuilder, Kind, Tag};

use crate::client::BeekeeperClient;
use crate::error::CliError;

/// How many rule records the CLI reads for one repository — the relay's own
/// `PROTECTION_MAX_RECORDS`, restated here so the two cannot drift apart
/// silently and a listing that hit the bound can say so.
pub(crate) const PROTECTION_MAX_RECORDS: usize = 64;

/// A repository's announcement, every founder's rule record, and the rules
/// they resolve to.
pub(crate) struct RepositoryRules {
    /// The repository's own announcement.
    pub announcement: Event,
    /// Its founder set, as this CLI could resolve it.
    pub founders: RepositoryFounders,
    /// The resolved rules and their provenance.
    pub resolved: ResolvedProtection,
    /// How many rule records were read (before the founder filter).
    pub records_read: usize,
    /// How many were dropped because their author is not a founder now.
    pub records_from_non_founders: usize,
}

impl RepositoryRules {
    /// Whether `pubkey` may set or remove a rule on this repository.
    pub fn may_set_rules(&self, pubkey: &str) -> bool {
        self.founders.contains(pubkey)
    }

    /// The record `pubkey` is entitled to write: the announcement when they
    /// signed it, otherwise their own rule record.
    pub fn writable_record(&self, pubkey: &str) -> WritableRecord {
        if self
            .announcement
            .pubkey
            .to_hex()
            .eq_ignore_ascii_case(pubkey)
        {
            WritableRecord::Announcement
        } else {
            WritableRecord::RuleRecord
        }
    }

    /// The newest `created_at` any layer holds for `pattern`, so a rewrite can
    /// be stamped past it and actually win.
    pub fn winning_created_at(&self, pattern: &str) -> u64 {
        self.resolved
            .decision_for(pattern)
            .map(|decision| decision.created_at)
            .unwrap_or_else(|| self.announcement.created_at.as_secs())
    }
}

/// Which record a `protect` write lands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WritableRecord {
    /// The repository's own kind:30617.
    Announcement,
    /// The caller's own kind:30625 rule record.
    RuleRecord,
}

impl WritableRecord {
    /// The word the command prints so a person can find what it wrote.
    pub fn label(self) -> &'static str {
        match self {
            Self::Announcement => "announcement",
            Self::RuleRecord => "rule-record",
        }
    }
}

/// The repository announced under `repo_id`, whoever signed it.
///
/// Repository ids are unique per community (`git_repo_names` reserves them),
/// so `#d` alone identifies one. The old read filtered by `authors: [me]`,
/// which is what made a co-founder's `protect list` a `NotFound` on a
/// repository they co-found.
pub(crate) async fn fetch_repo_announcement(
    client: &BeekeeperClient,
    repo_id: &str,
) -> Result<Option<Event>, CliError> {
    let filter = serde_json::json!({
        "kinds": [KIND_GIT_REPO_ANNOUNCEMENT],
        "#d": [repo_id],
        "limit": 8,
    });
    let raw = client.query(&filter).await?;
    let mut events: Vec<Event> = serde_json::from_str(&raw)
        .map_err(|error| CliError::Other(format!("failed to parse relay response: {error}")))?;
    events.sort_by_key(|event| std::cmp::Reverse(event.created_at));
    Ok(events.into_iter().next())
}

/// Every stored rule record addressing this repository.
pub(crate) async fn fetch_rule_records(
    client: &BeekeeperClient,
    repo_owner_hex: &str,
    repo_id: &str,
) -> Result<Vec<Event>, CliError> {
    let filter = serde_json::json!({
        "kinds": [KIND_GIT_REPO_PROTECTION],
        "#d": [repository_protection_d_tag(repo_owner_hex, repo_id)],
        "limit": PROTECTION_MAX_RECORDS,
    });
    let raw = client.query(&filter).await?;
    serde_json::from_str(&raw)
        .map_err(|error| CliError::Other(format!("failed to parse relay response: {error}")))
}

/// Read a repository's rules the way the relay's push gate does.
///
/// The founder filter is the same one the gate applies: a record whose author
/// founded the repository yesterday and does not today contributes nothing.
pub(crate) async fn read_repository_rules(
    client: &BeekeeperClient,
    repo_id: &str,
) -> Result<RepositoryRules, CliError> {
    crate::validate::validate_repo_id(repo_id)?;
    let announcement = fetch_repo_announcement(client, repo_id)
        .await?
        .ok_or_else(|| CliError::NotFound(format!("no repository is announced as {repo_id:?}")))?;
    let owner_hex = announcement.pubkey.to_hex();
    let founders = super::repos::repository_founders(client, &announcement).await;

    let tags: Vec<Vec<String>> = announcement
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    let announcement_layer = ProtectionLayer::from_announcement_tags(
        announcement.created_at.as_secs(),
        announcement.id.to_hex(),
        &tags,
    )
    .map_err(|error| {
        CliError::Other(format!(
            "repository contains invalid protection rules: {error}"
        ))
    })?;

    let records = fetch_rule_records(client, &owner_hex, repo_id).await?;
    let records_read = records.len();
    let mut records_from_non_founders = 0usize;
    let mut layers = vec![announcement_layer];
    for event in records {
        let Ok(record) = decode_repository_protection(&event) else {
            continue;
        };
        if !record.addresses(&owner_hex, repo_id) {
            continue;
        }
        if !founders.contains(record.author()) {
            records_from_non_founders += 1;
            continue;
        }
        layers.push(ProtectionLayer::from_record(
            &record,
            event.created_at.as_secs(),
        ));
    }

    Ok(RepositoryRules {
        announcement,
        founders,
        resolved: resolve_protection_layers(&layers),
        records_read,
        records_from_non_founders,
    })
}

/// The caller's own current rule record for this repository, if any.
pub(crate) async fn fetch_own_rule_record(
    client: &BeekeeperClient,
    repo_owner_hex: &str,
    repo_id: &str,
) -> Result<Option<Event>, CliError> {
    let me = client.keys().public_key().to_hex();
    let mut mine: Vec<Event> = fetch_rule_records(client, repo_owner_hex, repo_id)
        .await?
        .into_iter()
        .filter(|event| event.pubkey.to_hex().eq_ignore_ascii_case(&me))
        .collect();
    mine.sort_by_key(|event| std::cmp::Reverse(event.created_at));
    Ok(mine.into_iter().next())
}

/// Every `buzz-protect` row an event carries, as raw values without the tag
/// name — the shape [`build_repository_protection`] takes.
fn protect_rows(event: &Event) -> Vec<Vec<String>> {
    event
        .tags
        .iter()
        .filter_map(|tag| {
            let values = tag.as_slice();
            (values.first().map(String::as_str) == Some("buzz-protect") && values.len() >= 3)
                .then(|| values[1..].to_vec())
        })
        .collect()
}

/// Build the caller's next rule record: their current rows with `pattern`'s
/// replaced by `replacement` (or, given `None`, by the clear token).
///
/// `remove` writes `["<pattern>", "none"]` rather than simply dropping the
/// row, because dropping it would fall back to whatever the *announcement*
/// says — which is the opposite of what a person who typed "remove the
/// protection on this ref" asked for. The clear is a signed founder act with
/// a name on it, which is the property this whole kind exists to have.
pub(crate) fn next_rule_record_rows(
    current: Option<&Event>,
    pattern: &str,
    replacement: Option<Vec<String>>,
) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = current
        .map(protect_rows)
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.first().map(String::as_str) != Some(pattern))
        .collect();
    rows.push(
        replacement.unwrap_or_else(|| vec![pattern.to_string(), PROTECTION_RULE_CLEAR.to_string()]),
    );
    rows
}

/// Sign and publish a rule record carrying `rows`.
///
/// `created_at` is advanced past whatever currently wins the pattern, so the
/// write actually takes effect under last-write-wins rather than losing
/// silently to a newer announcement.
pub(crate) async fn publish_rule_record(
    client: &BeekeeperClient,
    repo_owner_hex: &str,
    repo_id: &str,
    rows: &[Vec<String>],
    created_at: u64,
) -> Result<String, CliError> {
    let draft = build_repository_protection(repo_owner_hex, repo_id, rows)
        .map_err(|error| CliError::Usage(format!("invalid protection rule: {error}")))?;
    let mut tags: Vec<Tag> = Vec::with_capacity(draft.tags.len());
    for tag in &draft.tags {
        tags.push(
            Tag::parse(tag.clone())
                .map_err(|error| CliError::Other(format!("failed to build tag: {error}")))?,
        );
    }
    let builder = EventBuilder::new(
        Kind::Custom(KIND_GIT_REPO_PROTECTION as u16),
        draft.content.clone(),
    )
    .tags(tags)
    .custom_created_at(nostr::Timestamp::from(created_at));
    let event = client.sign_event(builder)?;
    client.submit_event(event).await
}

/// The JSON one resolved pattern prints: the rule, and which record carries
/// it and who signed it.
pub(crate) fn decision_json(
    rules: &RepositoryRules,
    decision: &beekeeper_core::repository_protection::ProtectionPatternDecision,
) -> serde_json::Value {
    let signer = rules.announcement.pubkey.to_hex();
    let (record, author) = match &decision.source {
        ProtectionRecordSource::Announcement => ("announcement", signer),
        ProtectionRecordSource::FounderRecord { author } => ("rule-record", author.clone()),
    };
    serde_json::json!({
        "ref": decision.pattern,
        "rules": decision.rules,
        "cleared": decision.cleared,
        "record": record,
        "record_event_id": decision.event_id,
        "signed_by": author,
        "superseded": decision
            .superseded
            .iter()
            .map(|source| match source {
                ProtectionRecordSource::Announcement => "announcement".to_string(),
                ProtectionRecordSource::FounderRecord { author } =>
                    format!("rule-record:{author}"),
            })
            .collect::<Vec<String>>(),
    })
}

#[cfg(test)]
#[path = "repos_protection_tests.rs"]
mod tests;
