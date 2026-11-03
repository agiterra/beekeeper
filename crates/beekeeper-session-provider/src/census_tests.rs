use super::*;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use std::cell::RefCell;

const SENTINEL: &str = "CENSUS_SENTINEL_MUST_NOT_PRINT";

fn command(
    keys: &Keys,
    channel: Uuid,
    provider: &str,
    id: &str,
    at: u64,
    turn: Option<&str>,
) -> Value {
    use beekeeper_core::coding_session_identity::ProviderInstanceAlias;
    use beekeeper_core::coding_session_lifecycle_command::{
        CodingSessionLifecycleCommandPayload, CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
    };
    let payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_string(),
        command_id: id.to_string(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: Some(format!("30621:{}:slug", "a".repeat(64))),
            repo_ref: None,
            session_ref: None,
            genesis_ref: None,
            provider_instance_ref: ProviderInstanceAlias::from_wire("acme-primary").unwrap(),
            provider_authority_pubkey: provider.to_string(),
            model: Some("sonnet[low]".into()),
            title: Some(SENTINEL.into()),
            initial_turn: turn.map(str::to_owned),
            actor: None,
            role: None,
            hire_ref: None,
            routing: None,
        },
    };
    let event = beekeeper_sdk::build_coding_session_lifecycle_command(channel, &payload)
        .unwrap()
        .custom_created_at(Timestamp::from_secs(at))
        .sign_with_keys(keys)
        .unwrap();
    serde_json::to_value(event).unwrap()
}

/// A relay's keyset paging, over rows it orders as the database does.
struct FakeRelay {
    rows: Vec<Value>,
    limit: usize,
    keyset: bool,
    fail: RefCell<bool>,
}

impl FakeRelay {
    fn new(mut rows: Vec<Value>, limit: usize) -> Self {
        rows.sort_by(|a, b| {
            b["created_at"]
                .as_u64()
                .cmp(&a["created_at"].as_u64())
                .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
        });
        Self {
            rows,
            limit,
            keyset: true,
            fail: RefCell::new(false),
        }
    }
}

impl CensusReader for FakeRelay {
    async fn page(&self, _filter: Value, cursor: Option<(u64, String)>) -> Result<Vec<Value>, ()> {
        if *self.fail.borrow() {
            return Err(());
        }
        Ok(self
            .rows
            .iter()
            .filter(|row| match &cursor {
                None => true,
                Some((until, id)) if self.keyset => {
                    let at = row["created_at"].as_u64().unwrap();
                    at < *until || (at == *until && row["id"].as_str().unwrap() > id.as_str())
                }
                Some((until, _)) => row["created_at"].as_u64().unwrap() <= *until,
            })
            .take(self.limit)
            .cloned()
            .collect())
    }
}

fn bare_rows(count: usize, at: u64) -> Vec<Value> {
    (0..count)
        .map(|n| json!({"id": format!("{n:064x}"), "created_at": at}))
        .collect()
}

#[tokio::test]
async fn five_hundred_and_one_in_one_second_are_all_read_by_the_keyset() {
    let relay = FakeRelay::new(bare_rows(501, 1000), 500);
    let rows = read_all(&relay, json!({})).await.unwrap();
    assert_eq!(rows.len(), 501);
}

#[tokio::test]
async fn a_cursor_that_does_not_move_is_never_complete() {
    // A relay that ignores `before_id`: the same full page comes back.
    let mut relay = FakeRelay::new(bare_rows(501, 1000), 500);
    relay.keyset = false;
    assert_eq!(
        read_all(&relay, json!({})).await.unwrap_err(),
        "TIMESTAMP_SATURATED"
    );
}

#[tokio::test]
async fn a_short_page_is_not_taken_for_the_end() {
    // A relay clamping below what was asked.
    let mut rows = bare_rows(30, 2000);
    rows.extend((30..60).map(|n| json!({"id": format!("{n:064x}"), "created_at": 1000})));
    let relay = FakeRelay::new(rows, 7);
    assert_eq!(read_all(&relay, json!({})).await.unwrap().len(), 60);
}

