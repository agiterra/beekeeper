//! A founder-signed rule record: any founder may set or remove a
//! repository's protection (kind 30625).
//!
//! # Why the kind exists
//!
//! Finding 33's residual R2. `buzz-protect` rows live on the kind:30617
//! announcement, which is addressable by `(kind, author, d)` — so only its
//! signer can ever rewrite them. A co-founder running `bee repos protect set`
//! did not edit the repository's rules; they published *a second repository*
//! at their own address. [`crate::repository_founders`] made the founder set
//! plural for every other authority question in batch 3 (which missions may
//! rule, who may land, who may name a project's packs) and left this one
//! singular, disclosed rather than fixed.
//!
//! This module is the fix. A **rule record** is a small addressable event
//! that carries `buzz-protect` rows for a repository it does not own:
//!
//! ```text
//! kind:  30625
//! d:     "<repo-owner-hex>:<repo-id>"
//! tags:  ["buzz-protect", "<ref-pattern>", "<rule>", …]   (zero or more)
//! body:  {"schema":"buzz-repo-protection/v1"}
//! ```
//!
//! The relay admits one only from a founder of the repository its `d` names
//! (`buzz-relay`'s `handlers::repo_protection`), so the record's authority is
//! its **author**, checked at the write; nothing about its shape confers any.
//!
//! # The ruling this implements
//!
//! *A co-founder MAY remove protection the signer set.* Equal founders are
//! equal, and every change is a signed, observable founder act with a name on
//! it — which is a stronger property than "only one key can change it", where
//! the other founder's only recourse is to ask. The alternative ruling
//! (set-but-never-remove) is a one-line change to
//! [`resolve_protection_layers`]: refuse to let a [`ProtectionRecordSource`]
//! other than the pattern's current owner clear it. Brian may overrule.
//!
//! # Layering: last write wins, per exact ref pattern
//!
//! The rules that govern a repository are resolved from **layers**: the
//! announcement's own rows, plus the newest rule record per founder. Each
//! layer names some set of exact ref patterns.
//!
//! For each pattern *string* — byte-exact, not by what it matches — the
//! layer with the newest `created_at` wins and contributes **all** of its
//! rows for that pattern; every older layer's rows for that pattern are
//! dropped. Patterns are independent, so setting `refs/heads/main` never
//! disturbs `refs/tags/*`. Ties on `created_at` break on the greater event
//! id, so two founders acting in the same second resolve the same way on
//! every relay and in every client.
//!
//! Removal is [`crate::git_perms::PROTECTION_RULE_CLEAR`]: a row that names a
//! pattern and says it carries no rules. It wins the pattern like any other
//! row, and resolution then drops it, so the ref ends up governed by the
//! built-in defaults exactly as if nobody had ever mentioned it. A kind-5
//! tombstone by a record's own author removes the whole record — the relay's
//! ordinary addressable-deletion path — and with it that founder's layer.
//!
//! # Read-optional, everywhere
//!
//! Finding 31's rule. A repository whose rules were signed before this kind
//! existed has exactly one layer, its announcement, and resolves to the rules
//! it always had (`rules_signed_before_the_kind_existed_still_govern`). Every
//! reader — relay push policy, `bee repos protect list`, the desktop
//! Protection panel — treats rule records as **absent by default** and never
//! requires one to exist. An unknown rule token inside a record is reported
//! and ignored, never fatal to the record.

use serde::{Deserialize, Serialize};

use crate::git_perms::{parse_protection_tags, ProtectionRule, RuleParseError};

/// Content schema of a repository rule record.
pub const REPOSITORY_PROTECTION_SCHEMA: &str = "buzz-repo-protection/v1";

/// The `buzz-protect` tag name, shared with kind:30617.
pub const PROTECT_TAG: &str = "buzz-protect";

/// Re-exported so a caller writing a removal row does not have to reach into
/// two modules to spell one act.
pub use crate::git_perms::PROTECTION_RULE_CLEAR;

