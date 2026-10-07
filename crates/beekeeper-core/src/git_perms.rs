//! Git permission types — ref patterns, protection rules, and policy evaluation inputs.
//!
//! This module defines the core data types for the Beekeeper git permission system.
//! The permission model: **channel role or project role = repo role** — a
//! kind:30617 announcement may bind a channel (`buzz-channel`), declare a
//! project (`["project", …]`), or both, and a pusher's effective role is the
//! more permissive of the two grants ([`max_git_role`],
//! [`git_role_for_project_role`]). `buzz-protect` tags on kind:30617 add
//! constraints that apply to everyone (including the owner), whichever path
//! the role arrived through.
//!
//! # Architecture
//!
//! ```text
//! kind:30617 tags → parse → Vec<ProtectionRule>
//!                                    ↓
//! push arrives → classify refs → match patterns → union rules → enforce
//! ```

use crate::channel::{MemberRole, ProjectRole};
use std::fmt;

/// Machine-readable token prefixing the push-policy denial for a kind:30617
/// announcement that grants access through **no** path at all — neither a
/// `buzz-channel` binding nor a `["project", …]` back-reference. A repo
/// inside a project is legitimately unbound and must never see this token:
/// telling its pusher to bind a channel would be advice for a problem they
/// do not have (they are simply not on the project's roster).
///
/// This is a **declared cross-component contract**, not a log string. Known
/// consumers switch on it:
/// - relay `api/git/policy.rs` — produces [`GIT_NO_CHANNEL_BINDING_BODY`]
/// - desktop `src-tauri/commands/project_git_workflow.rs` — merge-failure
///   classifier maps it to a structured `no_channel_binding` error code
/// - desktop `src/features/projects/lib/projectBranchErrors.ts` — dialog
///   copy matcher (TS re-types the literal; its test pins the value)
pub const GIT_NO_CHANNEL_BINDING_TOKEN: &str = "no_channel_binding";

/// Full push-policy denial body for an unbound repository.
///
/// Format: `<token>: <legacy phrase>`. The trailing prose deliberately
/// repeats the token's meaning because desktops already in the field match
/// the exact phrase `no channel binding` (spaces, not underscores — the
/// token alone would NOT satisfy that matcher). Do not "fix" the redundancy:
/// removing the phrase silently breaks every shipped desktop, and removing
/// the token breaks the structured consumers above. A relay-side test pins
/// both matchers.
pub const GIT_NO_CHANNEL_BINDING_BODY: &str =
    "no_channel_binding: repository has no channel binding";

/// The rule token that says a ref pattern carries **no** rules.
///
/// A `buzz-protect` row is the only way to name a pattern, and naming one is
/// how a founder's rule record ([`crate::repository_protection`]) takes a
/// pattern over from an older record. So "remove the protection someone else
/// set" needs a row that means *nothing applies here* —
/// `["buzz-protect", "refs/heads/main", "none"]` — rather than the absence of
/// a row, which is indistinguishable from never having had an opinion.
///
/// A cleared pattern is **not** a guarded ref: the operator said explicitly
/// that it carries no rules, so [`EffectiveRules::for_ref`] reports no
/// explicit match for it and the built-in defaults apply, exactly as they
/// would on a pattern nobody ever mentioned.
pub const PROTECTION_RULE_CLEAR: &str = "none";

/// Maximum number of `buzz-protect` tags per repo.
pub const MAX_PROTECTION_RULES: usize = 50;
/// Maximum character length of a ref pattern.
pub const MAX_PATTERN_LENGTH: usize = 256;
/// Maximum number of wildcard segments per pattern.
pub const MAX_WILDCARDS_PER_PATTERN: usize = 3;

/// A validated ref pattern for matching git refs.
///
/// Grammar: `segment ("/" segment)*` where segment is either a literal
/// `[a-zA-Z0-9._-]+` or `*` (matches exactly one path segment).
///
/// Patterns MUST start with `refs/`. No `**`, `?`, `[...]`, or partial globs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefPattern {
    /// The original pattern string (e.g., "refs/heads/*").
    raw: String,
    /// Pre-split segments for matching.
    segments: Vec<PatternSegment>,
}

/// A single segment in a ref pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PatternSegment {
    /// Matches exactly this literal string.
    Literal(String),
    /// Matches any single path segment.
    Wildcard,
    /// Matches one or more path segments (recursive). Must be the last segment.
    RecursiveWildcard,
}