#[tokio::test]
async fn a_failed_read_says_so() {
    let relay = FakeRelay::new(bare_rows(1, 1), 10);
    *relay.fail.borrow_mut() = true;
    assert_eq!(
        read_all(&relay, json!({})).await.unwrap_err(),
        "QUERY_FAILED"
    );
}

struct Case {
    keys: Keys,
    provider: String,
    channel: Uuid,
}

impl Case {
    fn new() -> Self {
        Self {
            keys: Keys::generate(),
            provider: "ab".repeat(32),
            channel: Uuid::new_v4(),
        }
    }
    fn row(
        &self,
        raw: &Value,
        state: &CensusState,
        consumed: &[&str],
        refused: &[&str],
        now: u64,
    ) -> Row {
        let consumed = consumed.iter().map(|s| s.to_string()).collect();
        let refused = refused.iter().map(|s| s.to_string()).collect();
        classify(
            raw,
            self.channel,
            5000,
            &Facts {
                provider: &self.provider,
                now,
                horizon: 86_400,
                state,
                consumed: &consumed,
                refused: &refused,
            },
        )
    }
}

const NOW: u64 = 1_000_000;

#[test]
fn each_class() {
    let case = Case::new();
    let empty = CensusState::default();
    let id = "11111111-1111-4111-8111-111111111111";
    let raw = command(&case.keys, case.channel, &case.provider, id, NOW - 10, None);
    assert_eq!(
        case.row(&raw, &empty, &[], &[], NOW).class,
        Class::Candidate
    );
    assert_eq!(
        case.row(&raw, &empty, &[id], &[], NOW).class,
        Class::Consumed
    );
    assert_eq!(
        case.row(&raw, &empty, &[], &[id], NOW).class,
        Class::Refused
    );

    let other = command(
        &case.keys,
        case.channel,
        &"cd".repeat(32),
        id,
        NOW - 10,
        None,
    );
    assert_eq!(
        case.row(&other, &empty, &[], &[], NOW).class,
        Class::OtherProvider
    );

    let old = command(
        &case.keys,
        case.channel,
        &case.provider,
        id,
        NOW - 86_401,
        None,
    );
    assert_eq!(case.row(&old, &empty, &[], &[], NOW).class, Class::Expired);

    let mut tampered = raw.clone();
    tampered["content"] = json!("{}");
    assert_eq!(
        case.row(&tampered, &empty, &[], &[], NOW).class,
        Class::Invalid
    );

    let elsewhere = Case {
        channel: Uuid::new_v4(),
        keys: case.keys.clone(),
        provider: case.provider.clone(),
    };
    assert_eq!(
        elsewhere.row(&raw, &empty, &[], &[], NOW).class,
        Class::Invalid
    );

    let junk = EventBuilder::new(Kind::Custom(44221), "not a command")
        .tags([Tag::parse(["h", &case.channel.to_string()]).unwrap()])
        .sign_with_keys(&case.keys)
        .unwrap();
    assert_eq!(
        case.row(&serde_json::to_value(junk).unwrap(), &empty, &[], &[], NOW)
            .class,
        Class::Malformed
    );
}