/// Why a rule record could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectionRecordError {
    /// The event is not kind 30625.
    WrongKind(u16),
    /// No `d` tag, or one that does not address a repository.
    MalformedAddress,
    /// Content is not the expected JSON envelope.
    MalformedContent,
    /// Content parsed but named a different schema.
    UnknownSchema(String),
    /// A `buzz-protect` row is structurally invalid.
    MalformedRow(RuleParseError),
}

impl std::fmt::Display for ProtectionRecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongKind(kind) => write!(f, "not a repository rule record (kind {kind})"),
            Self::MalformedAddress => write!(
                f,
                "d tag must be \"<repo-owner-hex>:<repo-id>\" with a 64-hex owner"
            ),
            Self::MalformedContent => write!(
                f,
                "content must be {{\"schema\":\"{REPOSITORY_PROTECTION_SCHEMA}\"}}"
            ),
            Self::UnknownSchema(schema) => write!(
                f,
                "unknown schema {schema:?}, expected {REPOSITORY_PROTECTION_SCHEMA:?}"
            ),
            Self::MalformedRow(error) => write!(f, "invalid buzz-protect row: {error}"),
        }
    }
}

impl std::error::Error for ProtectionRecordError {}

/// The JSON envelope a rule record carries.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProtectionContent {
    schema: String,
}

/// The `d` tag addressing `repo_id` under `repo_owner_hex`.
///
/// Lower-cased owner, repository id verbatim: repository ids are compared
/// byte-exactly everywhere else (`EventQuery::d_tag`), and folding their case
/// here would address a repository that does not exist.
pub fn repository_protection_d_tag(repo_owner_hex: &str, repo_id: &str) -> String {
    format!("{}:{repo_id}", repo_owner_hex.trim().to_ascii_lowercase())
}

/// Split a rule record's `d` tag back into `(owner-hex, repo-id)`.
///
/// Splits on the **first** colon only. A repository id may contain colons —
/// `crates/buzz-cli`'s `repo_id_from_event` never forbade it, and ingest's
/// own addressable parsing splits the same way — so splitting on the last
/// colon, or refusing colon-bearing ids, would silently address a different
/// repository than the record names.
pub fn parse_repository_protection_d_tag(d_tag: &str) -> Option<(String, String)> {
    let (owner, repo_id) = d_tag.split_once(':')?;
    let owner = owner.to_ascii_lowercase();
    if owner.len() != 64 || !owner.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    if repo_id.is_empty() {
        return None;
    }
    Some((owner, repo_id.to_string()))
}

/// An unsigned rule record: the content and tags a caller signs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryProtectionDraft {
    /// Event content — the schema envelope.
    pub content: String,
    /// Event tags, `d` first then one row per pattern.
    pub tags: Vec<Vec<String>>,
}

/// Build the record a founder signs to carry `rows` for one repository.
///
/// `rows` are `buzz-protect` values **without** the tag name: each is
/// `[pattern, rule, rule, …]`, exactly the shape
/// [`crate::git_perms::parse_protection_tag`] takes. An empty `rows` is legal
/// and meaningful — it is a founder saying "I hold no rules here", which
/// retires everything they had set without needing a tombstone.
///
/// # Errors
/// Returns the parse error of the first row that is not a valid rule, so a
/// typo is refused at the writer rather than published and ignored.
pub fn build_repository_protection(
    repo_owner_hex: &str,
    repo_id: &str,
    rows: &[Vec<String>],
) -> Result<RepositoryProtectionDraft, ProtectionRecordError> {
    let owner = repo_owner_hex.trim().to_ascii_lowercase();
    if owner.len() != 64 || !owner.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ProtectionRecordError::MalformedAddress);
    }
    if repo_id.is_empty() {
        return Err(ProtectionRecordError::MalformedAddress);
    }
    let mut tags = vec![vec![
        "d".to_string(),
        repository_protection_d_tag(&owner, repo_id),
    ]];
    for row in rows {
        let values: Vec<&str> = row.iter().map(String::as_str).collect();
        crate::git_perms::parse_protection_tag(&values)
            .map_err(ProtectionRecordError::MalformedRow)?;
        let mut tag = vec![PROTECT_TAG.to_string()];
        tag.extend(row.iter().cloned());
        tags.push(tag);
    }
    let content = serde_json::to_string(&ProtectionContent {
        schema: REPOSITORY_PROTECTION_SCHEMA.to_string(),
    })
    .map_err(|_| ProtectionRecordError::MalformedContent)?;
    Ok(RepositoryProtectionDraft { content, tags })
}

