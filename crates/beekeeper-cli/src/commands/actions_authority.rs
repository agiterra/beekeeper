//! Whether the caller may actually publish a project's actions and start a
//! manual run of one — what `bee actions status` reports beside each entry.
//!
//! # The defect this exists for (ledger 186, finding 178(f))
//!
//! `bee actions status` used to parse `actions.yml` and print what it *would*
//! publish. It never said whether the key holding it could publish anything at
//! all. Saving a project-bound kind:30620 is admitted only for the project's
//! creator, a roster Owner or a founder of one of its endorsed repositories
//! (`crates/beekeeper-relay/src/handlers/command_executor.rs:822-862`), and starting
//! a manual kind:46020 run only for the workflow's owner or a project
//! Owner/Collaborator (`:1018-1042`). A seat holds none of those, so a team
//! session accepted the goal "…and a verify action" that it had no standing to
//! finish, and only discovered it at the refusal.
//!
//! # Shape: a pure decision over inputs that were already read
//!
//! The reads live in [`read_project_action_authority`] and every failure they
//! meet becomes a *named* non-answer in the inputs, never a `false`. The rules
//! live in [`decide_project_action_authority`], which touches no network and is
//! therefore testable against a hand-built input — the same split the verdict
//! prediction uses, and the reason a stub can exercise the grant path without
//! a relay.
//!
//! # What it does not read, and says so
//!
//! This is a prediction of the relay's answer, and it is narrower than the
//! relay's rule on purpose:
//!
//! - an **endorsed repository's founder** standing is not read here, so a
//!   founder who holds no roster row reads as "nothing found";
//! - the **channel owner/admin** role is not read, which matters twice: it is
//!   an *additional* requirement for any entry with a `run_on_host` or
//!   `call_webhook` step (`command_executor.rs:789-818`), and it is what a
//!   community owner would otherwise be admitted by.
//!
//! Both are named in the basis rather than guessed at, because a control that
//! says `false` where the relay says yes is the same class of bug as one that
//! says `true` where the relay refuses.

use std::collections::BTreeMap;

use beekeeper_core::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionPayload,
};
use beekeeper_core::coding_session_project_action_grant::{
    find_project_action_grant, fold_project_action_grants, ProjectActionCapability,
    ProjectActionGrant, ProjectActionGrantLink,
};
use beekeeper_core::kind::{
    is_valid_project_role, KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_PROJECT_MEMBERS,
    PROJECT_ROLE_OWNER,
};

use crate::client::BuzzClient;

/// How many kind:44228 transitions one status read may page through.
///
/// The chains are filtered by `#h` (a real Nostr filter), so this only bounds
/// a pathologically long-lived channel. Reading fewer links can only make a
/// chain look non-contiguous, which this module discloses rather than folds.
const MAX_CHANNEL_AUTHORITY_TRANSITIONS: u32 = 512;

/// What the named channel's kind:44228 authority chains say about grants — or
/// why they say nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityChainRead {
    /// No `--channel` was given, so there was no chain to look in. A project's
    /// grants live in a session's chain, and a session lives in a channel;
    /// without one there is nothing to read and nothing to conclude.
    NoChannel,
    /// The channel's chains were read and folded.
    Chains {
        /// Every live grant the contiguous chains in this channel carry.
        grants: Vec<ProjectActionGrant>,
        /// Chains discarded because their readable links are not contiguous.
        ///
        /// A partial chain must not activate a link: the missing links could
        /// be the revocation. Counted so a "nothing found" can disclose that
        /// something was skipped instead of claiming the channel is empty.
        discarded_chains: usize,
        /// Events under this `h` tag whose transition payload did not decode.
        unread_links: usize,
    },
    /// The read itself failed; the string is the failure, in the relay's words.
    Failed(String),
}

/// What the project's relay-signed kind:39010 roster projection says — or why
/// it says nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectRosterRead {
    /// The projection exists and grants these pubkeys (lowercase hex) the
    /// **Owner** role. The creator is not among them: they hold no roster row
    /// by construction and are decided earlier, from the coordinate itself.
    Owners(Vec<String>),
    /// No projection has been emitted for this coordinate. The relay would
    /// still answer from the project head's own `p` tags, which is not read
    /// here, so ownership is unknown rather than absent.
    NoProjection,
    /// The read itself failed; the string is the failure.
    Failed(String),
}

