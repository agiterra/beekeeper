//! Exact accepted echo preserves the full observed private/local source.
use super::inbound_tests::local_agent;
use super::*;

#[test]
fn exact_accepted_actor_echo_keeps_private_source_cas_and_rejects_other_edits() {
    let source = local_agent();
    let mut desired = source.clone();
    desired.name = "Accepted name".into();
    desired.parallelism = 3;
    desired.respond_to = crate::managed_agents::RespondTo::Allowlist;
    desired.respond_to_allowlist = vec!["b".repeat(64)];
    let public = crate::managed_agents::agent_events::agent_event_content(&desired);
    let mut records = vec![source.clone()];
    apply_inbound_managed_agent(&mut records, &source.pubkey, public.clone());
    let echo = records.remove(0);
    assert!(matches_exact_managed_agent_echo(
        &source,
        &echo,
        public.clone()
    ));
    assert_eq!(echo.private_key_nsec, source.private_key_nsec);
    assert_eq!(echo.env_vars, source.env_vars);
    assert_eq!(echo.persona_source_version, source.persona_source_version);
    assert_eq!(echo.system_prompt, source.system_prompt);
    let mut changed = echo.clone();
    changed.private_key_nsec = "changed custody".into();
    assert!(!matches_exact_managed_agent_echo(
        &source,
        &changed,
        public.clone()
    ));
    changed = echo.clone();
    changed.env_vars.insert("API_KEY".into(), "changed".into());
    assert!(!matches_exact_managed_agent_echo(
        &source,
        &changed,
        public.clone()
    ));
    changed = echo.clone();
    changed.model = Some("another private runtime choice".into());
    assert!(!matches_exact_managed_agent_echo(
        &source,
        &changed,
        public.clone()
    ));
    changed = echo.clone();
    changed.name = "unrelated edit".into();
    assert!(!matches_exact_managed_agent_echo(
        &source,
        &changed,
        public.clone()
    ));
    changed = echo.clone();
    changed.respond_to_allowlist.push("c".repeat(64));
    assert!(!matches_exact_managed_agent_echo(&source, &changed, public));
}
