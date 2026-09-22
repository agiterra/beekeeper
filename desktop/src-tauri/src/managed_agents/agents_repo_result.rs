//! What [`super::agents_repo::project_agents_init`] reports, and how it
//! decides whether the sequence finished.
//!
//! Split from `agents_repo.rs` for the file-size gate; the type is the
//! command's whole answer, so it is one place on purpose. Every field is a
//! wire fact the run observed. `complete` is true only when every step
//! landed, and `gap` is one sentence naming the first that did not — so a
//! screen can print the host's own verdict rather than composing a happier
//! one.

use serde::Serialize;

use crate::managed_agents::packs_repo::SEED_BRANCH;

/// What creating a project's repositories actually produced — every wire
/// fact, and `gap` when the sequence did not finish.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAgentsInit {
    /// The project coordinate this ran for.
    pub project_ref: String,
    /// `30617:<viewer>:<slug>`.
    pub code_repo_ref: String,
    /// The code repository's `d` tag.
    pub code_repo_id: String,
    /// Event id of the code repository's announcement when this run
    /// published it; `null` when it already existed or was not reached.
    pub code_announcement_event_id: Option<String>,
    /// The code repository was already announced under the viewer's key.
    pub code_repo_existed: bool,
    /// The code repository named by the project's own head rather than
    /// derived from its slug — a project created before the pivot keeps the
    /// repository it already has (`false` when the slug was used).
    pub code_repo_adopted: bool,
    /// The code repository's seed commit (its `README.md` on `main`), or
    /// `null` when seeding failed or was skipped.
    pub code_seed_commit_sha: Option<String>,
    /// The relay already held a push record for the code repository, so
    /// nothing was seeded or pushed to it.
    pub code_seed_skipped: bool,
    /// The code seed's or its push's own words when the seed is not on the
    /// relay.
    pub code_seed_error: Option<String>,
    /// `30617:<viewer>:<slug>-beekeeper-agents`.
    pub agents_repo_ref: String,
    /// The agents repository's `d` tag.
    pub agents_repo_id: String,
    /// The relay git URL the agents repository is served at.
    pub agents_clone_url: String,
    /// Event id of the agents repository's announcement when this run
    /// published it.
    pub agents_announcement_event_id: Option<String>,
    /// The agents repository was already announced under the viewer's key.
    pub agents_repo_existed: bool,
    /// The branch the seed is on.
    pub branch: String,
    /// The roles seeded, ascending; empty when the seed was skipped or failed.
    pub roles: Vec<String>,
    /// The seed commit, or `null` when seeding failed or was skipped.
    pub seed_commit_sha: Option<String>,
    /// The seed step's own words when it failed before there was a commit.
    pub seed_error: Option<String>,
    /// The relay already held a push record, so nothing was seeded or pushed.
    pub seed_skipped: bool,
    /// Whether the agents repository holds the seed on the relay — pushed
    /// by this run, or already there.
    pub pushed: bool,
    /// The push's own words when it did not land.
    pub push_error: Option<String>,
    /// The relay-signed kind:30618 recording the push, read back; never
    /// fabricated.
    pub push_record_event_id: Option<String>,
    /// Event id of the kind:30624 this run published.
    pub source_event_id: Option<String>,
    /// The project already had a pack source naming the agents repository.
    pub source_existed: bool,
    /// The repository coordinate this run migrated the project *off*, when
    /// it was asked to; `null` for an ordinary create or finish.
    pub migrated_from: Option<String>,
    /// The roles converted out of that repository, ascending.
    pub migrated_roles: Vec<String>,
    /// What the conversion could not carry across, one sentence per role;
    /// empty when nothing was dropped.
    pub migration_notes: Vec<String>,
    /// The relay refused the conditional source because the project's
    /// source had moved since the caller read it. Everything else stands;
    /// nothing was re-pointed.
    pub source_conflict: bool,
    /// The relay's refusal of a published event, when one was refused.
    pub publication_error: Option<String>,
    /// The identity the seed commit was (or would have been) authored as.
    pub commit_identity_name: String,
    pub commit_identity_email: String,
    /// Event id of the kind:5 withdrawing this run's agents announcement
    /// after its seed or push failed.
    pub agents_announcement_withdrawn_event_id: Option<String>,
    /// The withdrawal's own words when the tombstone itself failed.
    pub agents_announcement_withdrawal_error: Option<String>,
    /// This host's checkout of the code repository, recorded as the
    /// project's folder; `null` when there is none.
    pub checkout_path: Option<String>,
    /// `checkout_path` was cloned by this run — `false` when an existing
    /// checkout was reused or was already recorded.
    pub checkout_cloned: bool,
    /// Why there is no recorded checkout, in words: the clone failed, the
    /// record failed, or the recorded folder is not a checkout of this
    /// repository (named, not overwritten).
    pub checkout_error: Option<String>,
    /// The agent pubkeys this run put on the project's roster.
    pub roster_added: Vec<String>,
    /// Why some project agents are not on the roster after this run.
    pub roster_error: Option<String>,
    /// Both repositories announced and seeded, the source set, the checkout
    /// recorded, the roster complete.
    pub complete: bool,
    /// One sentence naming what is missing when `complete` is `false`.
    pub gap: Option<String>,
    /// The project's default agents this computer installed from the
    /// seeded team, in role order (spec § 4.11). Empty when the seed did
    /// not land or the install failed — see `agents_error`.
    pub agents_installed: Vec<crate::managed_agents::default_agents::InstalledDefaultAgent>,
    /// Why no agents were installed, when `agents_installed` is empty after
    /// a seed that landed.
    pub agents_error: Option<String>,
}