/// Everything [`decide_project_action_authority`] is allowed to look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectActionAuthorityInputs {
    /// The caller's signing pubkey, lowercase hex.
    ///
    /// The relay checks the *authenticated* key
    /// (`command_executor.rs:380`), not a NIP-OA owner, so a seat is judged
    /// by its own key even when it signs on an owner's behalf.
    pub caller_pubkey: String,
    /// The project coordinate under test, `30621:<64-hex owner>:<d>`.
    pub project_ref: String,
    /// What the channel's authority chains said.
    pub chain: AuthorityChainRead,
    /// What the project's roster projection said.
    pub roster: ProjectRosterRead,
}

/// The answer for one project: two predictions and the fact behind them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectActionAuthority {
    /// Whether a project-bound kind:30620 from this key would be admitted.
    /// `None` means the relay could not be asked — never a guess.
    pub may_publish: Option<bool>,
    /// Whether a manual kind:46020 run from this key would be admitted.
    pub may_trigger: Option<bool>,
    /// The fact that decided it, or the read that could not be made.
    pub basis: String,
    /// What would make it true, when anything would.
    pub remedy: Option<String>,
    /// Whether the answer rests on a live project-actions delegation rather
    /// than on the caller's own project standing. Read by
    /// [`Self::narrowed_by_channel_elevation`].
    pub rests_on_delegation: bool,
}

impl ProjectActionAuthority {
    /// The `authority` object `bee actions status` prints: `{basis, remedy}`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "basis": self.basis,
            "remedy": self.remedy,
        })
    }

    /// Withdraw a `may_publish: true` for an entry that *also* needs the
    /// channel owner/admin role, which this module does not read.
    ///
    /// A `run_on_host` or `call_webhook` step is admitted on channel standing
    /// on top of project standing (`command_executor.rs`, the `has_host_steps`
    /// check), so project standing alone cannot promise the publish. A `false`
    /// is left alone: project standing is *necessary* for a project-bound
    /// definition, so its absence already settles the refusal.
    ///
    /// **Not** withdrawn when the answer rests on a live project-actions
    /// delegation: the host-step check accepts that same delegation for a
    /// project-bound definition (ledger 186), so a delegated `true` is a whole
    /// answer and narrowing it here would report a doubt the relay does not
    /// have. A `call_webhook` step is a different matter — no delegation
    /// reaches it — and this command does not distinguish the two steps, so a
    /// delegated `true` over a `call_webhook` entry is the one case this line
    /// is more confident than the relay. It is named here rather than left for
    /// a reader to find.
    pub fn narrowed_by_channel_elevation(mut self, required: bool) -> Self {
        if required && !self.rests_on_delegation && self.may_publish == Some(true) {
            self.may_publish = None;
            self.basis = format!(
                "{}; this entry has a run_on_host or call_webhook step, which the relay \
                 additionally admits only for the channel's owner or admin — a role this \
                 command does not read",
                self.basis
            );
        }
        self
    }
}

/// The delegable capabilities named in the wire's own tokens.
///
/// Built from [`ProjectActionCapability::is_delegable`] rather than written out,
/// so a remedy can never promise the one capability the grant deliberately
/// withholds (approving a host step).
fn delegable_capabilities() -> String {
    [
        ProjectActionCapability::PublishDefinition,
        ProjectActionCapability::TriggerManualRun,
        ProjectActionCapability::ApproveHostStep,
    ]
    .into_iter()
    .filter(|capability| capability.is_delegable())
    .map(ProjectActionCapability::as_str)
    .collect::<Vec<_>>()
    .join(" and ")
}