#[test]
fn a_held_intent_is_correlated_by_its_fingerprint_not_its_id_alone() {
    let case = Case::new();
    let id = "22222222-2222-4222-8222-222222222222";
    let raw = command(&case.keys, case.channel, &case.provider, id, NOW - 10, None);
    let event: Event = serde_json::from_value(raw.clone()).unwrap();
    let print = lifecycle_fingerprint(case.channel, &event.pubkey.to_hex(), &event.content);
    let held = |phase, print: &str| CensusState {
        dispositions: BTreeMap::from([(
            id.to_owned(),
            Some(state::HeldStart {
                phase,
                fingerprint: print.to_owned(),
                channel: Uuid::nil(),
                actor: None,
                initial_turn: false,
            }),
        )]),
        ..Default::default()
    };
    assert_eq!(
        case.row(
            &raw,
            &held(LifecycleStartPhase::BeforeEffects, &print),
            &[],
            &[],
            NOW
        )
        .class,
        Class::IntentBeforeEffects
    );
    assert_eq!(
        case.row(
            &raw,
            &held(LifecycleStartPhase::EffectsStarted, &print),
            &[],
            &[],
            NOW
        )
        .class,
        Class::IntentUncertain
    );
    assert_eq!(
        case.row(
            &raw,
            &held(LifecycleStartPhase::BeforeEffects, "different"),
            &[],
            &[],
            NOW
        )
        .class,
        Class::IntentConflict
    );
    // Even with the id in the ledger: an unresolved start is not "consumed".
    assert_eq!(
        case.row(
            &raw,
            &held(LifecycleStartPhase::RuntimeStarting, &print),
            &[id],
            &[],
            NOW
        )
        .class,
        Class::IntentUncertain
    );
}

#[test]
fn a_future_dated_command_is_a_candidate_and_says_so() {
    let case = Case::new();
    let raw = command(
        &case.keys,
        case.channel,
        &case.provider,
        "33333333-3333-4333-8333-333333333333",
        NOW + 800,
        None,
    );
    let row = case.row(&raw, &CensusState::default(), &[], &[], NOW);
    assert_eq!(row.class, Class::Candidate);
    assert_eq!(row.span, "futureDated");
    let behind = command(
        &case.keys,
        case.channel,
        &case.provider,
        "33333333-3333-4333-8333-333333333334",
        4000,
        None,
    );
    assert_eq!(
        case.row(&behind, &CensusState::default(), &[], &[], 6000)
            .span,
        "behindFloor"
    );
}

#[test]
fn no_body_prompt_title_or_signature_reaches_a_row() {
    let case = Case::new();
    let raw = command(
        &case.keys,
        case.channel,
        &case.provider,
        "44444444-4444-4444-8444-444444444444",
        NOW,
        Some(SENTINEL),
    );
    let row = case.row(&raw, &CensusState::default(), &[], &[], NOW);
    assert!(row.initial_turn);
    assert_eq!(row.model.as_deref(), Some("sonnet[low]"));
    let text = serde_json::to_string(&row).unwrap();
    assert!(!text.contains(SENTINEL), "{text}");
    assert!(!text.contains(raw["sig"].as_str().unwrap()), "{text}");
    assert!(!text.contains("content"), "{text}");
}

#[test]
fn a_ledger_entry_a_start_would_prune_counts_as_absent() {
    let body = format!(
        "{}\n{}\nnot json\n",
        json!({"commandId":"kept","at":NOW - 10}),
        json!({"commandId":"gone","at":NOW - 86_500})
    );
    let seen = state::census_ledger(Some(body.as_bytes()), 86_400, NOW);
    assert!(seen.contains("kept") && !seen.contains("gone"));
    assert!(state::census_ledger(None, 86_400, NOW).is_empty());
}

#[test]
fn the_floor_is_the_start_floor_and_a_route_can_lower_it() {
    assert_eq!(
        crate::replay_floor(Some(5000), Some(9000), None),
        Some(5000)
    );
    assert_eq!(
        crate::replay_floor(None, Some(9000), None),
        Some(9000 - 600)
    );
    assert_eq!(
        crate::replay_floor(Some(5000), Some(9000), Some(4000)),
        Some(4000)
    );
    assert_eq!(lookback_since(5000), 5000 - 965);
}

fn write(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).unwrap();
}