/// Errors from parsing a ref pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    /// Pattern is empty.
    Empty,
    /// Pattern exceeds maximum length.
    TooLong,
    /// Pattern doesn't start with `refs/`.
    MissingRefsPrefix,
    /// A segment contains invalid characters or is a partial glob.
    InvalidSegment(String),
    /// Too many wildcard segments.
    TooManyWildcards,
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "pattern is empty"),
            Self::TooLong => write!(f, "pattern exceeds {MAX_PATTERN_LENGTH} chars"),
            Self::MissingRefsPrefix => write!(f, "pattern must start with 'refs/'"),
            Self::InvalidSegment(s) => write!(f, "invalid segment: {s:?}"),
            Self::TooManyWildcards => {
                write!(f, "pattern exceeds {MAX_WILDCARDS_PER_PATTERN} wildcards")
            }
        }
    }
}

impl std::error::Error for PatternError {}

impl RefPattern {
    /// Parse and validate a ref pattern string.
    pub fn parse(pattern: &str) -> Result<Self, PatternError> {
        if pattern.is_empty() {
            return Err(PatternError::Empty);
        }
        if pattern.len() > MAX_PATTERN_LENGTH {
            return Err(PatternError::TooLong);
        }
        if !pattern.starts_with("refs/") {
            return Err(PatternError::MissingRefsPrefix);
        }

        let mut segments = Vec::new();
        let mut wildcard_count = 0;

        let parts: Vec<&str> = pattern.split('/').collect();
        for (i, part) in parts.iter().enumerate() {
            if *part == "**" {
                // `**` must be the last segment (recursive match).
                if i != parts.len() - 1 {
                    return Err(PatternError::InvalidSegment(
                        "** must be the last segment".to_string(),
                    ));
                }
                wildcard_count += 1;
                if wildcard_count > MAX_WILDCARDS_PER_PATTERN {
                    return Err(PatternError::TooManyWildcards);
                }
                segments.push(PatternSegment::RecursiveWildcard);
            } else if *part == "*" {
                wildcard_count += 1;
                if wildcard_count > MAX_WILDCARDS_PER_PATTERN {
                    return Err(PatternError::TooManyWildcards);
                }
                segments.push(PatternSegment::Wildcard);
            } else if part.is_empty() {
                return Err(PatternError::InvalidSegment(String::new()));
            } else if part.contains('*')
                || part.contains('?')
                || part.contains('[')
                || part.contains(']')
            {
                // Partial globs (e.g., "v*") are not allowed.
                return Err(PatternError::InvalidSegment(part.to_string()));
            } else if !part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
            {
                return Err(PatternError::InvalidSegment(part.to_string()));
            } else {
                segments.push(PatternSegment::Literal(part.to_string()));
            }
        }

        Ok(Self {
            raw: pattern.to_string(),
            segments,
        })
    }

    /// Test whether this pattern matches a given ref name.
    ///
    /// Matching is segment-by-segment:
    /// - `*` matches exactly one path segment
    /// - `**` (must be last) matches one or more remaining segments
    pub fn matches(&self, ref_name: &str) -> bool {
        let ref_segments: Vec<&str> = ref_name.split('/').collect();

        // Check for recursive wildcard (must be last segment).
        if let Some(PatternSegment::RecursiveWildcard) = self.segments.last() {
            let prefix_len = self.segments.len() - 1;
            // Ref must have at least as many segments as the prefix (+ 1 for the **)
            if ref_segments.len() <= prefix_len {
                return false;
            }
            // All prefix segments must match.
            return self.segments[..prefix_len]
                .iter()
                .zip(ref_segments[..prefix_len].iter())
                .all(|(pat, seg)| match pat {
                    PatternSegment::Wildcard => true,
                    PatternSegment::Literal(lit) => lit == *seg,
                    PatternSegment::RecursiveWildcard => unreachable!(),
                });
        }

        // Non-recursive: exact segment count match required.
        if ref_segments.len() != self.segments.len() {
            return false;
        }
        self.segments
            .iter()
            .zip(ref_segments.iter())
            .all(|(pat, seg)| match pat {
                PatternSegment::Wildcard => true,
                PatternSegment::Literal(lit) => lit == *seg,
                PatternSegment::RecursiveWildcard => unreachable!(),
            })
    }