/// What would give `grantee_pubkey` standing over `project_ref`'s actions.
///
/// One sentence, naming the exact link a project owner signs. It is also the
/// `remedy` on a refused `bee actions publish` entry, so the two surfaces
/// cannot drift into describing different fixes.
pub fn project_action_grant_remedy(project_ref: &str, grantee_pubkey: &str) -> String {
    format!(
        "a project owner grants this key the capability by signing a \
         `grant-project-actions` link naming {project_ref} for {grantee_pubkey} in the team \
         session's kind:44228 authority chain (it confers {}, never approve-host-step, and is \
         revocable by `revoke-project-actions`); Beekeeper's founding form signs it for the \
         session's lead at Start",
        delegable_capabilities()
    )
}

/// The owner component of a `30621:<owner-hex>:<d>` coordinate, when it is one.
///
/// `None` for anything that is not a 64-hex pubkey: an owner comparison against
/// a malformed coordinate would answer "not the creator" for the creator.
fn coordinate_owner(project_ref: &str) -> Option<String> {
    let owner = project_ref.split(':').nth(1)?;
    if owner.len() == 64 && owner.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(owner.to_ascii_lowercase())
    } else {
        None
    }
}

/// First eight hex characters of a key, for prose that names a signer.
fn short_key(pubkey: &str) -> String {
    pubkey.chars().take(8).collect()
}

/// Decide, from inputs already read, whether the caller may publish and
/// trigger this project's actions.
///
/// The rules run in the relay's own order of specificity — creator, then a
/// live delegation, then a roster Owner row — and every branch that cannot see
/// far enough answers `None` with the read it lacked. Both flags move together
/// because every fact here confers both capabilities; they stay separate fields
/// because the relay's two admission paths are separate code and a future rule
/// (a trigger-only role, say) must be able to split them.
pub fn decide_project_action_authority(
    inputs: &ProjectActionAuthorityInputs,
) -> ProjectActionAuthority {
    let caller = inputs.caller_pubkey.to_ascii_lowercase();
    let Some(owner) = coordinate_owner(&inputs.project_ref) else {
        return unknown(format!(
            "the --project coordinate {:?} does not name a 64-hex owner, so no standing could be \
             checked against it",
            inputs.project_ref
        ));
    };

    // 1. The creator. Read from the coordinate itself, so this is the one
    // answer that needs no relay at all.
    if caller == owner {
        return allowed(format!(
            "the caller is the project's creator (the owner component of {})",
            inputs.project_ref
        ));
    }

    // 2. A live project-actions delegation in the named channel's chains.
    let (grants, discarded_chains, unread_links) = match &inputs.chain {
        AuthorityChainRead::NoChannel => {
            return unknown(format!(
                "no --channel was given, so the kind:44228 authority chain that could carry a \
                 project-actions grant for {} was not read",
                short_key(&caller)
            ));
        }
        AuthorityChainRead::Failed(failure) => {
            return unknown(format!(
                "the channel's kind:{KIND_CODING_SESSION_AUTHORITY_TRANSITION} authority chains \
                 could not be read: {failure}"
            ));
        }
        AuthorityChainRead::Chains {
            grants,
            discarded_chains,
            unread_links,
        } => (grants, *discarded_chains, *unread_links),
    };

    if let Some(grant) = find_project_action_grant(grants, &caller, &inputs.project_ref) {
        return decide_from_grant(grant, &owner, &inputs.roster, &inputs.project_ref);
    }

    // 3. An Owner row on the relay's roster projection. The projection's
    // signer is not verified against the relay's trusted self here, so the
    // basis says whose word this is.
    match &inputs.roster {
        ProjectRosterRead::Owners(owners) if owners.contains(&caller) => allowed(format!(
            "an Owner row for {} on the relay's kind:{KIND_PROJECT_MEMBERS} roster projection \
                 of {} (this is the relay's projection; its signer is not verified here)",
            short_key(&caller),
            inputs.project_ref
        )),
        ProjectRosterRead::Owners(_) => {
            // Nothing found. A skipped chain is the one thing that could have
            // hidden a grant, so it downgrades the refusal to a non-answer
            // rather than being swallowed.
            if discarded_chains > 0 || unread_links > 0 {
                return unknown(format!(
                    "no grant and no Owner row was found, but {discarded_chains} authority \
                     chain(s) in this channel are not contiguous in what this key can read and \
                     {unread_links} link(s) did not decode, so a grant they may carry is not \
                     disclosed"
                ));
            }
            ProjectActionAuthority {
                may_publish: Some(false),
                may_trigger: Some(false),
                basis: format!(
                    "nothing found: {} is not the creator of {}, holds no live \
                     project-actions grant in this channel's authority chains, and has no Owner \
                     row on the relay's kind:{KIND_PROJECT_MEMBERS} roster projection (an \
                     endorsed repository's founder standing and the channel owner/admin role are \
                     not read here)",
                    short_key(&caller),
                    inputs.project_ref
                ),
                remedy: Some(project_action_grant_remedy(&inputs.project_ref, &caller)),
                rests_on_delegation: false,
            }
        }
        ProjectRosterRead::NoProjection => unknown(format!(
            "no live project-actions grant was found and no kind:{KIND_PROJECT_MEMBERS} roster \
             projection exists for {}, so Owner rows could not be read",
            inputs.project_ref
        )),
        ProjectRosterRead::Failed(failure) => unknown(format!(
            "no live project-actions grant was found and the kind:{KIND_PROJECT_MEMBERS} roster \
             projection of {} could not be read: {failure}",
            inputs.project_ref
        )),
    }
}