#[tokio::test]
async fn any_change_between_the_two_reads_forces_a_retry_then_gives_up() {
    let dir = tempfile::tempdir().unwrap();
    let relay = FakeRelay::new(Vec::new(), 10);
    let read_at = |d: &Path| LocalInputs::read(d, None, None);
    let keep = std::time::SystemTime::now();

    // Stable: a clean answer.
    let path = dir.path().to_owned();
    assert!(stable_survey(&relay, "aa", 86_400, || read_at(&path))
        .await
        .is_ok());

    // A byte changed in place, with the mtime put back, at every read.
    write(dir.path(), "commands.jsonl", "");
    let count = RefCell::new(0u32);
    let flip = || {
        let mut n = count.borrow_mut();
        *n += 1;
        write(
            &path,
            "commands.jsonl",
            &format!("{{\"commandId\":\"{n}\",\"at\":1}}\n"),
        );
        std::fs::File::options()
            .write(true)
            .open(path.join("commands.jsonl"))
            .unwrap()
            .set_modified(keep)
            .unwrap();
        read_at(&path)
    };
    assert_eq!(
        stable_survey(&relay, "aa", 86_400, flip).await.unwrap_err(),
        "SNAPSHOT_UNSTABLE"
    );

    // A file appearing, and a lock changing hands, are each a difference.
    let plain = read_at(&path).unwrap();
    write(&path, "conversation-routes.json", "{\"routes\":[]}");
    assert_ne!(plain.digests, read_at(&path).unwrap().digests);
    let plain = read_at(&path).unwrap();
    write(&path, state::LOCK_FILE, "4242");
    assert_ne!(plain.digests, read_at(&path).unwrap().digests);
}

#[tokio::test]
async fn an_unreadable_channel_is_unknown_and_never_clean() {
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let now = now_secs();
    write(
        dir.path(),
        "state.json",
        &json!({"version":1,"watermarks":{channel.to_string():now - 100}}).to_string(),
    );
    let inputs = LocalInputs::read(dir.path(), None, None).unwrap();
    // Not a member, and its floor is recent: it cannot be called clean.
    let relay = FakeRelay::new(Vec::new(), 10);
    let (channels, commands) = survey(&relay, "aa", 86_400, now, &inputs).await.unwrap();
    assert_eq!(channels[0].result, "unknown");
    assert_eq!(verdict(&channels, &commands).0, EXIT_INCOMPLETE);
    // Not a member and long quiet: a start would not read it, and it cannot matter.
    write(
        dir.path(),
        "state.json",
        &json!({"version":1,"watermarks":{channel.to_string():now - 90_000}}).to_string(),
    );
    let inputs = LocalInputs::read(dir.path(), None, None).unwrap();
    let (channels, commands) = survey(&relay, "aa", 86_400, now, &inputs).await.unwrap();
    assert_eq!(channels[0].result, "notMember");
    assert_eq!(verdict(&channels, &commands).0, EXIT_CLEAN);
}

fn held_state(channel: Uuid, phase: &str) -> String {
    let answer = EventBuilder::new(Kind::Custom(44224), "{}")
        .sign_with_keys(&Keys::generate())
        .unwrap();
    json!({"version":1,"terminalDispositions":{"55555555-5555-4555-8555-555555555555":{
        "semanticKey":"k","event":answer,"lifecycleStart":{
            "phase":phase,"fingerprint":"ff",
            "target":{"driver":"d","instanceId":"i","sessionId":"s","generation":1},
            "actor":"ab".repeat(32),"channel":channel,"providerRef":"p","runtime":"r","initialTurn":true}}}})
    .to_string()
}

fn member(channel: Uuid) -> Value {
    json!({"id":"m".repeat(64),"created_at":1,"tags":[["d",channel.to_string()]]})
}

