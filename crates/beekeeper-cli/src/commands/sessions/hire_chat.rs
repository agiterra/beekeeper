//! Exact owner-authored chat refusal recognition for one signed hire.
use super::crew::{parse_hire_refusal, HireRefusal};
use nostr::Event;
use serde_json::Value;

pub(super) fn has_return_address(event: &Event) -> bool {
    beekeeper_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(&event.content)
        .is_ok_and(|payload| matches!(payload.action,
            beekeeper_core::coding_session_lifecycle_command::CodingSessionLifecycleAction::SessionHire { reply_to: Some(_), .. }))
}

pub(super) fn find_refusal(
    rows: &[Value],
    channel: &str,
    genesis: &str,
    request: &Event,
) -> Option<HireRefusal> {
    if request.verify().is_err() {
        return None;
    }
    let payload =
        beekeeper_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
            &request.content,
        )
        .ok()?;
    let beekeeper_core::coding_session_lifecycle_command::CodingSessionLifecycleAction::SessionHire { reply_to: Some(source), genesis_ref, .. } = payload.action else { return None; };
    if genesis_ref != genesis || !tag(request, "h", channel) {
        return None;
    }
    let events: Vec<Event> = rows
        .iter()
        .filter_map(|row| serde_json::from_value(row.clone()).ok())
        .filter(|event: &Event| event.verify().is_ok())
        .collect();
    let founder = events
        .iter()
        .find(|event| {
            event.id.to_hex() == genesis
                && event.kind.as_u16() as u32 == beekeeper_core::kind::KIND_CODING_SESSION_GENESIS
                && tag(event, "h", channel)
        })?
        .pubkey;
    events
        .iter()
        .filter(|event| {
            event.kind.as_u16() == 9
                && event.pubkey == founder
                && tag(event, "h", channel)
                && tag(event, "hire-ref", &request.id.to_hex())
                && tag(
                    event,
                    beekeeper_core::coding_session_payload::CODING_SESSION_HOST_ANSWER_TAG_NAME,
                    beekeeper_core::coding_session_payload::CODING_SESSION_HOST_ANSWER_TAG_HIRE,
                )
                && event
                    .tags
                    .iter()
                    .any(|tag| tag.as_slice() == ["e", &source, "", "reply"])
                && tag(event, "p", &request.pubkey.to_hex())
        })
        .find_map(|event| {
            let (code, reason) = parse_hire_refusal(&event.content)?;
            Some(HireRefusal {
                event_id: event.id.to_hex(),
                at: event.created_at.as_secs() as i64,
                code,
                reason,
            })
        })
}
fn tag(event: &Event, name: &str, value: &str) -> bool {
    event.tags.iter().any(|tag| {
        tag.as_slice().first().is_some_and(|v| v == name)
            && tag.as_slice().get(1).is_some_and(|v| v == value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{Keys, Tag};
    #[test]
    fn exact_signed_founder_notice_ends_wait_and_foreign_or_other_hire_does_not() {
        let owner = Keys::generate();
        let lead = Keys::generate();
        let channel = uuid::Uuid::new_v4();
        let genesis = beekeeper_sdk::build_coding_session_genesis(
            channel,
            &beekeeper_core::coding_session_genesis::CodingSessionGenesisPayload::new(
                uuid::Uuid::new_v4().to_string(),
            ),
        )
        .unwrap()
        .sign_with_keys(&owner)
        .unwrap();
        let source = "a".repeat(64);
        let mut payload = super::super::crew::hire_payload(
            &uuid::Uuid::new_v4().to_string(),
            &uuid::Uuid::new_v4().to_string(),
            &genesis.id.to_hex(),
            "runner",
            None,
            None,
            "bounded work",
            Some(&lead.public_key().to_hex()),
            None,
        );
        if let beekeeper_core::coding_session_lifecycle_command::CodingSessionLifecycleAction::SessionHire { reply_to, .. } = &mut payload.action { *reply_to = Some(source.clone()); }
        let request = beekeeper_sdk::build_coding_session_lifecycle_command(channel, &payload)
            .unwrap()
            .sign_with_keys(&lead)
            .unwrap();
        let build = |keys: &Keys, hire: &str| {
            beekeeper_sdk::build_message(
                channel,
                "hire refused: HIRE_MODEL_NOT_OFFERED — exact model required",
                Some(&beekeeper_sdk::ThreadRef {
                    root_event_id: nostr::EventId::from_hex(&source).unwrap(),
                    parent_event_id: nostr::EventId::from_hex(&source).unwrap(),
                }),
                &[&lead.public_key().to_hex()],
                false,
                &[],
            )
            .unwrap()
            .tags([
                Tag::parse(["hire-ref", hire]).unwrap(),
                Tag::parse([
                    beekeeper_core::coding_session_payload::CODING_SESSION_HOST_ANSWER_TAG_NAME,
                    "hire",
                ])
                .unwrap(),
            ])
            .sign_with_keys(keys)
            .unwrap()
        };
        for notice in [
            build(&lead, &request.id.to_hex()),
            build(&owner, &"b".repeat(64)),
        ] {
            assert!(find_refusal(
                &[
                    serde_json::to_value(&genesis).unwrap(),
                    serde_json::to_value(notice).unwrap()
                ],
                &channel.to_string(),
                &genesis.id.to_hex(),
                &request
            )
            .is_none());
        }
        let notice = build(&owner, &request.id.to_hex());
        let result = find_refusal(
            &[
                serde_json::to_value(&genesis).unwrap(),
                serde_json::to_value(&notice).unwrap(),
            ],
            &channel.to_string(),
            &genesis.id.to_hex(),
            &request,
        )
        .unwrap();
        assert_eq!(result.event_id, notice.id.to_hex());
        assert_eq!(result.code, "HIRE_MODEL_NOT_OFFERED");
    }
}