/// A grant was found; decide whether its *signer* still owns the project.
///
/// The grant is only as good as the granter's standing today
/// ([`ProjectActionGrant::granted_by`]): an owner who signed a delegation and
/// then lost ownership must not leave a capability behind. The creator is
/// checked from the coordinate, so the common case (the owner signed it) needs
/// no roster at all; anything else needs the roster and says so when it cannot
/// have it.
fn decide_from_grant(
    grant: &ProjectActionGrant,
    owner: &str,
    roster: &ProjectRosterRead,
    project_ref: &str,
) -> ProjectActionAuthority {
    let granter = grant.granted_by.to_ascii_lowercase();
    if granter == owner {
        return allowed_by_delegation(format!(
            "a live project-actions grant {} for {project_ref}, signed by the project's creator",
            grant.grant_event_id
        ));
    }
    match roster {
        ProjectRosterRead::Owners(owners) if owners.contains(&granter) => {
            allowed_by_delegation(format!(
                "a live project-actions grant {} for {project_ref}, signed by {}, who holds an \
                 Owner row on the relay's kind:{KIND_PROJECT_MEMBERS} roster projection",
                grant.grant_event_id,
                short_key(&granter)
            ))
        }
        ProjectRosterRead::Owners(_) => unknown(format!(
            "project-actions grant {} names signer {}, who is neither the creator of \
             {project_ref} nor an Owner on its roster projection, so the grant confers nothing \
             the relay would honour",
            grant.grant_event_id,
            short_key(&granter)
        )),
        ProjectRosterRead::NoProjection => unknown(format!(
            "project-actions grant {} was signed by {}, and no kind:{KIND_PROJECT_MEMBERS} roster \
             projection exists for {project_ref}, so that signer's current ownership could not be \
             confirmed",
            grant.grant_event_id,
            short_key(&granter)
        )),
        ProjectRosterRead::Failed(failure) => unknown(format!(
            "project-actions grant {} was signed by {}, and the roster projection that would \
             confirm that signer's current ownership of {project_ref} could not be read: {failure}",
            grant.grant_event_id,
            short_key(&granter)
        )),
    }
}

/// Both capabilities predicted admitted, with the fact that decided it.
fn allowed(basis: String) -> ProjectActionAuthority {
    ProjectActionAuthority {
        may_publish: Some(true),
        may_trigger: Some(true),
        basis,
        remedy: None,
        rests_on_delegation: false,
    }
}

/// Admitted by a live delegation rather than by the caller's own standing.
///
/// Recorded separately because the host-step check accepts the same
/// delegation, so this answer must not be narrowed by
/// [`ProjectActionAuthority::narrowed_by_channel_elevation`].
fn allowed_by_delegation(basis: String) -> ProjectActionAuthority {
    ProjectActionAuthority {
        rests_on_delegation: true,
        ..allowed(basis)
    }
}

