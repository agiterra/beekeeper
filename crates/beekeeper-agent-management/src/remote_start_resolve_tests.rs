//! Exact accepted primary-channel resolution is read-only and fail-closed.
use super::*;

#[tokio::test]
async fn exact_primary_channel_resolver_accepts_active_and_stopped_stream_without_dm_fallback() {
    let (client, args, configured_dm) = additional_fixture();
    client.members.lock().expect("members").insert(
        args.channel,
        vec![
            client.owner.public_key().to_hex(),
            client.actor.public_key().to_hex(),
            client.provider.public_key().to_hex(),
        ],
    );
    let exact = target();
    client.events.lock().expect("events").push(metadata(
        &client,
        args.channel,
        exact.clone(),
        beekeeper_core::coding_session_payload::SessionStatus::Idle,
    ));
    let mut lookup = args.clone();
    lookup.channel = configured_dm;
    assert_eq!(
        resolve_remote_start_channel(&lookup, &exact, &client)
            .await
            .expect("accepted active stream"),
        args.channel
    );
    client.events.lock().expect("events").clear();
    client.events.lock().expect("events").push(metadata(
        &client,
        args.channel,
        exact.clone(),
        beekeeper_core::coding_session_payload::SessionStatus::Stopped,
    ));
    assert_eq!(
        resolve_remote_start_channel(&lookup, &exact, &client)
            .await
            .expect("accepted stopped stream"),
        args.channel
    );
    assert_eq!(lookup.channel, configured_dm, "request remains immutable");
    assert_eq!(
        client.events.lock().expect("events").len(),
        1,
        "resolver never publishes"
    );
    lookup.provider_instance = "wrong-runtime".into();
    assert!(matches!(
        resolve_remote_start_channel(&lookup, &exact, &client).await,
        Err(Error::Conflict(_))
    ));
    lookup.provider_instance = args.provider_instance.clone();
    client
        .members
        .lock()
        .expect("members")
        .insert(args.channel, vec![client.actor.public_key().to_hex()]);
    assert!(matches!(
        resolve_remote_start_channel(&lookup, &exact, &client).await,
        Err(Error::Refused(_))
    ));
}
#[tokio::test]
async fn exact_primary_channel_resolver_refuses_absent_and_ambiguous_signed_metadata() {
    let (client, args, configured_dm) = additional_fixture();
    client.members.lock().expect("members").insert(
        args.channel,
        vec![
            client.owner.public_key().to_hex(),
            client.actor.public_key().to_hex(),
            client.provider.public_key().to_hex(),
        ],
    );
    let exact = target();
    let mut lookup = args.clone();
    lookup.channel = configured_dm;
    assert!(matches!(
        resolve_remote_start_channel(&lookup, &exact, &client).await,
        Err(Error::Unconfirmed(_))
    ));
    client.events.lock().expect("events").extend([
        metadata(
            &client,
            args.channel,
            exact.clone(),
            beekeeper_core::coding_session_payload::SessionStatus::Idle,
        ),
        metadata(
            &client,
            configured_dm,
            exact.clone(),
            beekeeper_core::coding_session_payload::SessionStatus::Idle,
        ),
    ]);
    assert!(matches!(
        resolve_remote_start_channel(&lookup, &exact, &client).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(client.events.lock().expect("events").len(), 2);
    assert_eq!(lookup.channel, configured_dm);
}