/// A decoded rule record: whose it is, which repository it addresses, and the
/// rows it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryProtectionRecord {
    repo_owner: String,
    repo_id: String,
    author: String,
    event_id: String,
    created_at: u64,
    rules: Vec<ProtectionRule>,
    unknown_rules: Vec<String>,
}

impl RepositoryProtectionRecord {
    /// The repository owner this record's address names.
    pub fn repo_owner(&self) -> &str {
        &self.repo_owner
    }

    /// The repository id this record's address names.
    pub fn repo_id(&self) -> &str {
        &self.repo_id
    }

    /// The founder who signed it. This, not the record's shape, is its
    /// authority.
    pub fn author(&self) -> &str {
        &self.author
    }

    /// The record's own event id — what a surface cites when it says which
    /// record carries a rule.
    pub fn event_id(&self) -> &str {
        &self.event_id
    }

    /// The record's `created_at`, which is what orders it against other
    /// layers.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// The rows it carries, cleared rows included.
    pub fn rules(&self) -> &[ProtectionRule] {
        &self.rules
    }

    /// Rule tokens this build does not recognise — reported, never fatal.
    pub fn unknown_rules(&self) -> &[String] {
        &self.unknown_rules
    }

    /// Whether this record addresses the given repository.
    pub fn addresses(&self, repo_owner_hex: &str, repo_id: &str) -> bool {
        self.repo_owner.eq_ignore_ascii_case(repo_owner_hex.trim()) && self.repo_id == repo_id
    }
}

/// Read a signed kind:30625 into a [`RepositoryProtectionRecord`].
///
/// Unknown **tags** are ignored rather than refused — deliberately, and
/// unlike kind 30624's stricter envelope. A pack source's authority partly
/// rests on its shape (a tag nobody validated could re-point a team's agents);
/// a rule record's authority is entirely its author, already checked at the
/// write gate, so refusing it over a tag a newer client added would only make
/// a future amendment un-shippable. Unknown *rule tokens* are likewise
/// reported and skipped, so this build never mistakes "I do not know this
/// rule" for "this record is invalid".
///
/// # Errors
/// Wrong kind, an address that names no repository, a missing or foreign
/// content schema, or a structurally invalid `buzz-protect` row.
pub fn decode_repository_protection(
    event: &nostr::Event,
) -> Result<RepositoryProtectionRecord, ProtectionRecordError> {
    let kind = event.kind.as_u16();
    if u32::from(kind) != crate::kind::KIND_GIT_REPO_PROTECTION {
        return Err(ProtectionRecordError::WrongKind(kind));
    }
    let tags: Vec<Vec<String>> = event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    let d_tag = tags
        .iter()
        .find_map(|tag| match tag.as_slice() {
            [name, value, ..] if name == "d" => Some(value.as_str()),
            _ => None,
        })
        .ok_or(ProtectionRecordError::MalformedAddress)?;
    let (repo_owner, repo_id) =
        parse_repository_protection_d_tag(d_tag).ok_or(ProtectionRecordError::MalformedAddress)?;

    let content: ProtectionContent = serde_json::from_str(&event.content)
        .map_err(|_| ProtectionRecordError::MalformedContent)?;
    if content.schema != REPOSITORY_PROTECTION_SCHEMA {
        return Err(ProtectionRecordError::UnknownSchema(content.schema));
    }

    let parsed = parse_protection_tags(&tags).map_err(ProtectionRecordError::MalformedRow)?;
    Ok(RepositoryProtectionRecord {
        repo_owner,
        repo_id,
        author: event.pubkey.to_hex(),
        event_id: event.id.to_hex(),
        created_at: event.created_at.as_secs(),
        rules: parsed.rules,
        unknown_rules: parsed.unknown_rules,
    })
}