/// Neither capability answered, with the read that was missing.
///
/// No remedy: what would make it true is unknown precisely because the fact
/// that decides it was not read, and offering the grant here would suggest the
/// caller lacks standing when nobody checked.
fn unknown(basis: String) -> ProjectActionAuthority {
    ProjectActionAuthority {
        may_publish: None,
        may_trigger: None,
        basis,
        remedy: None,
        rests_on_delegation: false,
    }
}

/// Read what [`decide_project_action_authority`] needs, disclosing every gap.
///
/// Never returns an error: a status report whose authority read failed must
/// still print the entries, with the failure named in the basis. `channel` is
/// `None` when `bee actions status` was given no `--channel`.
pub async fn read_project_action_authority(
    client: &BuzzClient,
    project_ref: &str,
    channel: Option<&str>,
) -> ProjectActionAuthorityInputs {
    let caller_pubkey = client.keys().public_key().to_hex().to_ascii_lowercase();
    let chain = match channel {
        Some(channel) => read_authority_chains(client, channel).await,
        None => AuthorityChainRead::NoChannel,
    };
    let roster = read_project_roster(client, project_ref).await;
    ProjectActionAuthorityInputs {
        caller_pubkey,
        project_ref: project_ref.to_owned(),
        chain,
        roster,
    }
}

/// One readable link of a chain: `(seq, event id, signer, payload)`.
type ReadLink = (u32, String, String, CodingSessionAuthorityTransitionPayload);

/// Fold every contiguous kind:44228 chain under one channel's `h` tag.
async fn read_authority_chains(client: &BuzzClient, channel: &str) -> AuthorityChainRead {
    let rows = match client
        .query_paginated(
            serde_json::json!({
                "kinds": [KIND_CODING_SESSION_AUTHORITY_TRANSITION],
                "#h": [channel],
            }),
            MAX_CHANNEL_AUTHORITY_TRANSITIONS,
        )
        .await
    {
        Ok(rows) => rows,
        Err(error) => return AuthorityChainRead::Failed(error.to_string()),
    };

    let mut chains: BTreeMap<String, Vec<ReadLink>> = BTreeMap::new();
    let mut unread_links = 0usize;
    for row in rows {
        let event_id = row.get("id").and_then(serde_json::Value::as_str);
        let signer = row.get("pubkey").and_then(serde_json::Value::as_str);
        let content = row.get("content").and_then(serde_json::Value::as_str);
        let (Some(event_id), Some(signer), Some(content)) = (event_id, signer, content) else {
            unread_links += 1;
            continue;
        };
        let Ok(payload) = decode_coding_session_authority_transition(content) else {
            unread_links += 1;
            continue;
        };
        chains
            .entry(payload.genesis_ref.clone())
            .or_default()
            .push((
                payload.seq,
                event_id.to_ascii_lowercase(),
                signer.to_ascii_lowercase(),
                payload,
            ));
    }

    let mut grants = Vec::new();
    let mut discarded_chains = 0usize;
    for links in chains.into_values() {
        match contiguous_chain_links(links) {
            Some(links) => grants.extend(fold_project_action_grants(links)),
            None => discarded_chains += 1,
        }
    }
    AuthorityChainRead::Chains {
        grants,
        discarded_chains,
        unread_links,
    }
}

