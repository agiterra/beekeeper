//! Private request/reply at the execution dequeue boundary.
use crate::continuity_bootstrap::{PreparedBootstrap, Refusal};
use crate::continuity_collect::Custody;
use beekeeper_core::coding_session_command::CodingSessionTarget;
use tokio::sync::watch;

/// Private execution context; never serialized or logged.
#[derive(Clone)]
pub struct Request {
    pub(crate) target: CodingSessionTarget,
    pub(crate) command_id: String,
    pub(crate) text: String,
    pub(crate) operator: Option<String>,
    pub(crate) custody: Result<Custody, Refusal>,
    pub(crate) role_definition: Option<RoleDefinition>,
    pub(crate) runtime_briefing: Option<RuntimeBriefing>,
    pub(crate) checkpoint: String,
}
/// The exact runtime briefing this execution's adapter was (or will be) given,
/// composed once at open. The continuity block binds to it by size and digest;
/// it never carries a second copy.
#[derive(Clone)]
pub(crate) struct RuntimeBriefing {
    pub text: String,
    pub transport: crate::session::BootstrapTransport,
}
impl RuntimeBriefing {
    /// Size and digest, never the text.
    pub fn binding(&self) -> crate::continuity_bootstrap::RuntimeBinding {
        crate::continuity_bootstrap::RuntimeBinding {
            bytes: self.text.len(),
            digest: beekeeper_core::continuity::value_hash(self.text.as_bytes()),
            transport: match self.transport {
                crate::session::BootstrapTransport::SystemPrompt => "system_prompt",
                crate::session::BootstrapTransport::FirstTurn => "first_turn",
            },
        }
    }
}
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ContinuityRequest(<private>)")
    }
}
/// Staged role contract from the same custody used to open the execution.
#[derive(Clone)]
pub(crate) struct RoleDefinition {
    pub pack: Option<crate::actor_seats::PackRef>,
    pub compose: Option<beekeeper_core::coding_session_payload::ComposeRef>,
    pub text: String,
}
/// Actor-owned bootstrap state; native restore never constructs this.
pub(crate) struct Execution {
    pub target: CodingSessionTarget,
    pub custody: Result<Custody, Refusal>,
    pub last_receipt: Option<PreparedBootstrap>,
    pub role_definition: Option<RoleDefinition>,
    pub runtime_briefing: Option<RuntimeBriefing>,
    pub checkpoint: String,
}
impl Execution {
    pub fn from_launch(
        request: &crate::session::CreateRequest,
        continuity: &crate::session::SessionContinuity,
        runtime_briefing: Option<RuntimeBriefing>,
    ) -> Option<Self> {
        // Only explicit new conversations. An implicit native fallback is not
        // consent to reconstruct, and native resume/load remains native.
        if request.seat.is_none()
            || request.resume_cursor.is_some()
            || !matches!(
                continuity,
                crate::session::SessionContinuity::Fresh
                    | crate::session::SessionContinuity::Rehydrated
            )
        {
            return None;
        }
        let role_definition = request.seat_skills.as_ref().and_then(|skills| {
            beekeeper_persona::resolve::resolve_persona_by_name(
                &skills.pack_dir,
                &skills.persona_id,
            )
            .ok()
            .map(|persona| RoleDefinition {
                pack: skills.pack_ref.clone(),
                compose: skills.compose_ref.clone(),
                text: persona.system_prompt,
            })
        });
        Some(Self {
            target: request.target.clone(), custody: Custody::from_launch(request),
            last_receipt: None, role_definition, runtime_briefing,
            checkpoint: request.rehydration_mcp.as_ref().map(|d| d.first_turn_brief.clone())
                .unwrap_or_else(|| "No verified prior checkpoint package is available; no past report establishes current completion.".into()),
        })
    }
    pub async fn prepare(
        &self,
        events: &tokio::sync::mpsc::Sender<crate::session::SessionEvent>,
        shutdown: &mut watch::Receiver<bool>,
        command: &str,
        text: &str,
        operator: Option<&str>,
    ) -> Result<PreparedBootstrap, Refusal> {
        let (decision, mut answer) = watch::channel(None);
        let request = Request {
            target: self.target.clone(),
            command_id: command.into(),
            text: text.into(),
            operator: operator.map(str::to_owned),
            custody: self.custody.clone(),
            role_definition: self.role_definition.clone(),
            runtime_briefing: self.runtime_briefing.clone(),
            checkpoint: self.checkpoint.clone(),
        };
        events
            .send(crate::session::SessionEvent::ContinuityAdmissionRequested {
                request: Box::new(request),
                decision,
            })
            .await
            .map_err(|_| Refusal::Unavailable)?;
        tokio::select! {
            biased;
            _=shutdown.changed()=>Err(Refusal::Unavailable),
            changed=answer.changed()=>{
                changed.map_err(|_|Refusal::Unavailable)?;
                answer.borrow().clone().ok_or(Refusal::Unavailable)?
            }
            _=tokio::time::sleep(std::time::Duration::from_secs(30))=>Err(Refusal::Unavailable),
        }
    }
}
