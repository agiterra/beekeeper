//! Who founded a repository — the set of keys that speak for it.
//!
//! Finding 33 (2026-09-03): the `require-verdict` gate keyed every authority
//! question to the kind:30617 announcement's signer. `agiterra-beekeeper` is
//! signed by one human and co-owned by two, so the rule as landed would have
//! admitted no ruling from the other founder's missions and refused their
//! landing pushes. Brian's direction: *"Andy and I are equal owners in
//! agiterra. All code is ours."*
//!
//! # The set
//!
//! A repository's **founders** are, in order:
//!
//! 1. the announcement's **signer** — always, and always first;
//! 2. every pubkey in its NIP-34 `["maintainers", <hex>, <hex>, …]` tag;
//! 3. every pubkey whose effective git role on the repository resolves to
//!    **Owner** through the project roster its `["project", …]` back-reference
//!    names (commit `a56ad5d01`: the roster is a first-class git ACL and
//!    project owner → [`crate::channel::MemberRole::Owner`] via
//!    [`crate::git_perms::git_role_for_project_role`]).
//!
//! (1) and (2) are on the signed announcement, so anyone holding it can derive
//! them. (3) needs the roster, which lives in relay storage and on kinds
//! 9010/9011 — [`RepositoryFounders`] therefore carries whether that half was
//! read ([`RepositoryFounders::roster_owners_read`]), and every surface that
//! shows a founder set says so rather than presenting a partial set as whole.
//!
//! # Rules
//!
//! The `buzz-protect` rows on the announcement can still only be rewritten by
//! its signer — a replacement kind:30617 signed by a co-founder is a different
//! addressable event at a different author, not an edit. Since lane L26 that
//! is no longer the whole story: **any** founder may set or remove a rule by
//! signing a rule record (kind 30625,
//! [`crate::repository_protection`]), which the push gate resolves against the
//! announcement's rows with last-write-wins per exact ref pattern. So the
//! answer to "who governs this repository's refs" is the founder set, and
//! [`RepositoryFounders::rules_sentence`] says so — it used to say the signer
//! alone, which was true when it was written and would be a lie now.
//!
//! Membership-based push (the channel binding and the roster's own tiers) is
//! untouched: this type answers *"who founded it"*, never *"who may push"*.

use nostr::Event;

use crate::channel::{MemberRole, ProjectRole};
use crate::git_perms::git_role_for_project_role;

/// NIP-34 multi-value tag naming a repository's maintainers.
pub const REPOSITORY_MAINTAINERS_TAG: &str = "maintainers";

/// The keys that speak for a repository.
///
/// Construct with [`RepositoryFounders::from_announcement`] (or
/// [`RepositoryFounders::from_parts`] when only the signer and tag list
/// reached the caller), then add the roster half with
/// [`RepositoryFounders::with_roster_owners`] if it could be read. A value
/// that never had the roster added reports `roster_owners_read() == None`, and
/// that `None` is a fact to disclose, not a zero to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryFounders {
    signer: String,
    founders: Vec<String>,
    maintainers_declared: usize,
    invalid_maintainers: usize,
    roster_owners_read: Option<usize>,
}

impl RepositoryFounders {
    /// Derive the announcement half of the set: signer ∪ `maintainers`.
    pub fn from_announcement(event: &Event) -> Self {
        let tags: Vec<Vec<String>> = event
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect();
        Self::from_parts(&event.pubkey.to_hex(), &tags)
    }

    /// The same derivation for a caller holding the signer and tag list
    /// separately — the desktop reads exactly that over the wire.
    ///
    /// A malformed maintainer value is **ignored and counted**
    /// ([`RepositoryFounders::invalid_maintainers`]) rather than dropped in
    /// silence: a typo in a co-founder's key is the difference between two
    /// founders and one, and nothing else would ever tell them.
    pub fn from_parts(signer_pubkey: &str, tags: &[Vec<String>]) -> Self {
        let signer = signer_pubkey.to_ascii_lowercase();
        let mut founders: Vec<String> = Vec::new();
        let mut maintainers_declared = 0usize;
        let mut invalid_maintainers = 0usize;
        if is_hex64(&signer) {
            founders.push(signer.clone());
        }
        for tag in tags {
            let [name, values @ ..] = tag.as_slice() else {
                continue;
            };
            if name != REPOSITORY_MAINTAINERS_TAG {
                continue;
            }
            for value in values {
                maintainers_declared += 1;
                let candidate = value.trim().to_ascii_lowercase();
                if !is_hex64(&candidate) {
                    invalid_maintainers += 1;
                    continue;
                }
                push_unique(&mut founders, candidate);
            }
        }
        Self {
            signer,
            founders,
            maintainers_declared,
            invalid_maintainers,
            // Not read: this constructor saw only the announcement.
            roster_owners_read: None,
        }
    }