/// The chain's links in accepted order, or `None` when what is readable is not
/// the whole chain.
///
/// Contiguity is `seq` 1..n with each link's `prevAccepted` naming the previous
/// link's event id. Anything else — a gap, a duplicate `seq`, a fork, or a
/// window that starts mid-chain because the earlier links are not readable by
/// this key — means the fold could be missing the revocation that ends a
/// grant, so the chain contributes nothing and the caller is told it was
/// skipped.
fn contiguous_chain_links(mut links: Vec<ReadLink>) -> Option<Vec<ProjectActionGrantLink>> {
    links.sort_by_key(|(seq, _, _, _)| *seq);
    let mut ordered = Vec::with_capacity(links.len());
    let mut previous: Option<(u32, String)> = None;
    for (seq, event_id, signer, payload) in links {
        match &previous {
            None if seq != 1 || payload.prev_accepted.is_some() => return None,
            Some((previous_seq, previous_id))
                if seq != previous_seq + 1
                    || payload.prev_accepted.as_deref() != Some(previous_id) =>
            {
                return None
            }
            None | Some(_) => {}
        }
        previous = Some((seq, event_id.clone()));
        ordered.push(ProjectActionGrantLink {
            seq,
            accepted_event_id: event_id,
            transition_type: payload.transition_type,
            signer_pubkey: signer,
            grantee_pubkey: payload.grantee_pubkey.to_ascii_lowercase(),
            project_ref: payload.project_ref,
        });
    }
    Some(ordered)
}

/// The **Owner** rows of the project's latest relay-signed kind:39010
/// projection.
///
/// Role lives at index 3 of each `p` tag, and an unrecognized role reads as a
/// collaborator exactly as `bee projects members` reads it — this must not
/// disagree with what a person is shown.
async fn read_project_roster(client: &BuzzClient, project_ref: &str) -> ProjectRosterRead {
    let raw = match client
        .query(&serde_json::json!({
            "kinds": [KIND_PROJECT_MEMBERS],
            "#d": [project_ref],
            "limit": 1,
        }))
        .await
    {
        Ok(raw) => raw,
        Err(error) => return ProjectRosterRead::Failed(error.to_string()),
    };
    let mut projections: Vec<serde_json::Value> = match serde_json::from_str(&raw) {
        Ok(projections) => projections,
        Err(error) => {
            return ProjectRosterRead::Failed(format!("relay response is not valid JSON: {error}"))
        }
    };
    projections.sort_by_key(|event| {
        std::cmp::Reverse(event.get("created_at").and_then(serde_json::Value::as_i64))
    });
    let Some(projection) = projections.first() else {
        return ProjectRosterRead::NoProjection;
    };
    ProjectRosterRead::Owners(owner_rows(projection))
}

