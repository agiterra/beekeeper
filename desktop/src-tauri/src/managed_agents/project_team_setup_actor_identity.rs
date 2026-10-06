//! Owner-encrypted mint recovery; persisted before keyring or instance-store writes.

use super::{invalid, pack, PreparedSetupActor, ProjectTeamSetupDraft, SetupError};
use crate::managed_agents::packs_cache::PackRef;
use crate::managed_agents::types::{AgentDefinition, ManagedAgentRecord};
use nostr::{nips::nip44, Keys};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Receipt {
    schema: String,
    setup_id: String,
    project_ref: String,
    owner_pubkey: String,
    relay_url: String,
    pub(super) directory_id: String,
    pub(super) pack_ref: PackRef,
    pub(super) pack_digest: String,
    pub(super) definition: AgentDefinition,
    pub(super) pubkey: String,
    nsec: String,
    auth_tag: String,
}

pub(super) fn definition_id(draft: &ProjectTeamSetupDraft) -> String {
    format!("project-team-setup:{}", draft.setup_id)
}

pub(super) fn mint(
    draft: &ProjectTeamSetupDraft,
    owner: &Keys,
    directory_id: String,
    pack_ref: PackRef,
    pack_digest: String,
    persona: beekeeper_persona_pkg::resolve::ResolvedPersona,
) -> Result<Receipt, SetupError> {
    let (_, identity) = crate::commands::mint_agent_identity(owner).map_err(invalid)?;
    let now = crate::util::now_iso();
    Ok(Receipt {
        schema: "project-team-setup-actor/v1".into(),
        setup_id: draft.setup_id.clone(),
        project_ref: draft.project_ref.clone(),
        owner_pubkey: draft.owner_pubkey.clone(),
        relay_url: draft.relay_url.clone(),
        directory_id,
        pack_ref,
        pack_digest,
        definition: AgentDefinition {
            id: definition_id(draft),
            display_name: "Project setup".into(),
            avatar_url: persona.avatar,
            system_prompt: persona.system_prompt,
            runtime: None,
            model: None,
            provider: None,
            name_pool: Vec::new(),
            is_builtin: false,
            is_active: true,
            shared: false,
            source_team: None,
            source_team_persona_slug: None,
            catalog_source: None,
            env_vars: Default::default(),
            respond_to: None,
            respond_to_allowlist: Vec::new(),
            parallelism: None,
            created_at: now.clone(),
            updated_at: now,
        },
        pubkey: identity.pubkey,
        nsec: identity.private_key_nsec,
        auth_tag: identity
            .auth_tag
            .ok_or_else(|| invalid("The setup identity needs an owner attestation."))?,
    })
}

pub(super) fn persist(path: &Path, receipt: &Receipt, owner: &Keys) -> Result<(), SetupError> {
    let plaintext = serde_json::to_vec(receipt).map_err(|e| invalid(e.to_string()))?;
    let ciphertext = nip44::encrypt(
        owner.secret_key(),
        &owner.public_key(),
        plaintext,
        nip44::Version::V2,
    )
    .map_err(|e| invalid(e.to_string()))?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    pack::write_new(&temporary, ciphertext.as_bytes())?;
    std::fs::rename(temporary, path)?;
    pack::sync(
        path.parent()
            .ok_or_else(|| invalid("Actor receipt has no parent."))?,
    )
}

pub(super) fn read(
    path: &Path,
    draft: &ProjectTeamSetupDraft,
    owner: &Keys,
) -> Result<Option<Receipt>, SetupError> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    let ciphertext = pack::read(path, 128 * 1024)?;
    let plaintext =
        nip44::decrypt(owner.secret_key(), &owner.public_key(), ciphertext).map_err(|_| {
            invalid("The setup identity receipt could not be authenticated by this owner.")
        })?;
    let receipt: Receipt = serde_json::from_str(&plaintext)
        .map_err(|_| invalid("The setup identity receipt is malformed."))?;
    receipt.validate(draft, owner)?;
    Ok(Some(receipt))
}

