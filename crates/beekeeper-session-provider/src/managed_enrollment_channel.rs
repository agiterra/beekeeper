//! Channel-scoped additional responsibilities reuse protected modern enrollment.
use super::*;

/// A new channel is a separate responsibility, never an alias for the primary.
/// Only an attested owner's modern enrollment and actual channel membership
/// authorize reuse of its private workspace; existing command admission follows.
pub(super) async fn additional_workspace(
    config: &Config,
    state: &StateStore,
    rest: &beekeeper_acp::relay::RestClient,
    value: &Value,
    scope: (&str, Uuid, &str),
    context: Option<&(std::path::PathBuf, beekeeper_host_core::layout::Instance)>,
) -> Result<std::path::PathBuf, CreateFailure> {
    let (actor, channel, owner) = scope;
    let (home, instance) =
        context.ok_or_else(|| refused("new channel requires modern host ownership"))?;
    let held = ownership::for_pair(home, *instance, actor, &config.relay_url)
        .map_err(|_| refused("ownership unavailable"))?
        .ok_or_else(|| refused("new channel requires modern host ownership"))?;
    if !ownership::is_modern_enrollment(&held)
        || !matches!(held.phase, Phase::Adopted | Phase::HostStopped)
        || state.sessions().any(|record| {
            record.actor.as_deref() == Some(actor)
                && record.closed
                && record.process_witness.as_ref().is_none_or(|witness| {
                    !witness.proves_absent(&record.target(&config.instance_id))
                })
        })
    {
        return Err(refused(
            "actor ownership or prior process quiescence is unconfirmed",
        ));
    }
    let relay = rest
        .fetch_relay_self_verified()
        .await
        .map_err(|_| refused("channel membership authority unavailable"))?
        .ok_or_else(|| refused("channel membership authority unavailable"))?;
    let rows = rest
        .query(&[nostr::Filter::new()
            .kind(nostr::Kind::Custom(39002))
            .identifier(channel.to_string())])
        .await
        .map_err(|_| refused("requested channel membership unavailable"))?;
    let membership = rows
        .as_array()
        .ok_or_else(|| refused("channel membership malformed"))?
        .iter()
        .filter_map(|raw| serde_json::from_value::<Event>(raw.clone()).ok())
        .filter(|event| {
            event.kind.as_u16() == 39002
                && event.pubkey.to_hex() == relay
                && event.verify().is_ok()
                && event
                    .tags
                    .iter()
                    .any(|tag| tag.as_slice() == ["d", &channel.to_string()])
        })
        .max_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| b.id.cmp(&a.id))
        })
        .ok_or_else(|| refused("requested channel has no verified membership"))?;
    for participant in [owner, actor, &config.keys.public_key().to_hex()] {
        if !membership.tags.iter().any(|tag| {
            tag.as_slice().first().is_some_and(|kind| kind == "p")
                && tag.as_slice().get(1).is_some_and(|key| key == participant)
        }) {
            return Err(refused(
                "owner, actor and provider must join requested channel before execution",
            ));
        }
    }
    value
        .get("cwd")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .ok_or_else(|| refused("enrolled workspace missing"))
}