/// Lowercase pubkeys of every `p` tag on a kind:39010 projection whose role is
/// `owner`.
fn owner_rows(projection: &serde_json::Value) -> Vec<String> {
    projection
        .get("tags")
        .and_then(serde_json::Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(|tag| {
                    let parts = tag.as_array()?;
                    if parts.first()?.as_str()? != "p" {
                        return None;
                    }
                    let pubkey = parts.get(1)?.as_str()?.to_ascii_lowercase();
                    let role = parts
                        .get(3)
                        .and_then(serde_json::Value::as_str)
                        .filter(|role| is_valid_project_role(role))
                        .unwrap_or(beekeeper_core::kind::PROJECT_ROLE_COLLABORATOR);
                    (role == PROJECT_ROLE_OWNER).then_some(pubkey)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_core::coding_session_authority_transition::CodingSessionAuthorityTransitionType;

    const PROJECT: &str =
        "30621:1111111111111111111111111111111111111111111111111111111111111111:kettle";

    fn key(byte: &str) -> String {
        byte.repeat(32)
    }

    fn owner() -> String {
        key("11")
    }

    fn lead() -> String {
        key("22")
    }

    fn grant_id() -> String {
        key("ab")
    }

    /// Inputs for a caller with no standing and both reads answering cleanly.
    fn nothing_found(caller: &str) -> ProjectActionAuthorityInputs {
        ProjectActionAuthorityInputs {
            caller_pubkey: caller.to_owned(),
            project_ref: PROJECT.to_owned(),
            chain: AuthorityChainRead::Chains {
                grants: Vec::new(),
                discarded_chains: 0,
                unread_links: 0,
            },
            roster: ProjectRosterRead::Owners(Vec::new()),
        }
    }

    fn live_grant(granter: &str, grantee: &str) -> ProjectActionGrant {
        ProjectActionGrant {
            grantee_pubkey: grantee.to_owned(),
            project_ref: PROJECT.to_owned(),
            granted_by: granter.to_owned(),
            grant_event_id: grant_id(),
        }
    }

    #[test]
    fn a_live_grant_from_the_creator_admits_both_capabilities() {
        let mut inputs = nothing_found(&lead());
        inputs.chain = AuthorityChainRead::Chains {
            grants: vec![live_grant(&owner(), &lead())],
            discarded_chains: 0,
            unread_links: 0,
        };
        let decision = decide_project_action_authority(&inputs);
        assert_eq!(decision.may_publish, Some(true));
        assert_eq!(decision.may_trigger, Some(true));
        assert!(
            decision.basis.contains(&grant_id()),
            "the basis names the grant event id: {}",
            decision.basis
        );
        assert_eq!(decision.remedy, None);
    }

    #[test]
    fn nothing_found_refuses_and_names_the_grant_as_the_remedy() {
        let decision = decide_project_action_authority(&nothing_found(&lead()));
        assert_eq!(decision.may_publish, Some(false));
        assert_eq!(decision.may_trigger, Some(false));
        let remedy = decision.remedy.expect("a refusal offers the delegation");
        assert!(remedy.contains("grant-project-actions"), "{remedy}");
        assert!(remedy.contains(PROJECT), "{remedy}");
        assert!(remedy.contains(&lead()), "the grantee is named: {remedy}");
        // The remedy must never promise the capability the grant withholds.
        assert!(!remedy.contains("confers approve-host-step"), "{remedy}");
        assert!(
            decision.basis.contains("not read here"),
            "the refusal discloses what it did not read: {}",
            decision.basis
        );
    }

    #[test]
    fn a_failed_chain_read_answers_null_not_false() {
        let mut inputs = nothing_found(&lead());
        inputs.chain = AuthorityChainRead::Failed("relay: 500 internal".to_owned());
        let decision = decide_project_action_authority(&inputs);
        assert_eq!(decision.may_publish, None);
        assert_eq!(decision.may_trigger, None);
        assert!(
            decision.basis.contains("relay: 500 internal"),
            "{}",
            decision.basis
        );
        assert_eq!(decision.remedy, None);
    }

    #[test]
    fn a_failed_roster_read_answers_null_not_false() {
        let mut inputs = nothing_found(&lead());
        inputs.roster = ProjectRosterRead::Failed("relay: timed out".to_owned());
        let decision = decide_project_action_authority(&inputs);
        assert_eq!(decision.may_publish, None);
        assert_eq!(decision.may_trigger, None);
        assert!(decision.basis.contains("timed out"), "{}", decision.basis);
    }

    #[test]
    fn the_creator_needs_no_relay_read_at_all() {
        let mut inputs = nothing_found(&owner());
        inputs.chain = AuthorityChainRead::Failed("unreachable".to_owned());
        inputs.roster = ProjectRosterRead::Failed("unreachable".to_owned());
        let decision = decide_project_action_authority(&inputs);
        assert_eq!(decision.may_publish, Some(true));
        assert!(decision.basis.contains("creator"), "{}", decision.basis);
    }

    #[test]
    fn a_roster_owner_row_admits_and_says_whose_word_it_is() {
        let mut inputs = nothing_found(&lead());
        inputs.roster = ProjectRosterRead::Owners(vec![lead()]);
        let decision = decide_project_action_authority(&inputs);
        assert_eq!(decision.may_publish, Some(true));
        assert!(
            decision.basis.contains("relay's projection"),
            "{}",
            decision.basis
        );
    }

    #[test]
    fn no_channel_is_a_non_answer_with_the_missing_read_named() {
        let mut inputs = nothing_found(&lead());
        inputs.chain = AuthorityChainRead::NoChannel;
        let decision = decide_project_action_authority(&inputs);
        assert_eq!(decision.may_publish, None);
        assert!(decision.basis.contains("--channel"), "{}", decision.basis);
    }

    #[test]
    fn a_skipped_chain_downgrades_a_refusal_to_a_non_answer() {
        let mut inputs = nothing_found(&lead());
        inputs.chain = AuthorityChainRead::Chains {
            grants: Vec::new(),
            discarded_chains: 1,
            unread_links: 0,
        };
        let decision = decide_project_action_authority(&inputs);
        assert_eq!(decision.may_publish, None);
        assert!(
            decision.basis.contains("not contiguous"),
            "{}",
            decision.basis
        );
    }

    #[test]
    fn a_grant_from_a_signer_who_is_not_an_owner_confers_nothing() {
        let mut inputs = nothing_found(&lead());
        inputs.chain = AuthorityChainRead::Chains {
            grants: vec![live_grant(&key("99"), &lead())],
            discarded_chains: 0,
            unread_links: 0,
        };
        let decision = decide_project_action_authority(&inputs);
        assert_eq!(decision.may_publish, None);
        assert!(
            decision.basis.contains("neither the creator"),
            "{}",
            decision.basis
        );
    }

    /// The relay's host-step check accepts the same delegation (ledger 186),
    /// so a delegated answer is whole and must not be narrowed into a doubt
    /// the relay does not have.
    #[test]
    fn a_delegated_publish_survives_the_host_step_narrowing() {
        let mut inputs = nothing_found(&lead());
        inputs.chain = AuthorityChainRead::Chains {
            grants: vec![live_grant(&owner(), &lead())],
            discarded_chains: 0,
            unread_links: 0,
        };
        let decision = decide_project_action_authority(&inputs).narrowed_by_channel_elevation(true);
        assert_eq!(decision.may_publish, Some(true));
        assert_eq!(decision.may_trigger, Some(true));
        assert!(decision.rests_on_delegation);
    }

    #[test]
    fn a_host_step_withdraws_a_publish_promise_but_not_a_refusal() {
        let admitted = decide_project_action_authority(&nothing_found(&owner()))
            .narrowed_by_channel_elevation(true);
        assert_eq!(admitted.may_publish, None, "channel role is not read");
        assert_eq!(admitted.may_trigger, Some(true), "triggering is unaffected");
        assert!(admitted.basis.contains("run_on_host"), "{}", admitted.basis);

        let refused = decide_project_action_authority(&nothing_found(&lead()))
            .narrowed_by_channel_elevation(true);
        assert_eq!(
            refused.may_publish,
            Some(false),
            "project standing is necessary, so its absence still settles the refusal"
        );
    }

    #[test]
    fn a_malformed_coordinate_is_disclosed_not_guessed() {
        let mut inputs = nothing_found(&lead());
        inputs.project_ref = "30621:not-hex:kettle".to_owned();
        let decision = decide_project_action_authority(&inputs);
        assert_eq!(decision.may_publish, None);
        assert!(
            decision.basis.contains("64-hex owner"),
            "{}",
            decision.basis
        );
    }

    #[test]
    fn a_chain_missing_its_first_link_is_skipped_rather_than_folded() {
        // seq 2 alone: the grant could already have been revoked by a link
        // this key cannot read, so folding it would activate a stale link.
        let link = |seq: u32, prev: Option<&str>| -> ReadLink {
            (
                seq,
                key("0a"),
                owner(),
                CodingSessionAuthorityTransitionPayload {
                    genesis_ref: key("cd"),
                    prev_accepted: prev.map(str::to_owned),
                    seq,
                    transition_type: CodingSessionAuthorityTransitionType::GrantProjectActions,
                    grantee_pubkey: lead(),
                    role: None,
                    body_pubkey: None,
                    project_ref: Some(PROJECT.to_owned()),
                },
            )
        };
        assert!(contiguous_chain_links(vec![link(2, Some(&key("0b")))]).is_none());
        let first = contiguous_chain_links(vec![link(1, None)]).expect("a lone first link folds");
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].project_ref.as_deref(), Some(PROJECT));
    }

    #[test]
    fn owner_rows_read_the_role_at_index_three() {
        let projection = serde_json::json!({
            "tags": [
                ["p", key("11"), "", "owner"],
                ["p", key("22"), "", "collaborator"],
                ["p", key("33"), "", "not-a-role"],
                ["p", key("44")],
                ["d", PROJECT],
            ],
        });
        assert_eq!(owner_rows(&projection), vec![key("11")]);
    }
}