impl Receipt {
    pub(super) fn validate(
        &self,
        draft: &ProjectTeamSetupDraft,
        owner: &Keys,
    ) -> Result<(), SetupError> {
        if self.schema != "project-team-setup-actor/v1"
            || self.setup_id != draft.setup_id
            || self.project_ref != draft.project_ref
            || self.relay_url != draft.relay_url
            || self.owner_pubkey != draft.owner_pubkey
            || owner.public_key().to_hex() != self.owner_pubkey
            || self.definition.id != definition_id(draft)
            || uuid::Uuid::parse_str(&self.directory_id).is_err()
        {
            return Err(invalid(
                "The setup identity receipt belongs to another setup, project, owner or community.",
            ));
        }
        let agent = Keys::parse(&self.nsec)
            .map_err(|_| invalid("The preserved setup identity key is malformed."))?;
        if agent.public_key().to_hex() != self.pubkey {
            return Err(invalid(
                "The preserved setup identity does not match its key.",
            ));
        }
        let attested =
            beekeeper_sdk_pkg::nip_oa::verify_auth_tag(&self.auth_tag, &agent.public_key())
                .map_err(|_| invalid("The setup identity owner attestation is invalid."))?;
        if attested != owner.public_key() {
            return Err(invalid("The setup identity has a different owner."));
        }
        if self.pack_ref.repo != crate::managed_agents::packs_cache::PACK_REF_SHIPPED_REPO
            || self.pack_ref.role != pack::ROLE
            || self.pack_ref.path != "personas/roles/project-setup"
            || self.pack_ref.sha.is_empty()
        {
            return Err(invalid(
                "The setup bootstrap has no shipped-pack provenance.",
            ));
        }
        Ok(())
    }

    pub(super) fn prepared(&self, storage: &Path) -> PreparedSetupActor {
        PreparedSetupActor {
            actor_pubkey: self.pubkey.clone(),
            pack_ref: self.pack_ref.clone(),
            pack_digest: self.pack_digest.clone(),
            authoring_directory: storage.join("draft").to_string_lossy().into_owned(),
        }
    }

    pub(super) fn record(&self, pack_dir: &Path) -> ManagedAgentRecord {
        let mut record = self.definition.clone().into_agent_record();
        record.pubkey = self.pubkey.clone();
        record.private_key_nsec = self.nsec.clone();
        record.auth_tag = Some(self.auth_tag.clone());
        record.persona_id = Some(self.definition.id.clone());
        record.slug = None;
        record.display_name = None;
        record.relay_url = self.relay_url.clone();
        record.home_role = Some(pack::ROLE.into());
        record.persona_team_dir = Some(pack_dir.to_path_buf());
        record.persona_name_in_team = Some(pack::ROLE.into());
        record.auto_restart_on_config_change = false;
        record
    }

    pub(super) fn reconcile(
        &self,
        records: &mut Vec<ManagedAgentRecord>,
        pack_dir: &Path,
    ) -> Result<(), SetupError> {
        let matches: Vec<_> = records
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                record.pubkey == self.pubkey
                    || record.persona_id.as_deref() == Some(&self.definition.id)
            })
            .map(|(index, _)| index)
            .collect();
        if matches.len() > 1 {
            return Err(invalid("Multiple identities claim this setup assignment."));
        }
        let expected = self.record(pack_dir);
        if let Some(&index) = matches.first() {
            let record = &mut records[index];
            if record.pubkey != self.pubkey
                || record.persona_id != expected.persona_id
                || record.relay_url != self.relay_url
                || record.auth_tag != expected.auth_tag
                || record.team_id.is_some()
                || record.source_team.is_some()
                || record.start_on_app_launch
                || !record.is_active
                || record.home_role != expected.home_role
                || record.persona_team_dir != expected.persona_team_dir
                || record.persona_name_in_team != expected.persona_name_in_team
            {
                return Err(invalid(
                    "The preserved setup identity was replaced or assigned outside this setup.",
                ));
            }
            if !record.private_key_nsec.is_empty() && record.private_key_nsec != self.nsec {
                return Err(invalid(
                    "The setup identity's stored key differs from its recovery receipt.",
                ));
            }
            // Recovery reinstates only the exact owner-encrypted key, never a newly minted key.
            record.private_key_nsec = self.nsec.clone();
        } else {
            records.push(expected);
        }
        Ok(())
    }
}