/// Which record a resolved rule came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectionRecordSource {
    /// The repository's own kind:30617 announcement — the only source that
    /// exists for a repository whose rules predate kind 30625.
    Announcement,
    /// A founder's kind:30625 rule record, by its author.
    FounderRecord {
        /// The founder who signed it, lower-hex.
        author: String,
    },
}

impl ProtectionRecordSource {
    /// The pubkey that signed this source.
    pub fn author(&self, announcement_signer: &str) -> String {
        match self {
            Self::Announcement => announcement_signer.to_ascii_lowercase(),
            Self::FounderRecord { author } => author.to_ascii_lowercase(),
        }
    }
}

/// One record's contribution to a repository's rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectionLayer {
    /// Which record this is.
    pub source: ProtectionRecordSource,
    /// The record's `created_at` — what orders it against the others.
    pub created_at: u64,
    /// The record's event id — the tie-break, and what a surface cites.
    pub event_id: String,
    /// Every `buzz-protect` row the record carries, cleared rows included.
    pub rules: Vec<ProtectionRule>,
    /// Rule tokens this build did not recognise in this record.
    pub unknown_rules: Vec<String>,
}

impl ProtectionLayer {
    /// The announcement's own layer, from its raw tags.
    ///
    /// # Errors
    /// A structurally invalid `buzz-protect` tag, which the push policy has
    /// always treated as fail-closed.
    pub fn from_announcement_tags(
        created_at: u64,
        event_id: String,
        tags: &[Vec<String>],
    ) -> Result<Self, RuleParseError> {
        let parsed = parse_protection_tags(tags)?;
        Ok(Self {
            source: ProtectionRecordSource::Announcement,
            created_at,
            event_id,
            rules: parsed.rules,
            unknown_rules: parsed.unknown_rules,
        })
    }

    /// A founder record's layer.
    ///
    /// `created_at` is passed rather than read off the record so a caller
    /// holding a storage row's authoritative timestamp can use it; pass
    /// [`RepositoryProtectionRecord::created_at`] when there is no other.
    pub fn from_record(record: &RepositoryProtectionRecord, created_at: u64) -> Self {
        Self {
            source: ProtectionRecordSource::FounderRecord {
                author: record.author().to_ascii_lowercase(),
            },
            created_at,
            event_id: record.event_id().to_string(),
            rules: record.rules().to_vec(),
            unknown_rules: record.unknown_rules().to_vec(),
        }
    }
}

/// Which record won one exact ref pattern, and what it displaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectionPatternDecision {
    /// The exact pattern string this decision covers.
    pub pattern: String,
    /// The record whose rows govern it.
    pub source: ProtectionRecordSource,
    /// That record's event id.
    pub event_id: String,
    /// That record's `created_at`.
    pub created_at: u64,
    /// Whether the winning record **cleared** the pattern — said it carries
    /// no rules. A cleared pattern contributes nothing to
    /// [`ResolvedProtection::rules`], and a surface must say "cleared by X"
    /// rather than showing nothing at all.
    pub cleared: bool,
    /// The rule tokens the winning rows carry, in wire order — what a listing
    /// prints without re-deriving the grammar.
    pub rules: Vec<String>,
    /// Every record that named this pattern and lost, newest first.
    pub superseded: Vec<ProtectionRecordSource>,
}

/// The rules that govern a repository, and who set each one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedProtection {
    rules: Vec<ProtectionRule>,
    decisions: Vec<ProtectionPatternDecision>,
    unknown_rules: Vec<String>,
}

impl ResolvedProtection {
    /// The flat rule list to hand [`crate::git_perms::EffectiveRules::for_ref`]
    /// — cleared patterns already removed.
    pub fn rules(&self) -> &[ProtectionRule] {
        &self.rules
    }

    /// One decision per exact pattern, ordered by pattern string so a listing
    /// is stable between reads.
    pub fn decisions(&self) -> &[ProtectionPatternDecision] {
        &self.decisions
    }

    /// Every rule token no layer's build recognised.
    pub fn unknown_rules(&self) -> &[String] {
        &self.unknown_rules
    }