#[tokio::test]
async fn a_held_start_counts_though_no_row_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let now = now_secs();
    for (phase, class) in [
        ("before_effects", Class::IntentBeforeEffects),
        ("effects_started", Class::IntentUncertain),
        ("runtime_starting", Class::IntentUncertain),
    ] {
        write(dir.path(), "state.json", &held_state(channel, phase));
        let inputs = LocalInputs::read(dir.path(), None, None).unwrap();
        // A member channel, with nothing in the read.
        struct Members(Uuid);
        impl CensusReader for Members {
            async fn page(
                &self,
                filter: Value,
                cursor: Option<(u64, String)>,
            ) -> Result<Vec<Value>, ()> {
                Ok(if filter["kinds"][0] == MEMBERS_KIND && cursor.is_none() {
                    vec![member(self.0)]
                } else {
                    Vec::new()
                })
            }
        }
        let (channels, commands) = survey(&Members(channel), "aa", 86_400, now, &inputs)
            .await
            .unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].class, class);
        assert_eq!(commands[0].span, "heldInState");
        assert!(commands[0].initial_turn);
        assert_eq!(verdict(&channels, &commands).0, EXIT_ROWS, "{phase}");

        // The same reservation in a channel it is no longer in: not clean, and listed.
        let relay = FakeRelay::new(Vec::new(), 10);
        let (channels, commands) = survey(&relay, "aa", 86_400, now, &inputs).await.unwrap();
        assert_eq!(channels[0].channel, channel);
        assert_eq!(channels[0].result, "unknown");
        assert_eq!(commands[0].span, "heldInState");
        assert_eq!(verdict(&channels, &commands).0, EXIT_INCOMPLETE, "{phase}");
    }
}