    /// The raw pattern string.
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl fmt::Display for RefPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

/// The type of ref update in a push.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateKind {
    /// New ref (old_oid is zero).
    Create,
    /// Existing ref updated, new commit is a descendant of old (fast-forward).
    FastForward,
    /// Existing ref updated, new commit is NOT a descendant of old.
    NonFastForward,
    /// Ref deleted (new_oid is zero).
    Delete,
}

impl UpdateKind {
    /// Classify a ref update from old/new OIDs.
    ///
    /// `is_ancestor` should be the result of `git merge-base --is-ancestor old new`.
    /// For creates/deletes, the value is ignored.
    pub fn classify(old_oid: &str, new_oid: &str, is_ancestor: bool) -> Self {
        const ZERO_OID: &str = "0000000000000000000000000000000000000000";
        if old_oid == ZERO_OID {
            Self::Create
        } else if new_oid == ZERO_OID {
            Self::Delete
        } else if is_ancestor {
            Self::FastForward
        } else {
            Self::NonFastForward
        }
    }
}

/// A single ref update within a push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefUpdate {
    /// The ref being updated (e.g., "refs/heads/main").
    pub ref_name: String,
    /// The type of update.
    pub kind: UpdateKind,
    /// Old OID (hex, 40 chars). Zero OID for creates.
    pub old_oid: String,
    /// New OID (hex, 40 chars). Zero OID for deletes.
    pub new_oid: String,
}

/// A single protection rule parsed from a `buzz-protect` tag on kind:30617.
///
/// Format: `["buzz-protect", "<ref-pattern>", "<rule>", ...]`
/// Multiple rules per tag are allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectionRule {
    /// The ref pattern this rule applies to.
    pub pattern: RefPattern,
    /// Minimum role required to push (if specified).
    pub push_role: Option<MemberRole>,
    /// Whether non-fast-forward updates are forbidden.
    pub no_force_push: bool,
    /// Whether ref deletion is forbidden.
    pub no_delete: bool,
    /// Whether direct push is denied (must use NIP-34 patch).
    ///
    /// NOTE: This blocks ALL ref update kinds (create, FF, NFF, delete) — not just
    /// fast-forward pushes. If set on a ref pattern, that ref can only be modified
    /// via the NIP-34 patch workflow. This is intentional: the ref is fully governed
    /// by the patch review process.
    pub require_patch: bool,
    /// Whether an update to this ref must be named by an approved mission
    /// verdict (NIP-CSTX kind 44244) before it is admitted.
    ///
    /// **Enforced by the relay, not by this module.** The rule needs a stored
    /// event search, which [`evaluate_ref_update`] deliberately cannot do —
    /// see `beekeeper-relay`'s `api::git::verdict_admission`. A relay predating
    /// batch 3 parses the token into
    /// [`ParsedProtection::unknown_rules`] and **ignores it**, so the rule is
    /// only ever as strong as the relay serving the repository.
    pub require_verdict: bool,
    /// Whether this row is the [`PROTECTION_RULE_CLEAR`] token — "this
    /// pattern carries no rules".
    ///
    /// Meaningful only through [`crate::repository_protection`]'s layering,
    /// which is what a clear supersedes *something* in. Resolution drops
    /// cleared rows before anything evaluates them, so a caller reading a
    /// resolved rule list never sees one; the field exists so the resolver can
    /// tell "cleared" from "no opinion".
    pub cleared: bool,
}

/// Errors from parsing a `buzz-protect` tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleParseError {
    /// Tag has fewer than 2 values (need at least pattern + one rule).
    TooFewValues,
    /// Too many protection rules on this repo.
    TooManyRules,
    /// Invalid ref pattern.
    InvalidPattern(PatternError),
    /// Unknown rule string.
    UnknownRule(String),
    /// Invalid role in `push:<role>`.
    InvalidRole(String),
}

impl fmt::Display for RuleParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewValues => write!(f, "buzz-protect tag needs pattern + at least one rule"),
            Self::TooManyRules => write!(f, "exceeds max {MAX_PROTECTION_RULES} rules per repo"),
            Self::InvalidPattern(e) => write!(f, "invalid pattern: {e}"),
            Self::UnknownRule(r) => write!(f, "unknown rule: {r:?}"),
            Self::InvalidRole(r) => write!(f, "invalid role in push rule: {r:?}"),
        }
    }
}

