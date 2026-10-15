//! The role names a project's team manifest gives the authority rules.
//!
//! Until round 2 of the headless-pluggable slice (2026-10-14) the literal
//! role names `"lead"`, `"verifier"` and `"runner"` gated authority in core,
//! db, relay, CLI and provider, and `team.yml lead:` was honoured in exactly
//! one place. A project whose manifest named its lead `captain` could publish
//! a role file by that name and hire into it, but the fold would refuse that
//! seat's assignments, the relay would refuse its hires, and the provider
//! would never wake it: the shipped foundation, not the project's own agents
//! repository, decided what the roles were called (VISION_COLLABORATION § Roles
//! evolve with the project).
//!
//! This type is the one place the names live. It is **built from the team
//! record** — `team.yml`'s `lead:` and `verification_roles:` — by
//! `beekeeper_persona::team::TeamManifest::role_vocabulary`, and carried to
//! every decision point that used to compare against a literal. A caller that
//! holds no team record uses [`RoleVocabulary::foundation`], which is the
//! default vocabulary and is deliberately greppable: every such call site is
//! a place the project's manifest has not yet reached.
//!
//! # The two notions a vocabulary separates
//!
//! - **The lead** (`lead:`): the one role that may assign work, dispose of
//!   reports, declare a mission completed or blocked, hire non-lead seats,
//!   hold a project-action delegation, and receive the team wake.
//! - **The verification roles** (`verification_roles:`): every role whose
//!   assignment is a claim about a particular commit and so must name it
//!   (`baseSha`), and whose seat is fenced onto that commit before its turn
//!   opens. The **first** entry is the judging role — the one whose
//!   `refutation` verdicts the fold authorizes and whose ruling satisfies
//!   `gates.verifierRequired`. The default `[verifier, runner]` keeps today's
//!   exact split: a `runner` must name its commit but does not refute.
//!
//! # The default rule
//!
//! `lead` when `lead:` is absent; `[verifier, runner]` when
//! `verification_roles:` is absent. Beekeeper's own project sets neither, so
//! nothing about it changes.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// The lead role when the team manifest names none.
pub const DEFAULT_LEAD_ROLE: &str = "lead";

/// The verification roles when the team manifest names none, judging role
/// first.
pub const DEFAULT_VERIFICATION_ROLES: [&str; 2] = ["verifier", "runner"];

/// The role names one project's authority rules compare against.
///
/// Exact, lowercase tokens, compared whole — a seat holding `verifier-2` is
/// not the judging seat, exactly as it was not before this type existed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleVocabulary {
    lead: String,
    verification: Vec<String>,
}

impl Default for RoleVocabulary {
    fn default() -> Self {
        Self {
            lead: DEFAULT_LEAD_ROLE.to_owned(),
            verification: DEFAULT_VERIFICATION_ROLES
                .iter()
                .map(|role| (*role).to_owned())
                .collect(),
        }
    }
}

impl RoleVocabulary {
    /// The default vocabulary, shared.
    ///
    /// For callers that hold no team record — the relay at ingest, the db's
    /// chain insert, a CLI invoked outside any seat. Every call is a site the
    /// project's own manifest has not reached; grep for it before claiming a
    /// surface honours `team.yml`.
    pub fn foundation() -> &'static RoleVocabulary {
        static FOUNDATION: OnceLock<RoleVocabulary> = OnceLock::new();
        FOUNDATION.get_or_init(RoleVocabulary::default)
    }

    /// Build from a team record's fields, applying the default rule.
    ///
    /// `lead` is `team.yml lead:`; `verification` is `verification_roles:`.
    /// `None` for either means the field was absent and the default applies.
    ///
    /// # Errors
    /// A role that is not a slug (1–64 bytes of `[a-z0-9-]`), an explicitly
    /// empty verification list (omit the key for the default; an empty list
    /// would make `gates.verifierRequired` unsatisfiable while looking
    /// configured), or a lead that is also a verification role (one seat
    /// cannot both dispose of a report and refute it).
    pub fn from_team(lead: Option<&str>, verification: Option<&[String]>) -> Result<Self, String> {
        let lead = match lead {
            None => DEFAULT_LEAD_ROLE.to_owned(),
            Some(role) => {
                validate_role_token("lead", role)?;
                role.to_owned()
            }
        };
        let verification: Vec<String> = match verification {
            None => DEFAULT_VERIFICATION_ROLES
                .iter()
                .map(|role| (*role).to_owned())
                .collect(),
            Some([]) => {
                return Err(
                    "verification_roles names no role; omit the key for the default".to_owned(),
                )
            }
            Some(roles) => {
                for role in roles {
                    validate_role_token("verification_roles", role)?;
                }
                let mut seen = std::collections::BTreeSet::new();
                for role in roles {
                    if !seen.insert(role.as_str()) {
                        return Err(format!("verification_roles names {role:?} twice"));
                    }
                }
                roles.to_vec()
            }
        };
        if verification.contains(&lead) {
            return Err(format!(
                "lead {lead:?} is also a verification role; a seat cannot both dispose of a \
                 report and refute it"
            ));
        }
        Ok(Self { lead, verification })
    }

    /// The lead role.
    pub fn lead(&self) -> &str {
        &self.lead
    }

    /// Whether `role` is the lead role, compared exactly.
    pub fn is_lead(&self, role: &str) -> bool {
        role == self.lead
    }

    /// The judging role: the first verification role, whose `refutation`
    /// verdicts the fold authorizes and whose ruling clears a completion.
    pub fn verdict_role(&self) -> &str {
        // `from_team` refuses an empty list and `Default` has two entries,
        // so the first is always there; the fallback is unreachable and
        // written out only so no `unwrap` lives on this path.
        self.verification
            .first()
            .map(String::as_str)
            .unwrap_or(DEFAULT_VERIFICATION_ROLES[0])
    }

    /// Whether `role` is the judging role, compared exactly.
    pub fn is_verdict_role(&self, role: &str) -> bool {
        role == self.verdict_role()
    }

    /// Every verification role, judging role first.
    pub fn verification_roles(&self) -> &[String] {
        &self.verification
    }

    /// Whether an assignment in `role` is a claim about a particular commit:
    /// it must name `baseSha`, and the seat is fenced onto that commit.
    pub fn verifies_a_commit(&self, role: &str) -> bool {
        self.verification.iter().any(|known| known == role)
    }

    /// Whether this is the default vocabulary.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn validate_role_token(field: &str, role: &str) -> Result<(), String> {
    if role.is_empty()
        || role.len() > 64
        || !role
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(format!(
            "{field}: a role is 1-64 bytes of [a-z0-9-], got {role:?}"
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "role_vocabulary_tests.rs"]
mod tests;
