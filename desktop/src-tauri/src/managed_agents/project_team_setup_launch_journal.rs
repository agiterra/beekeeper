//! Host-sealed launch choice and exact signed create, outside the editable draft.
use super::*;
use nostr::secp256k1::{schnorr::Signature, Message};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

const LIMIT: u64 = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LaunchJournal {
    version: u32,
    setup_id: String,
    project_ref: String,
    owner_pubkey: String,
    relay_url: String,
    pub reservation: authoring::AuthoringReservation,
    pub choice: LaunchChoice,
    pub actor: actor::PreparedSetupActor,
    pub instance_id: String,
    pub driver: String,
    pub create_event: Event,
    pub status: LaunchStatus,
    pub message: Option<String>,
    seal: String,
}

impl LaunchJournal {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        draft: &ProjectTeamSetupDraft,
        reservation: authoring::AuthoringReservation,
        choice: LaunchChoice,
        actor: actor::PreparedSetupActor,
        instance_id: String,
        driver: String,
        create_event: Event,
    ) -> Self {
        Self {
            version: 1,
            setup_id: draft.setup_id.clone(),
            project_ref: draft.project_ref.clone(),
            owner_pubkey: draft.owner_pubkey.clone(),
            relay_url: draft.relay_url.clone(),
            reservation,
            choice,
            actor,
            instance_id,
            driver,
            create_event,
            status: LaunchStatus::Prepared,
            message: None,
            seal: String::new(),
        }
    }
    pub(super) fn response(&self) -> SetupAuthoringLaunch {
        SetupAuthoringLaunch {
            setup_id: self.setup_id.clone(),
            session_ref: self.reservation.session_ref.clone(),
            channel_id: self.reservation.channel_id.clone(),
            create_command_id: self.reservation.create_command_id.clone(),
            choice: self.choice.clone(),
            actor_pubkey: self.actor.actor_pubkey.clone(),
            pack_ref: self.actor.pack_ref.clone(),
            status: self.status,
            message: self.message.clone(),
            target: None,
            receipt_event_id: None,
        }
    }
}

fn path(draft: &ProjectTeamSetupDraft) -> Result<PathBuf, SetupError> {
    let root = Path::new(&draft.draft_directory)
        .parent()
        .ok_or_else(|| invalid("The setup storage directory is missing."))?;
    super::super::tree::ensure_contained_directory(root, root)?;
    Ok(root.join("launch.json"))
}
fn message(journal: &LaunchJournal) -> Result<Message, SetupError> {
    let mut copy = journal.clone();
    copy.seal.clear();
    let bytes = serde_json::to_vec(&copy).map_err(|e| invalid(e.to_string()))?;
    let mut hash = Sha256::new();
    hash.update(b"beekeeper:project-team-setup:launch:v1\0");
    hash.update(bytes);
    Ok(Message::from_digest(hash.finalize().into()))
}
fn validate(
    journal: &LaunchJournal,
    draft: &ProjectTeamSetupDraft,
    reservation: &authoring::AuthoringReservation,
) -> Result<(), SetupError> {
    if journal.version != 1
        || journal.setup_id != draft.setup_id
        || journal.project_ref != draft.project_ref
        || journal.owner_pubkey != draft.owner_pubkey
        || journal.relay_url != draft.relay_url
        || journal.reservation != *reservation
    {
        return Err(invalid(
            "The saved launch does not match this setup's reservation and owner scope.",
        ));
    }
    let signature = Signature::from_str(&journal.seal).map_err(|e| invalid(e.to_string()))?;
    let owner =
        nostr::PublicKey::from_hex(&draft.owner_pubkey).map_err(|e| invalid(e.to_string()))?;
    nostr::SECP256K1
        .verify_schnorr(
            &signature,
            &message(journal)?,
            &owner.xonly().map_err(|e| invalid(e.to_string()))?,
        )
        .map_err(|_| invalid("The saved launch metadata failed its owner seal."))?;
    journal
        .create_event
        .verify()
        .map_err(|_| invalid("The saved create signature is invalid."))?;
    // Rebuild only the unsigned payload/tags. Replaying retains the original
    // event timestamp, id and signature verbatim.
    let payload =
        beekeeper_core_pkg::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
            &journal.create_event.content,
        )
        .map_err(invalid)?;
    let channel =
        uuid::Uuid::parse_str(&reservation.channel_id).map_err(|e| invalid(e.to_string()))?;
    let expected_tags =
        beekeeper_sdk_pkg::builders::build_coding_session_lifecycle_command(channel, &payload)
            .map_err(|e| invalid(e.to_string()))?
            .build(owner)
            .tags;
    if journal.create_event.pubkey != owner
        || journal.create_event.kind.as_u16()
            != beekeeper_core_pkg::kind::KIND_CODING_SESSION_LIFECYCLE_COMMAND as u16
        || journal.create_event.tags != expected_tags
        || payload.command_id != reservation.create_command_id
    {
        return Err(invalid(
            "The saved create envelope does not match its reservation.",
        ));
    }
    if payload != create_payload(draft, reservation, &journal.choice, &journal.actor)? {
        return Err(invalid(
            "The saved create does not match the exact setup payload.",
        ));
    }
    if !matches!(
        journal.status,
        LaunchStatus::Prepared | LaunchStatus::AwaitingReceipt | LaunchStatus::Ambiguous
    ) {
        return Err(invalid(
            "A saved launch cannot claim a receipt that it does not contain.",
        ));
    }
    Ok(())
}