#[tokio::test]
async fn another_providers_command_cannot_hide_a_held_start() {
    struct Rows(Uuid, Value);
    impl CensusReader for Rows {
        async fn page(
            &self,
            filter: Value,
            cursor: Option<(u64, String)>,
        ) -> Result<Vec<Value>, ()> {
            Ok(if cursor.is_some() {
                Vec::new()
            } else if filter["kinds"][0] == MEMBERS_KIND {
                vec![member(self.0)]
            } else {
                vec![self.1.clone()]
            })
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let now = now_secs();
    let provider = "aa".repeat(32);
    let relay = Rows(
        channel,
        command(
            &Keys::generate(),
            channel,
            &"bb".repeat(32),
            "55555555-5555-4555-8555-555555555555",
            now,
            None,
        ),
    );
    for phase in ["before_effects", "effects_started", "runtime_starting"] {
        write(dir.path(), "state.json", &held_state(channel, phase));
        let inputs = LocalInputs::read(dir.path(), None, None).unwrap();
        let (channels, commands) = survey(&relay, &provider, 86_400, now, &inputs)
            .await
            .unwrap();
        assert!(commands.iter().any(|row| row.class == Class::OtherProvider));
        assert!(commands
            .iter()
            .any(|row| row.span == "heldInState" && row.class.may_act()));
        assert_eq!(verdict(&channels, &commands).0, EXIT_ROWS, "{phase}");
    }
}

#[test]
fn a_caller_shell_may_not_set_the_horizon_in_either_spelling() {
    assert!(!horizon_overridden(|_| None));
    for name in [
        "BEEKEEPER_CSP_COMMAND_HORIZON_SECS",
        "BUZZ_CSP_COMMAND_HORIZON_SECS",
    ] {
        assert!(horizon_overridden(
            |asked| (asked == name).then(|| "60".into())
        ));
        // Set and empty is still somebody's intent.
        assert!(horizon_overridden(
            |asked| (asked == name).then(|| "".into())
        ));
    }
}

#[test]
fn only_a_field_that_is_what_it_says_is_printed() {
    let good = "1".repeat(64);
    assert!(hex64(&good) && !hex64(&"A".repeat(64)) && !hex64("ID_SENTINEL"));
    assert!(safe_command_id("11111111-1111-4111-8111-111111111111"));
    assert!(safe_command_id("csl-start-7"));
    for bad in [
        "CID nsec1notreally",
        "11111111-1111-4111-8111-11111111111G",
        &"a".repeat(33),
        "nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq",
        "a--b",
    ] {
        assert!(!safe_command_id(bad), "{bad}");
    }
    assert!(
        safe_model("sonnet")
            && safe_model("sonnet[low]")
            && safe_model("opus[1m][high]")
            && safe_model("gpt-6.1-sol[xhigh]")
    );
    for bad in [
        "MODEL ignore prior /Users/secret/path",
        "nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq",
        &"a".repeat(64),
        "sonnet[low",
        "sonnet[]",
        "sonnet[/etc]",
        "",
    ] {
        assert!(!safe_model(bad), "{bad}");
    }
}

#[test]
fn a_hostile_row_keeps_its_class_and_prints_none_of_its_fields() {
    let case = Case::new();
    let mut raw = command(
        &case.keys,
        case.channel,
        &case.provider,
        "66666666-6666-4666-8666-666666666666",
        NOW,
        None,
    );
    let row = case.row(&raw, &CensusState::default(), &[], &[], NOW);
    assert!(row.withheld.is_empty());
    raw["id"] = json!("ID_SENTINEL");
    raw["pubkey"] = json!("PUBKEY_SENTINEL");
    let row = case.row(&raw, &CensusState::default(), &[], &[], NOW);
    assert_eq!(row.class, Class::Invalid);
    assert_eq!(row.withheld, ["eventId", "author"]);
    let text = serde_json::to_string(&row).unwrap();
    assert!(!text.contains("SENTINEL"), "{text}");
}

#[test]
fn an_invalid_row_is_never_clean() {
    let case = Case::new();
    let raw =
        json!({"id":"junk","pubkey":"junk","created_at":NOW,"kind":44221,"tags":[],"content":"x"});
    let row = case.row(&raw, &CensusState::default(), &[], &[], NOW);
    assert_eq!(row.class, Class::Invalid);
    assert_eq!(verdict(&[], &[row]), (EXIT_INCOMPLETE, Some("INVALID_ROW")));
}

#[tokio::test]
async fn the_floor_source_names_where_the_floor_came_from() {
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let now = now_secs();
    struct Members(Uuid);
    impl CensusReader for Members {
        async fn page(
            &self,
            filter: Value,
            cursor: Option<(u64, String)>,
        ) -> Result<Vec<Value>, ()> {
            Ok(if filter["kinds"][0] == MEMBERS_KIND && cursor.is_none() {
                vec![member(self.0)]
            } else {
                Vec::new()
            })
        }
    }
    let route = |floor: u64| {
        json!({"routes":[{"actorPubkey":"ab".repeat(32),"channelId":channel,
            "target":{"driver":"d","instanceId":"i","sessionId":"s","generation":1},
            "replayFloorUnix":floor}]})
        .to_string()
    };
    let source = |inputs: LocalInputs| async move {
        let (channels, _) = survey(&Members(channel), "aa", 86_400, now, &inputs)
            .await
            .unwrap();
        channels[0].floor_source
    };
    let read = |d: &Path| LocalInputs::read(d, None, None).unwrap();
    // A route above the grace floor, no watermark: the grace set it.
    write(dir.path(), "conversation-routes.json", &route(now + 100));
    assert_eq!(source(read(dir.path())).await, Some("startupGrace"));
    // A route below it did.
    write(dir.path(), "conversation-routes.json", &route(now - 5_000));
    assert_eq!(source(read(dir.path())).await, Some("conversationRoute"));
    // A watermark below the route.
    write(
        dir.path(),
        "state.json",
        &json!({"version":1,"watermarks":{channel.to_string():now - 90_000}}).to_string(),
    );
    assert_eq!(source(read(dir.path())).await, Some("watermark"));
}

#[test]
fn the_default_horizon_is_what_a_start_uses_with_no_override() {
    assert_eq!(crate::config::DEFAULT_COMMAND_HORIZON_SECS, 86_400);
}