    /// Add the project-roster half: every pubkey the roster grants git Owner.
    ///
    /// Pass the roster's roles, not a pre-filtered list, so the Owner test is
    /// [`git_role_for_project_role`] — Andy's model in `a56ad5d01` — in one
    /// place rather than re-typed per caller. Calling this at all records that
    /// the roster **was read**, even when it named no additional owner.
    pub fn with_roster_roles<I>(mut self, roster: I) -> Self
    where
        I: IntoIterator<Item = (String, ProjectRole)>,
    {
        let mut added = 0usize;
        for (pubkey, role) in roster {
            if git_role_for_project_role(role) != Some(MemberRole::Owner) {
                continue;
            }
            let candidate = pubkey.trim().to_ascii_lowercase();
            if !is_hex64(&candidate) {
                continue;
            }
            if push_unique(&mut self.founders, candidate) {
                added += 1;
            }
        }
        self.roster_owners_read = Some(added);
        self
    }

    /// Add roster owners already filtered to git-Owner tier.
    ///
    /// The convenience form for a caller that resolved the tier itself (the
    /// relay's push policy asks the same question per principal). Recording
    /// the read is the point: an empty iterator still means *read*.
    pub fn with_roster_owners<I>(self, owners: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        self.with_roster_roles(
            owners
                .into_iter()
                .map(|pubkey| (pubkey, ProjectRole::Owner)),
        )
    }

    /// Every founder, lower-hex, deduped, signer first.
    pub fn pubkeys(&self) -> &[String] {
        &self.founders
    }

    /// Whether this key founded the repository. Case-folded, whole-string.
    pub fn contains(&self, pubkey: &str) -> bool {
        self.founders
            .iter()
            .any(|founder| founder.eq_ignore_ascii_case(pubkey.trim()))
    }

    /// How many founders the set holds.
    pub fn len(&self) -> usize {
        self.founders.len()
    }

    /// Whether the set is empty — only possible from an unparseable signer.
    pub fn is_empty(&self) -> bool {
        self.founders.is_empty()
    }

    /// The announcement's signer: the one key that may rewrite the rules in v1.
    pub fn signer(&self) -> &str {
        &self.signer
    }

    /// How many values the `maintainers` tag(s) declared, valid or not.
    pub fn maintainers_declared(&self) -> usize {
        self.maintainers_declared
    }

    /// How many declared maintainers were not 64-hex and were ignored.
    pub fn invalid_maintainers(&self) -> usize {
        self.invalid_maintainers
    }

    /// `Some(n)` when the project roster was read (`n` = founders it added),
    /// `None` when this caller could not read it.
    pub fn roster_owners_read(&self) -> Option<usize> {
        self.roster_owners_read
    }

    /// The sentence a CLI or a screen prints next to a repository's rules.
    ///
    /// Three facts, none of them flattering to omit: who may set or remove a
    /// rule (any founder, by signing a rule record — kind 30625), who the
    /// founders are, and whether the roster half of that set was read here.
    ///
    /// Before lane L26 this said "the announcement's signer, and only that
    /// key can rewrite them". That was true then and is false now, and a
    /// sentence that keeps saying it would be exactly the kind of control
    /// that lies about what it enforces.
    pub fn rules_sentence(&self) -> String {
        let list = if self.founders.is_empty() {
            "none — this announcement has no parseable signer".to_string()
        } else {
            self.founders.join(", ")
        };
        let roster = match self.roster_owners_read {
            Some(_) => String::new(),
            None => " The project roster was not read here, so an Owner on the project this \
                     repository belongs to is not listed."
                .to_string(),
        };
        let invalid = if self.invalid_maintainers == 0 {
            String::new()
        } else {
            format!(
                " {} maintainer value(s) were not 64-hex and were ignored.",
                self.invalid_maintainers
            )
        };
        format!(
            "rules are set by any founder, as a signed rule record; the announcement's own rows \
             stay with its signer {}; founders of this repository are {} ({}).{}{}",
            self.signer,
            list,
            self.founders.len(),
            roster,
            invalid
        )
    }
}

/// Append `candidate` unless an equal-hex entry is already present.
///
/// Returns whether it was appended, so a caller can count what it added.
fn push_unique(founders: &mut Vec<String>, candidate: String) -> bool {
    if founders
        .iter()
        .any(|founder| founder.eq_ignore_ascii_case(&candidate))
    {
        return false;
    }
    founders.push(candidate);
    true
}

/// Whether a string is exactly 64 ASCII hex digits.
fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "repository_founders_tests.rs"]
mod tests;
