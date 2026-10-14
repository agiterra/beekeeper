//! Existing session boundary preparation with protected managed adapter selection.
use super::*;

/// [`crate::Provider::execution_plan`] from the provider's configuration
/// alone, so a restore can prepare its scope off the run loop (SV-76).
pub(crate) fn execution_plan_for(
    config: &crate::config::Config,
    facts: &LaunchFacts<'_>,
) -> Result<ExecutionPlan, CreateFailure> {
    let projects = crate::commands::ProjectsFile::load(config.projects_file.as_deref());
    let project_checkout = facts
        .project_ref
        .and_then(|project| projects.projects.get(project))
        .cloned();
    let agents = facts
        .seat_skills
        .and_then(|skills| skills.agents_checkout.as_ref())
        .map(|agents| (agents.path.as_path(), agents.writable()))
        .or_else(|| facts.unseated_agents.map(|path| (path, false)));
    let role_contract = role_contract(facts.role, facts.seat_skills);
    let hermit_state = crate::execution_scope_host::host_hermit_state();
    // Every bounded session gets the host's own `bee`: offline commands
    // work for all, and relay commands still need the identity only a
    // seat is given.
    let bee = crate::seat_bee::host_seat_bee().map(|(bee, _)| bee.path.clone());
    let runtime_roots =
        crate::runtime_installations::roots(&config.state_dir, facts.runtime_instance, facts.cwd)?;
    let settings = crate::managed_configuration::Settings::load(config, facts.actor)?;
    let configured_mcp = settings
        .as_ref()
        .map(|settings| settings.mcp_command())
        .transpose()?
        .flatten();
    let mut inputs = ScopeInputs::new(
        ScopePurpose::Session,
        &config.state_dir,
        facts.session_id,
        facts.cwd,
    );
    inputs.project_ref = facts.project_ref;
    inputs.project_checkout = project_checkout.as_deref();
    inputs.association = facts.association;
    inputs.actor = facts.actor;
    inputs.driver = facts.driver;
    inputs.runtime = if facts.runtime_instance == beekeeper_host_core::managed_runtime::INSTANCE_REF
    {
        let settings = crate::managed_configuration::Settings::load(config, facts.actor)?
            .ok_or_else(|| {
                refuse(
                    EXECUTION_BOUNDARY_UNAVAILABLE,
                    "managed adapter requires protected configured execution facts",
                )
            })?;
        let mut descriptor = config
            .runtime(facts.runtime_instance)
            .cloned()
            .ok_or_else(|| {
                refuse(
                    EXECUTION_BOUNDARY_UNAVAILABLE,
                    "managed adapter slot is not commissioned",
                )
            })?;
        settings.apply_runtime(&mut descriptor)?;
        if descriptor.agent_command != facts.agent_command
            || descriptor.agent_args != facts.agent_args
        {
            return Err(refuse(
                EXECUTION_BOUNDARY_UNAVAILABLE,
                "managed execution facts differ from protected adapter configuration",
            ));
        }
        let command = Path::new(&descriptor.agent_command)
            .file_name()
            .and_then(|name| name.to_str());
        match (descriptor.runtime.as_str(), command) {
            ("codex", Some("codex-acp")) => RuntimeProfile::Codex,
            ("claude", Some("claude-agent-acp")) => RuntimeProfile::Claude,
            _ => RuntimeProfile::GenericManaged,
        }
    } else {
        config
            .runtime_profile_override
            .unwrap_or_else(|| RuntimeProfile::for_launch(facts.driver, facts.agent_command))
    };
    inputs.runtime_roots = &runtime_roots;
    inputs.agent_command = facts.agent_command;
    inputs.agent_args = facts.agent_args;
    inputs.agent_env = facts.agent_env;
    inputs.identity_env = facts.identity_env;
    inputs.agents_checkout = agents;
    inputs.seat_bundle = facts.seat_skills.map(|skills| skills.bundle_dir.as_path());
    inputs.role_contract = role_contract.as_deref();
    inputs.configured_mcp_command = configured_mcp.as_deref();
    inputs.context_mcp = facts
        .rehydration
        .map(|mcp| (mcp.command.as_path(), mcp.package_dir.as_path()));
    inputs.seat_bee = bee.as_deref();
    inputs.hermit_state = hermit_state.as_deref();
    inputs.prior = facts.prior;
    inputs.isolation = config.session_isolation;
    let mut plan = prepare(&inputs)?;
    if let ExecutionPlan::Prepared(prepared) = &mut plan {
        let granted = crate::full_access::granted(&config.state_dir, facts.session_id);
        // A provider that confines egress runs no session outside the
        // boundary: full access would remove the network rule with it.
        // The session then discloses the boundary it actually runs in.
        if granted && config.session_isolation.egress.proxy().is_some() {
            tracing::warn!(
                target: "csp::scope",
                session_id = %facts.session_id,
                "full access was granted for this session, but this provider confines \
                 session egress, so it runs inside the boundary"
            );
        }
        prepared.full_access = granted && config.session_isolation.egress.proxy().is_none();
    }
    Ok(plan)
}