impl std::error::Error for RuleParseError {}

/// Parse a single `buzz-protect` tag into a `ProtectionRule`.
///
/// Tag format: `["buzz-protect", "<pattern>", "<rule1>", "<rule2>", ...]`
/// The first element ("buzz-protect") should already be stripped — pass
/// the remaining values starting with the pattern.
/// Parse a single `buzz-protect` tag (simple API, discards unknown rules).
pub fn parse_protection_tag(values: &[&str]) -> Result<ProtectionRule, RuleParseError> {
    let (rule, _unknowns) = parse_protection_tag_with_warnings(values)?;
    Ok(rule)
}

/// Parse a single `buzz-protect` tag, returning unknown rules for logging.
pub fn parse_protection_tag_with_warnings(
    values: &[&str],
) -> Result<(ProtectionRule, Vec<String>), RuleParseError> {
    if values.len() < 2 {
        return Err(RuleParseError::TooFewValues);
    }

    let pattern = RefPattern::parse(values[0]).map_err(RuleParseError::InvalidPattern)?;

    let mut push_role: Option<MemberRole> = None;
    let mut no_force_push = false;
    let mut no_delete = false;
    let mut require_patch = false;
    let mut require_verdict = false;
    let mut cleared = false;
    let mut unknown_rules = Vec::new();

    for &rule_str in &values[1..] {
        if let Some(role_str) = rule_str.strip_prefix("push:") {
            let role: MemberRole = role_str
                .parse()
                .map_err(|_| RuleParseError::InvalidRole(role_str.to_string()))?;
            // Reject push:bot and push:guest — nonsensical rules.
            // Bot is promoted to Member at the policy layer; push:bot is meaningless.
            // Guest cannot push regardless; push:guest would be confusing.
            if matches!(role, MemberRole::Bot | MemberRole::Guest) {
                return Err(RuleParseError::InvalidRole(role_str.to_string()));
            }
            // Take the strictest (highest permission level).
            push_role = Some(match push_role {
                None => role,
                Some(existing) => {
                    if role.permission_level() > existing.permission_level() {
                        role
                    } else {
                        existing
                    }
                }
            });
        } else {
            match rule_str {
                "no-force-push" => no_force_push = true,
                "no-delete" => no_delete = true,
                "require-patch" => require_patch = true,
                "require-verdict" => require_verdict = true,
                PROTECTION_RULE_CLEAR => cleared = true,
                // Forward-compatibility: unknown rules are skipped but reported.
                other => unknown_rules.push(other.to_string()),
            }
        }
    }

    Ok((
        ProtectionRule {
            pattern,
            push_role,
            no_force_push,
            no_delete,
            require_patch,
            require_verdict,
            // A row that also carries a real rule is not a clear: the operator
            // said something, and the something wins over the nothing.
            cleared: cleared
                && push_role.is_none()
                && !no_force_push
                && !no_delete
                && !require_patch
                && !require_verdict,
        },
        unknown_rules,
    ))
}

/// Result of parsing protection tags — includes rules and any warnings.
#[derive(Debug, Clone)]
pub struct ParsedProtection {
    /// Successfully parsed protection rules.
    pub rules: Vec<ProtectionRule>,
    /// Unknown rule strings that were skipped (potential typos or future rules).
    /// Callers should log these as warnings.
    pub unknown_rules: Vec<String>,
}

/// Parse all `buzz-protect` tags from a kind:30617 event's tag list.
///
/// Returns an error if any `buzz-protect` tag is structurally malformed.
/// Unknown rule strings are skipped but reported in `ParsedProtection::unknown_rules`
/// so callers can log warnings (helps catch typos while maintaining forward-compat).
/// Enforces the per-repo rule count limit.
pub fn parse_protection_tags(tags: &[Vec<String>]) -> Result<ParsedProtection, RuleParseError> {
    let mut rules = Vec::new();
    let mut unknown_rules = Vec::new();

    for tag in tags {
        if tag.first().map(|s| s.as_str()) != Some("buzz-protect") {
            continue;
        }
        if rules.len() >= MAX_PROTECTION_RULES {
            return Err(RuleParseError::TooManyRules);
        }
        let values: Vec<&str> = tag[1..].iter().map(|s| s.as_str()).collect();
        let (rule, unknowns) = parse_protection_tag_with_warnings(&values)?;
        rules.push(rule);
        unknown_rules.extend(unknowns);
    }

    Ok(ParsedProtection {
        rules,
        unknown_rules,
    })
}

