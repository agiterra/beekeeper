//! Explicit owner-side local hire adapter. It never launches a runtime itself.
use crate::hire::{Adapter, Scope, SeatPlan};
use crate::hire_source::{git, host, projects, source, RoleMaterial};
use crate::management::local::{fail, owner};
use crate::{Context, Error, RelayTransport};
use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction as Action,
    CodingSessionLifecycleCommandPayload, CODING_SESSION_HIRE_BRIEF_PREFIX,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use beekeeper_core::managed_agent::ManagedAgentEventContent;
use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

/// Same standing policy fields and defaults as Desktop's hire policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Policy {
    /// Master switch.
    pub enabled: bool,
    /// Null permits every installed role in the explicitly selected scope.
    pub allowed_roles: Option<Vec<String>>,
    /// Active and pending seats, bounded to Desktop's 1..32 range.
    pub max_seats_per_umbrella: usize,
    /// Null permits every commissioned runtime in the selected scope.
    pub allowed_provider_instance_refs: Option<Vec<String>>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_roles: None,
            max_seats_per_umbrella: 10,
            allowed_provider_instance_refs: None,
        }
    }
}

/// One explicitly selected canonical identity/runtime/model for isolated cutover.
pub struct LocalHost<'a, T> {
    /// Management home; never the operating-system home by inference.
    pub context: &'a Context,
    /// Direct owner-authenticated transport retained only by management.
    pub client: &'a T,
    /// Canonically installed role identity.
    pub actor: String,
    /// Explicit commissioned runtime alias.
    pub provider_instance: String,
    /// Explicit selected model.
    pub model: String,
    /// Persisted operator policy with Desktop's fields/defaults.
    pub policy: Policy,
}

#[path = "hire_host_read.rs"]
mod read;
#[path = "hire_host_stage.rs"]
mod staging;

impl<T: RelayTransport> LocalHost<'_, T> {
    async fn material(
        &self,
        scope: &Scope,
    ) -> Result<(crate::PrivateConfigurationSnapshot, RoleMaterial), Error> {
        let snapshot =
            crate::private_configuration_snapshot(self.context, self.client, &self.actor)?;
        if snapshot.definition != snapshot.enrollment_definition
            || crate::management::configuration::current_head(self.client, &self.actor).await?
                != snapshot.definition
        {
            return Err(Error::Refused(
                "HIRE_STALE: canonical identity definition changed".into(),
            ));
        }
        let material: RoleMaterial = serde_json::from_value(
            snapshot.private.record["role_material"].clone(),
        )
        .map_err(|_| {
            Error::Refused("identity was not installed with canonical role material".into())
        })?;
        let definition: ManagedAgentEventContent =
            serde_json::from_str(&snapshot.definition.content).map_err(fail)?;
        let public_project =
            crate::authorize_project(self.client, &scope.project, &owner(self.client)?).await?;
        if material.project != scope.project
            || definition.home_role.as_deref() != Some(&material.pack_ref.role)
            || (public_project
                && (definition.project_withdrawn
                    || definition.project_digest
                        != beekeeper_core::project_agent_association::project_agent_digest(
                            &scope.project,
                        )))
            || source(self.client, &scope.project).await? != material.source
            || snapshot.private.record["provider_instance"] != self.provider_instance
            || snapshot.private.record["model"] != self.model
            || definition.model.as_deref() != Some(&self.model)
        {
            return Err(Error::Refused(
                "HIRE_STALE: project association, role source or runtime/model changed".into(),
            ));
        }
        let config = host(self.context, self.client)?;
        let trusted = projects(&config)?;
        let trusted = trusted
            .agents_repos
            .get(&scope.project)
            .ok_or_else(|| Error::Refused("trusted role source removed".into()))?;
        if crate::hire_source::current_tip(self.context, self.client, trusted, &material.source)
            .await?
            != material.pack_ref.sha
        {
            return Err(Error::Refused(
                "HIRE_STALE: role source ref moved from installed commit".into(),
            ));
        }
        if !crate::project_roster(self.client, &scope.project)
            .await?
            .iter()
            .any(|(key, role)| {
                key == &self.actor && matches!(role.as_str(), "owner" | "collaborator")
            })
        {
            return Err(Error::Refused(
                "HIRE_NO_PROJECT_AGENT: identity is no longer admitted to project".into(),
            ));
        }
        let persona = beekeeper_persona::resolve::resolve_persona_by_name(
            &material.pack_dir,
            &material.pack_ref.role,
        )
        .map_err(fail)?;
        let provenance = beekeeper_persona::compose::read_provenance(&material.pack_dir)
            .map_err(fail)?
            .ok_or_else(|| fail("role composition provenance missing"))?;
        if persona.role.as_deref() != Some(&material.pack_ref.role)
            || provenance.digest != material.digest
            || crate::hire_pack_integrity::digest(&material.pack_dir)? != material.content_digest
        {
            return Err(Error::Refused(
                "HIRE_STALE: composed role provenance changed".into(),
            ));
        }
        Ok((snapshot, material))
    }
}