    /// The decision covering an exact pattern string, if any.
    pub fn decision_for(&self, pattern: &str) -> Option<&ProtectionPatternDecision> {
        self.decisions
            .iter()
            .find(|decision| decision.pattern == pattern)
    }
}

/// Resolve layers into the rules that govern the repository.
///
/// Last write wins **per exact ref pattern**: for each pattern string, the
/// layer with the greatest `(created_at, event_id)` contributes all of its
/// rows and every other layer's rows for that pattern are dropped. The result
/// does not depend on the order `layers` arrives in.
///
/// Cleared patterns are removed from [`ResolvedProtection::rules`] and kept in
/// [`ResolvedProtection::decisions`], because "nobody protected this" and
/// "a founder removed the protection" are different facts and only one of
/// them has a name attached.
pub fn resolve_protection_layers(layers: &[ProtectionLayer]) -> ResolvedProtection {
    // Pattern string → every layer index that named it.
    let mut patterns: Vec<String> = Vec::new();
    for layer in layers {
        for rule in &layer.rules {
            let pattern = rule.pattern.as_str().to_string();
            if !patterns.contains(&pattern) {
                patterns.push(pattern);
            }
        }
    }
    patterns.sort();

    let mut rules: Vec<ProtectionRule> = Vec::new();
    let mut decisions: Vec<ProtectionPatternDecision> = Vec::new();

    for pattern in patterns {
        let mut naming: Vec<&ProtectionLayer> = layers
            .iter()
            .filter(|layer| {
                layer
                    .rules
                    .iter()
                    .any(|rule| rule.pattern.as_str() == pattern)
            })
            .collect();
        // Newest first; the greater event id breaks a tie so two founders
        // acting in the same second resolve identically everywhere.
        naming.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.event_id.cmp(&a.event_id))
        });
        let Some((winner, losers)) = naming.split_first() else {
            continue;
        };
        let winning_rows: Vec<&ProtectionRule> = winner
            .rules
            .iter()
            .filter(|rule| rule.pattern.as_str() == pattern)
            .collect();
        // Within one record, a real rule beats a `none` on the same pattern:
        // the operator said something, and the something wins over the
        // nothing. `parse_protection_tag` already applies that inside a single
        // row; this applies it across rows of one record.
        let cleared = winning_rows.iter().all(|rule| rule.cleared);
        if !cleared {
            rules.extend(
                winning_rows
                    .iter()
                    .filter(|rule| !rule.cleared)
                    .map(|rule| (*rule).clone()),
            );
        }
        decisions.push(ProtectionPatternDecision {
            pattern: pattern.clone(),
            source: winner.source.clone(),
            event_id: winner.event_id.clone(),
            created_at: winner.created_at,
            cleared,
            rules: winning_rows
                .iter()
                .flat_map(|rule| rule_tokens(rule))
                .collect(),
            superseded: losers.iter().map(|layer| layer.source.clone()).collect(),
        });
    }

    let mut unknown_rules: Vec<String> = Vec::new();
    for layer in layers {
        for unknown in &layer.unknown_rules {
            if !unknown_rules.contains(unknown) {
                unknown_rules.push(unknown.clone());
            }
        }
    }

    ResolvedProtection {
        rules,
        decisions,
        unknown_rules,
    }
}

/// The wire tokens one parsed rule stands for, in the order the grammar
/// writes them. Used only for display — the enforcement path reads the typed
/// fields, never these strings.
fn rule_tokens(rule: &ProtectionRule) -> Vec<String> {
    if rule.cleared {
        return vec![PROTECTION_RULE_CLEAR.to_string()];
    }
    let mut tokens = Vec::new();
    if let Some(role) = rule.push_role {
        tokens.push(format!("push:{role}"));
    }
    if rule.no_force_push {
        tokens.push("no-force-push".to_string());
    }
    if rule.no_delete {
        tokens.push("no-delete".to_string());
    }
    if rule.require_patch {
        tokens.push("require-patch".to_string());
    }
    if rule.require_verdict {
        tokens.push("require-verdict".to_string());
    }
    tokens
}

#[cfg(test)]
#[path = "repository_protection_tests.rs"]
mod tests;