/// Built-in default minimum role for an operation when no `buzz-protect` tag matches.
pub fn default_min_role(ref_name: &str, kind: UpdateKind) -> MemberRole {
    let is_branch = ref_name.starts_with("refs/heads/");
    let is_tag = ref_name.starts_with("refs/tags/");

    match kind {
        UpdateKind::Create => {
            if is_branch || is_tag {
                MemberRole::Member
            } else {
                MemberRole::Admin
            }
        }
        UpdateKind::FastForward => {
            if is_branch {
                MemberRole::Member
            } else if is_tag {
                // Tag "move" (overwrite) = Admin.
                MemberRole::Admin
            } else {
                MemberRole::Admin
            }
        }
        UpdateKind::NonFastForward => MemberRole::Admin,
        UpdateKind::Delete => MemberRole::Admin,
    }
}

/// The git role a project role confers on the project's repositories.
///
/// A repository whose kind:30617 carries a `["project", …]` back-reference
/// authorizes against the project's curated roster *as well as* its
/// `buzz-channel` binding — whichever grants more ([`max_git_role`]). The
/// mapping is deliberately narrow:
///
/// | Project role   | Git role               | Why |
/// |----------------|------------------------|-----|
/// | `Owner`        | [`MemberRole::Owner`]  | Roster control over the project is the same authority a channel owner holds over its repos. |
/// | `Collaborator` | [`MemberRole::Member`] | NIP-MP's "write into project contents" is ordinary push, not administration. |
/// | `Viewer`       | `None`                 | Read-only across the project; grants no push. |
///
/// `None` means "this role grants no push", **not** "deny": the caller still
/// consults the channel binding, which may grant independently.
///
/// `buzz-protect` rules are unaffected — they constrain every pusher
/// including an owner, whichever path the role arrived through.
pub fn git_role_for_project_role(role: ProjectRole) -> Option<MemberRole> {
    match role {
        ProjectRole::Owner => Some(MemberRole::Owner),
        ProjectRole::Collaborator => Some(MemberRole::Member),
        ProjectRole::Viewer => None,
    }
}

/// The more permissive of two git roles held through different paths
/// (project roster vs. bound channel).
///
/// The two ACLs are additive, so the effective role is the maximum: a channel
/// Admin must not be demoted by also being a project Collaborator, and a
/// project Owner must not be demoted by also being a channel Guest.
///
/// `Bot` is outside the hierarchy (`permission_level() == 0`) and callers
/// normalize it to `Member` *before* ranking. An un-normalized `Bot` passed
/// here loses to every other role, which is the fail-closed direction.
pub fn max_git_role(a: MemberRole, b: MemberRole) -> MemberRole {
    if b.permission_level() > a.permission_level() {
        b
    } else {
        a
    }
}

/// The effective constraints for a ref after unioning all matching rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveRules {
    /// Strictest `push:<role>` from all matching patterns (if any).
    pub push_role: Option<MemberRole>,
    /// Whether non-fast-forward is forbidden (any match sets this).
    pub no_force_push: bool,
    /// Whether deletion is forbidden (any match sets this).
    pub no_delete: bool,
    /// Whether direct push is denied (any match sets this).
    pub require_patch: bool,
    /// Whether an approved mission verdict must name the pushed commit (any
    /// match sets this). Enforced by the relay's stored-event search, never
    /// by [`evaluate_ref_update`].
    pub require_verdict: bool,
    /// Whether any explicit rule matched (vs. using defaults).
    pub has_explicit_match: bool,
}