impl<T: RelayTransport> Adapter for LocalHost<'_, T> {
    async fn authorize(&self, scope: &Scope, request: &Event) -> Result<(), Error> {
        host(self.context, self.client)?;
        let payload = decode_coding_session_lifecycle_command(&request.content).map_err(fail)?;
        if payload.hire_requester_matches_signer(&request.pubkey.to_hex()) == Some(false) {
            return Err(Error::Auth(
                "hire requester attribution differs from signed requester".into(),
            ));
        }
        let authority = self.authority(scope).await?;
        let signer = request.pubkey.to_hex();
        if signer != owner(self.client)?
            && !authority
                .grants
                .iter()
                .any(|g| g.actor_pubkey == signer && g.may_steer)
            && !authority.seats.iter().any(|s| {
                s.actor_pubkey == signer
                    && s.role
                        == beekeeper_core::role_vocabulary::RoleVocabulary::foundation().lead()
            })
        {
            return Err(Error::Auth(
                "requester has no accepted hire authority".into(),
            ));
        }
        self.verify_umbrella(scope).await
    }
    async fn refusal_target(
        &self,
        scope: &Scope,
        request: &Event,
    ) -> Result<CodingSessionTarget, Error> {
        self.requester_target(scope, request).await
    }
    async fn plan(
        &self,
        scope: &Scope,
        request: &Event,
        pending: &[SeatPlan],
    ) -> Result<SeatPlan, Error> {
        let payload = decode_coding_session_lifecycle_command(&request.content).map_err(fail)?;
        let Action::SessionHire {
            session_ref,
            genesis_ref,
            role,
            provider_instance_ref,
            model,
            brief,
            routing,
            ..
        } = payload.action
        else {
            return Err(Error::Usage("not a hire".into()));
        };
        if !self.policy.enabled {
            return Err(Error::Refused("HIRE_OFF".into()));
        }
        if !(1..=32).contains(&self.policy.max_seats_per_umbrella) {
            return Err(Error::Usage("hire seat ceiling must be 1..32".into()));
        }
        if self
            .policy
            .allowed_roles
            .as_ref()
            .is_some_and(|roles| !roles.contains(&role))
        {
            return Err(Error::Refused("HIRE_ROLE_NOT_ALLOWED".into()));
        }
        if routing.is_some() {
            return Err(Error::Refused(
                "HIRE_NO_ROUTE: this explicitly selected listener has no routing registry".into(),
            ));
        }
        if provider_instance_ref
            .as_ref()
            .is_some_and(|p| p.as_str() != self.provider_instance)
            || self
                .policy
                .allowed_provider_instance_refs
                .as_ref()
                .is_some_and(|p| !p.contains(&self.provider_instance))
        {
            return Err(Error::Refused("HIRE_PROVIDER_NOT_ALLOWED".into()));
        }
        if model.as_ref().is_some_and(|m| m != &self.model) {
            return Err(Error::Refused(
                "HIRE_MODEL_NOT_OFFERED: explicit request differs from selected identity model"
                    .into(),
            ));
        }
        let (snapshot, material) = self.material(scope).await?;
        if role != material.pack_ref.role {
            return Err(Error::Refused("HIRE_NO_PROJECT_AGENT".into()));
        }
        self.runtime(scope).await?;
        let active = self.active_seats(scope).await?;
        if active.iter().any(|actor| actor == &self.actor) {
            return Err(Error::Refused("HIRE_ROLE_BUSY".into()));
        }
        if active.len() + pending.len() >= self.policy.max_seats_per_umbrella {
            return Err(Error::Refused("HIRE_LIMIT".into()));
        }
        let config = host(self.context, self.client)?;
        let paths = projects(&config)?;
        let checkout = paths
            .projects
            .get(&scope.project)
            .ok_or_else(|| Error::Refused("HIRE_CHECKOUT_NOT_RECORDED".into()))?
            .canonicalize()
            .map_err(fail)?;
        crate::validate_workspace(
            &checkout,
            &self.context.home,
            &config.provider_state_dir,
            self.context.native_home.as_deref(),
        )?;
        let checkout_revision = git(self.context, &checkout, &["rev-parse", "HEAD"])?;
        if !git(self.context, &checkout, &["status", "--porcelain"])?.is_empty() {
            return Err(Error::Refused(
                "HIRE_STALE: trusted checkout is dirty".into(),
            ));
        }
        let command_id = Uuid::new_v4().to_string();
        let worktree =
            beekeeper_core::worktree_placement::default_worktree_parent(&checkout, false)
                .ok_or_else(|| fail("trusted checkout has no worktree parent"))?
                .path_for(&format!("{role}-{command_id}"));
        let agents_checkout = (material.agents_access != "none")
            .then(|| std::path::PathBuf::from(format!("{}-agents", worktree.display())));
        let brief = brief.trim();
        let initial_turn = if brief.starts_with(CODING_SESSION_HIRE_BRIEF_PREFIX) {
            brief.into()
        } else {
            format!("{CODING_SESSION_HIRE_BRIEF_PREFIX}{brief}")
        };
        let create = self.client.sign_event_unchecked(
            beekeeper_sdk::build_coding_session_lifecycle_command(
                scope.channel,
                &CodingSessionLifecycleCommandPayload {
                    schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
                    command_id,
                    action: Action::SessionCreate {
                        project_ref: Some(scope.project.clone()),
                        repo_ref: None,
                        session_ref: Some(session_ref),
                        genesis_ref: Some(genesis_ref),
                        provider_instance_ref: self
                            .provider_instance
                            .as_str()
                            .try_into()
                            .map_err(fail)?,
                        provider_authority_pubkey: config.provider_pubkey,
                        model: Some(self.model.clone()),
                        title: Some(role.clone()),
                        initial_turn: Some(initial_turn),
                        actor: Some(self.actor.clone()),
                        role: Some(role),
                        hire_ref: Some(request.id.to_hex()),
                        routing: None,
                    },
                },
            )
            .map_err(fail)?,
        )?;
        Ok(SeatPlan {
            create,
            definition_revision: snapshot.definition.id.to_hex(),
            private_snapshot_hash: snapshot.snapshot_hash,
            source_event_id: material.source.id.to_hex(),
            pack_ref: material.pack_ref,
            checkout,
            checkout_revision,
            worktree,
            pack_dir: material.pack_dir,
            agents_checkout,
        })
    }
    async fn revalidate(&self, scope: &Scope, plan: &SeatPlan) -> Result<(), Error> {
        if !self.policy.enabled
            || self
                .policy
                .allowed_roles
                .as_ref()
                .is_some_and(|roles| !roles.contains(&plan.pack_ref.role))
            || self
                .policy
                .allowed_provider_instance_refs
                .as_ref()
                .is_some_and(|refs| !refs.contains(&self.provider_instance))
        {
            return Err(Error::Refused(
                "HIRE_POLICY_CHANGED: saved selection is no longer allowed".into(),
            ));
        }
        let (snapshot, material) = self.material(scope).await?;
        self.runtime(scope).await?;
        if snapshot.definition.id.to_hex() != plan.definition_revision
            || snapshot.snapshot_hash != plan.private_snapshot_hash
            || material.pack_ref != plan.pack_ref
            || material.source.id.to_hex() != plan.source_event_id
            || material.pack_dir != plan.pack_dir
            || git(self.context, &plan.checkout, &["rev-parse", "HEAD"])? != plan.checkout_revision
            || !git(self.context, &plan.checkout, &["status", "--porcelain"])?.is_empty()
        {
            return Err(Error::Refused(
                "HIRE_STALE: saved selection changed; no replacement selected".into(),
            ));
        }
        let config = host(self.context, self.client)?;
        if projects(&config)?.projects.get(&scope.project) != Some(&plan.checkout) {
            return Err(Error::Refused("HIRE_CHECKOUT_NOT_RECORDED".into()));
        }
        Ok(())
    }
    async fn stage(&self, scope: &Scope, plan: &SeatPlan) -> Result<(), Error> {
        self.stage_seat(scope, plan).await
    }
    async fn grants(
        &self,
        scope: &Scope,
        plan: &SeatPlan,
        target: &CodingSessionTarget,
    ) -> Result<Vec<Event>, Error> {
        self.missing_grant(scope, plan, target).await
    }
    async fn grants_ready(
        &self,
        scope: &Scope,
        plan: &SeatPlan,
        target: &CodingSessionTarget,
    ) -> Result<bool, Error> {
        self.ready_grants(scope, plan, target).await
    }
    async fn grant_reconciled(&self, scope: &Scope, event: &Event) -> Result<bool, Error> {
        let grant=beekeeper_core::coding_session_authority_transition::decode_coding_session_authority_transition(&event.content).map_err(fail)?;
        // Once the accepted chain has reached this sequence, this exact event
        // is either accepted or permanently superseded. A lost ACK alone never
        // licenses replacement. The complete projection verifies every link.
        Ok(self.authority(scope).await?.head_seq >= grant.seq)
    }
}