impl ProjectAgentsInit {
    pub(crate) fn started(
        project_ref: &str,
        viewer: &str,
        code_repo_id: &str,
        agents_repo_id: &str,
        agents_clone_url: &str,
        identity: (String, String),
    ) -> Self {
        Self {
            project_ref: project_ref.to_string(),
            code_repo_ref: format!("30617:{viewer}:{code_repo_id}"),
            code_repo_id: code_repo_id.to_string(),
            code_announcement_event_id: None,
            code_repo_existed: false,
            code_repo_adopted: false,
            code_seed_commit_sha: None,
            code_seed_skipped: false,
            code_seed_error: None,
            agents_repo_ref: format!("30617:{viewer}:{agents_repo_id}"),
            agents_repo_id: agents_repo_id.to_string(),
            agents_clone_url: agents_clone_url.to_string(),
            agents_announcement_event_id: None,
            agents_repo_existed: false,
            branch: SEED_BRANCH.to_string(),
            roles: Vec::new(),
            seed_commit_sha: None,
            seed_error: None,
            seed_skipped: false,
            pushed: false,
            push_error: None,
            push_record_event_id: None,
            source_event_id: None,
            source_existed: false,
            migrated_from: None,
            migrated_roles: Vec::new(),
            migration_notes: Vec::new(),
            source_conflict: false,
            publication_error: None,
            commit_identity_name: identity.0,
            commit_identity_email: identity.1,
            agents_announcement_withdrawn_event_id: None,
            agents_announcement_withdrawal_error: None,
            checkout_path: None,
            checkout_cloned: false,
            checkout_error: None,
            roster_added: Vec::new(),
            roster_error: None,
            complete: false,
            gap: None,
            agents_installed: Vec::new(),
            agents_error: None,
        }
    }

    /// The code repository holds its seed on the relay — pushed by this run
    /// or already there.
    pub(crate) fn code_seeded(&self) -> bool {
        self.code_seed_skipped
            || (self.code_seed_commit_sha.is_some() && self.code_seed_error.is_none())
    }

    /// Settle `complete` and `gap` from the facts. An announcement this run
    /// withdrew counts as not announced: the next run announces it again.
    /// Re-callable: the command settles again once the roster step ran.
    pub(crate) fn settle(mut self) -> Self {
        let code_ok = self.code_repo_existed || self.code_announcement_event_id.is_some();
        let agents_ok = self.agents_repo_existed
            || (self.agents_announcement_event_id.is_some()
                && self.agents_announcement_withdrawn_event_id.is_none());
        let source_ok = self.source_existed || self.source_event_id.is_some();
        let checkout_ok = self.checkout_path.is_some() && self.checkout_error.is_none();
        let roster_ok = self.roster_error.is_none();
        self.complete = code_ok
            && self.code_seeded()
            && agents_ok
            && self.pushed
            && source_ok
            && checkout_ok
            && roster_ok;
        self.gap = if self.complete {
            None
        } else if !code_ok {
            Some(format!(
                "code repository {} not announced: {}",
                self.code_repo_id,
                self.publication_error.as_deref().unwrap_or("not reached")
            ))
        } else if !self.code_seeded() {
            Some(format!(
                "code repository {} not seeded: {}",
                self.code_repo_id,
                self.code_seed_error.as_deref().unwrap_or("not reached")
            ))
        } else if !self.pushed {
            Some(format!(
                "agents repository {} not seeded: {}",
                self.agents_repo_id,
                self.seed_error
                    .as_deref()
                    .or(self.push_error.as_deref())
                    .or(self.publication_error.as_deref())
                    .unwrap_or("not reached")
            ))
        } else if !agents_ok {
            Some(format!(
                "agents repository {} not announced: {}",
                self.agents_repo_id,
                self.publication_error.as_deref().unwrap_or("not reached")
            ))
        } else if !source_ok {
            Some(if self.source_conflict {
                format!(
                    "the project's role source moved while this ran, so it was not re-pointed at \
                     {}; read the source again and decide against what is there now. Everything \
                     else landed",
                    self.agents_repo_id
                )
            } else {
                format!(
                    "pack source not set: {}",
                    self.publication_error.as_deref().unwrap_or("not reached")
                )
            })
        } else if !checkout_ok {
            Some(format!(
                "code repository {} not checked out as the project's folder: {}",
                self.code_repo_id,
                self.checkout_error.as_deref().unwrap_or("not reached")
            ))
        } else {
            Some(format!(
                "project agents not all on the roster: {}",
                self.roster_error.as_deref().unwrap_or("not reached")
            ))
        };
        self
    }
}