impl EffectiveRules {
    /// Compute effective rules by unioning all protection rules that match a ref.
    pub fn for_ref(ref_name: &str, rules: &[ProtectionRule]) -> Self {
        let mut push_role: Option<MemberRole> = None;
        let mut no_force_push = false;
        let mut no_delete = false;
        let mut require_patch = false;
        let mut require_verdict = false;
        let mut has_explicit_match = false;

        for rule in rules {
            if !rule.pattern.matches(ref_name) {
                continue;
            }
            // A cleared row is the operator saying this pattern carries no
            // rules, so it must not count as an explicit match either —
            // otherwise "removed the protection" would silently leave the ref
            // guarded (`ref_is_guarded` in the relay's push policy reads
            // exactly this flag). Resolution normally drops these before we
            // get here; an unresolved list is handled the same way so the two
            // paths cannot disagree.
            if rule.cleared {
                continue;
            }
            has_explicit_match = true;

            // Union: take strictest push role.
            if let Some(role) = rule.push_role {
                push_role = Some(match push_role {
                    None => role,
                    Some(existing) => {
                        if role.permission_level() > existing.permission_level() {
                            role
                        } else {
                            existing
                        }
                    }
                });
            }

            // Union: any match sets these flags.
            no_force_push = no_force_push || rule.no_force_push;
            no_delete = no_delete || rule.no_delete;
            require_patch = require_patch || rule.require_patch;
            require_verdict = require_verdict || rule.require_verdict;
        }

        Self {
            push_role,
            no_force_push,
            no_delete,
            require_patch,
            require_verdict,
            has_explicit_match,
        }
    }
}

/// A single denial reason from the policy engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    /// The ref that was denied.
    pub ref_name: String,
    /// Human-readable reason.
    pub reason: String,
}

impl fmt::Display for Denial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.ref_name, self.reason)
    }
}

/// Evaluate a single ref update against effective rules and the pusher's role.
///
/// Returns `Ok(())` if allowed, `Err(Denial)` if denied.
pub fn evaluate_ref_update(
    update: &RefUpdate,
    role: MemberRole,
    rules: &[ProtectionRule],
) -> Result<(), Denial> {
    let effective = EffectiveRules::for_ref(&update.ref_name, rules);

    // If no explicit rules match, use built-in defaults.
    if !effective.has_explicit_match {
        let min_role = default_min_role(&update.ref_name, update.kind);
        if !role.has_at_least(min_role) {
            return Err(Denial {
                ref_name: update.ref_name.clone(),
                reason: format!(
                    "requires {} role (you have {}), using built-in defaults",
                    min_role, role
                ),
            });
        }
        return Ok(());
    }

    // Check require-patch (blocks all direct pushes).
    if effective.require_patch {
        return Err(Denial {
            ref_name: update.ref_name.clone(),
            reason: "direct push denied: require-patch is set, submit a NIP-34 patch".to_string(),
        });
    }

    // Check push role.
    // Explicit push:role can NEVER weaken the built-in default. Always take the
    // HIGHER of (explicit, default). This prevents `push:member` from accidentally
    // allowing Members to force-push, delete, or overwrite tags.
    let default_role = default_min_role(&update.ref_name, update.kind);
    let min_role = match effective.push_role {
        Some(explicit) => {
            // Take the stricter (higher permission level) of explicit vs default.
            if explicit.permission_level() >= default_role.permission_level() {
                explicit
            } else {
                default_role
            }
        }
        None => default_role,
    };
    if !role.has_at_least(min_role) {
        return Err(Denial {
            ref_name: update.ref_name.clone(),
            reason: format!("requires {} role (you have {})", min_role, role),
        });
    }

    // Check no-force-push.
    if effective.no_force_push && update.kind == UpdateKind::NonFastForward {
        return Err(Denial {
            ref_name: update.ref_name.clone(),
            reason: "non-fast-forward update denied: no-force-push is set".to_string(),
        });
    }

    // Check no-delete.
    if effective.no_delete && update.kind == UpdateKind::Delete {
        return Err(Denial {
            ref_name: update.ref_name.clone(),
            reason: "ref deletion denied: no-delete is set".to_string(),
        });
    }

    Ok(())
}

/// Evaluate an entire push (multiple ref updates) against protection rules.
///
/// Returns `Ok(())` if ALL refs are allowed, `Err(Vec<Denial>)` if any are denied.
/// A push is atomic — if any ref fails, the entire push is rejected.
pub fn evaluate_push(
    updates: &[RefUpdate],
    role: MemberRole,
    rules: &[ProtectionRule],
) -> Result<(), Vec<Denial>> {
    let denials: Vec<Denial> = updates
        .iter()
        .filter_map(|update| evaluate_ref_update(update, role, rules).err())
        .collect();

    if denials.is_empty() {
        Ok(())
    } else {
        Err(denials)
    }
}

#[cfg(test)]
#[path = "git_perms_tests.rs"]
mod tests;