fn marker(draft: &ProjectTeamSetupDraft) -> Result<Option<String>, SetupError> {
    let path = path(draft)?.with_file_name("launch-event-id");
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    super::super::tree::check_regular_file(&path, 64)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut bytes = String::new();
    options.open(path)?.take(65).read_to_string(&mut bytes)?;
    if bytes.len() != 64
        || !bytes
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid(
            "The saved launch event marker is malformed; the original request must be recovered.",
        ));
    }
    Ok(Some(bytes))
}

pub(super) fn ensure_marker(
    draft: &ProjectTeamSetupDraft,
    event_id: &str,
) -> Result<(), SetupError> {
    if let Some(previous) = marker(draft)? {
        if previous != event_id {
            return Err(invalid(
                "The saved launch event ID no longer matches its recovery marker.",
            ));
        }
        return Ok(());
    }
    write_private(
        &path(draft)?.with_file_name("launch-event-id"),
        event_id.as_bytes(),
    )
}

pub(super) fn read(
    draft: &ProjectTeamSetupDraft,
    reservation: &authoring::AuthoringReservation,
) -> Result<Option<LaunchJournal>, SetupError> {
    let path = path(draft)?;
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if marker(draft)?.is_some() {
                return Err(invalid("The launch journal is missing but its saved event marker remains. Recover the original journal before retrying; no replacement request was signed."));
            }
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    super::super::tree::check_regular_file(&path, LIMIT)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut bytes = Vec::new();
    options
        .open(path)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(invalid("The saved launch exceeds its size bound."));
    }
    let saved: LaunchJournal =
        serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
    validate(&saved, draft, reservation)?;
    if marker(draft)?.is_some_and(|id| id != saved.create_event.id.to_hex()) {
        return Err(invalid(
            "The saved launch event ID no longer matches its recovery marker.",
        ));
    }
    Ok(Some(saved))
}

pub(super) fn save(
    draft: &ProjectTeamSetupDraft,
    journal: &LaunchJournal,
    keys: &Keys,
) -> Result<(), SetupError> {
    let path = path(draft)?;
    if keys.public_key().to_hex() != draft.owner_pubkey {
        return Err(invalid("Launch signing owner changed."));
    }
    let mut journal = journal.clone();
    journal.seal = keys.sign_schnorr(&message(&journal)?).to_string();
    validate(&journal, draft, &journal.reservation)?;
    let bytes = serde_json::to_vec(&journal).map_err(|e| invalid(e.to_string()))?;
    if bytes.len() as u64 > LIMIT {
        return Err(invalid("The saved launch exceeds its size bound."));
    }
    // Validate any previous journal before replacing its state.
    if let Some(previous) = read(draft, &journal.reservation)? {
        if previous.create_event != journal.create_event
            || previous.choice != journal.choice
            || previous.actor != journal.actor
            || previous.instance_id != journal.instance_id
            || previous.driver != journal.driver
        {
            return Err(invalid(
                "A launch transition cannot replace its signed request, actor or runtime choice.",
            ));
        }
    }
    write_private(&path, &bytes)?;
    ensure_marker(draft, &journal.create_event.id.to_hex())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), SetupError> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<(), SetupError> {
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        #[cfg(unix)]
        if let Some(parent) = path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub(super) fn lock(draft: &ProjectTeamSetupDraft) -> Result<std::fs::File, SetupError> {
    let path = path(draft)?.with_file_name("launch.lock");
    match std::fs::symlink_metadata(&path) {
        Ok(_) => super::super::tree::check_regular_file(&path, 0)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    file.lock()?;
    Ok(file)
}
