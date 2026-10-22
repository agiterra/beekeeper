//! The `webhook-plugin@1` world: per-world load checks, the import
//! allowlist, the type-prefix grant, the host's MAC check, the concurrency
//! slots, and the sealed application event, against real components.
//!
//! Tests that need the built fixture (`plugins/test-webhook`) are
//! `#[ignore]`d; `just test-plugins` builds it and runs them with
//! `--include-ignored`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use beekeeper_plugin_host::webhook::{
    AuthFailure, Header, VerifiedDelivery, VerifyError, WebhookOutcome,
    APPLICATION_EVENT_NAMESPACE, KIND_APPLICATION_EVENT,
};
use beekeeper_plugin_host::{
    sha256_hex, Access, Delivery, DraftKind, DraftReferences, DraftValidators, HostConfig,
    Invocation, InvokeError, LoadError, PluginHost, RecordSource, Registry, Scope, SourceError,
    StoredRecord, TrapKind, WebhookCall, RESERVED_TAG_KEYS,
};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

const CHANNEL: &str = "6f1c2b9e-3d4a-4f5b-8c7d-9e0f1a2b3c4d";
const NOW: u64 = 1_800_000_000;

fn artifacts() -> PathBuf {
    let dir = std::env::var_os("BEEKEEPER_PLUGIN_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugins"));
    assert!(
        dir.join("test-webhook.wasm").exists(),
        "no built webhook fixture in {} — run scripts/build-plugins.sh",
        dir.display()
    );
    dir
}

/// Ingest's application-event rules, as a test stand-in for the relay's
/// validator: one `h`, one `L` with the namespace, one `l` with a type and
/// the namespace, content `{schema >= 1, data: {..}}`.
fn application_event(data_tags: &[&str]) -> DraftKind {
    DraftKind::new(
        data_tags,
        Arc::new(|tags, content| {
            let count = |key: &str| tags.iter().filter(|t| t[0] == key).count();
            if count("h") != 1 || count("L") != 1 || count("l") != 1 {
                return Err("requires exactly one h, L and l tag".to_owned());
            }
            let l = tags.iter().find(|t| t[0] == "l").expect("l");
            if l.len() != 3 || l[2] != APPLICATION_EVENT_NAMESPACE {
                return Err("bad type label".to_owned());
            }
            let value: serde_json::Value =
                serde_json::from_str(content).map_err(|e| e.to_string())?;
            if value["schema"].as_u64().is_none_or(|s| s == 0) || !value["data"].is_object() {
                return Err("bad content".to_owned());
            }
            Ok(DraftReferences::default())
        }),
    )
}

fn config() -> HostConfig {
    HostConfig {
        validators: DraftValidators::new(),
        application_event: Some(application_event(&[])),
    }
}

fn webhook_registry(file: &Path, name: &str, extra: &str) -> Registry {
    let sha = sha256_hex(&std::fs::read(file).expect("read plugin"));
    let text = format!(
        "[[plugin]]\nname = \"{name}\"\nworld = \"webhook-plugin@1\"\npath = \"{}\"\nsha256 = \"{sha}\"\n{extra}",
        file.display()
    );
    Registry::from_toml(&text, Path::new("/")).expect("registry")
}

const GRANT: &str = "[plugin.grants]\ntype_prefix = \"com.example\"\n";

fn plugin_error(error: LoadError) -> String {
    match error {
        LoadError::Plugin { message, .. } => message,
        other => panic!("expected a plugin error, got {other}"),
    }
}

fn wat_file(dir: &Path, wat: &str) -> PathBuf {
    let file = dir.join("c.wasm");
    std::fs::write(&file, wat::parse_str(wat).expect("wat")).expect("write");
    file
}

// ------------------------------------------------------------ no fixture needed

#[test]
fn a_webhook_plugin_may_not_import_records() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = wat_file(
        dir.path(),
        r#"(component (import "beekeeper:plugin/records@1.0.0" (instance)))"#,
    );
    let error = plugin_error(
        PluginHost::load_with(&webhook_registry(&file, "p", GRANT), config())
            .err()
            .expect("refused"),
    );
    assert!(error.contains("beekeeper:plugin/records"), "{error}");
    assert!(
        error.contains("webhook-plugin@1 plugin may import only"),
        "{error}"
    );

    // The allowlist is per world: the same import passes an
    // operation-provider's check and the component fails later, on its
    // missing export.
    let text = format!(
        "[[plugin]]\nname = \"p\"\nworld = \"operation-provider@1\"\npath = \"{}\"\nsha256 = \"{}\"\n",
        file.display(),
        sha256_hex(&std::fs::read(&file).expect("read"))
    );
    let registry = Registry::from_toml(&text, Path::new("/")).expect("registry");
    let error = plugin_error(
        PluginHost::load(&registry, DraftValidators::new())
            .err()
            .expect("refused"),
    );
    assert!(!error.contains("may import only"), "{error}");
    assert!(error.contains("beekeeper:plugin/operations"), "{error}");
}

#[test]
fn a_webhook_plugin_may_import_log_and_must_export_the_webhook_interface() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = wat_file(
        dir.path(),
        r#"(component (import "beekeeper:plugin/log@1.0.0" (instance)))"#,
    );
    let error = plugin_error(
        PluginHost::load_with(&webhook_registry(&file, "p", GRANT), config())
            .err()
            .expect("refused"),
    );
    assert!(!error.contains("may import only"), "{error}");
    assert!(error.contains("beekeeper:plugin/webhook"), "{error}");
}

#[test]
fn a_webhook_plugin_needs_an_application_event_validator() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = wat_file(dir.path(), "(component)");
    let error = plugin_error(
        PluginHost::load(&webhook_registry(&file, "p", GRANT), DraftValidators::new())
            .err()
            .expect("refused"),
    );
    assert!(error.contains("application-event validator"), "{error}");
}

#[test]
fn an_allowlist_naming_a_reserved_tag_refuses_the_host() {
    let ok = |_: &[Vec<String>], _: &str| Ok(DraftReferences::default());
    // Planning's allowlist names no reserved key: the host loads.
    PluginHost::load(
        &Registry::default(),
        DraftValidators::new().with(
            44248,
            DraftKind::new(
                &beekeeper_nip_td::project_todo::PROJECT_TODO_DATA_TAG_KEYS,
                Arc::new(ok),
            ),
        ),
    )
    .expect("planning's allowlist is clean");
    PluginHost::load_with(&Registry::default(), config()).expect("an empty event allowlist");

    // Every reserved key, `a` (the project scope key, which `DraftKind`
    // drops from the seal's allowlist) included.
    assert!(RESERVED_TAG_KEYS.contains(&"a"));
    for key in RESERVED_TAG_KEYS.iter().copied() {
        let error = PluginHost::load(
            &Registry::default(),
            DraftValidators::new().with(44248, DraftKind::new(&["td-op", key], Arc::new(ok))),
        )
        .err()
        .expect("refused");
        assert!(error.to_string().contains("reserved"), "{key}: {error}");
        let error = PluginHost::load_with(
            &Registry::default(),
            HostConfig {
                validators: DraftValidators::new(),
                application_event: Some(application_event(&["r", key])),
            },
        )
        .err()
        .expect("refused");
        assert!(error.to_string().contains("kind 50000"), "{key}: {error}");
    }
}

// ------------------------------------------------------------ the fixture

fn fixture(extra: &str) -> PluginHost {
    let file = artifacts().join("test-webhook.wasm");
    PluginHost::load_with(
        &webhook_registry(&file, "test-webhook", &format!("{GRANT}{extra}")),
        config(),
    )
    .expect("webhook fixture loads")
}

fn headers(pairs: &[(&str, &str)]) -> Vec<Header> {
    pairs
        .iter()
        .map(|(name, value)| Header {
            name: (*name).to_owned(),
            value: value.as_bytes().to_vec(),
        })
        .collect()
}

/// The installation's webhook secret: the embedder's, never the plugin's.
const SECRET: &[u8] = b"installation secret";

fn mac(secret: &[u8], body: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret).expect("any key size");
    mac.update(body);
    mac.finalize().into_bytes().to_vec()
}

/// The fixture's headers for `event`, signed over `body` with [`SECRET`].
fn signed(event: &str, body: &[u8]) -> Vec<Header> {
    headers(&[
        ("x-signature", &hex::encode(mac(SECRET, body))),
        ("x-delivery", "72d3162e-cc78-11e3-81ab-4c9367dc0958"),
        ("x-event", event),
    ])
}

/// The fixture's headers for `push` with `x-timestamp: claimed`, MAC'd over
/// `v0:<signed>:` and `body` — the vendor's real signature when `claimed` is
/// `signed`, a header rewritten in transit when it is not.
fn stamped(claimed: u64, signed: u64, body: &[u8]) -> Vec<Header> {
    let message = [format!("v0:{signed}:").as_bytes(), body].concat();
    let mut h = self::signed("push", b"");
    h[0].value = hex::encode(mac(SECRET, &message)).into_bytes();
    h.extend(headers(&[("x-timestamp", &claimed.to_string())]));
    h
}

fn spinning() -> Vec<Header> {
    let mut headers = signed("push", b"");
    headers.extend(self::headers(&[("x-fault", "spin")]));
    headers
}

/// Authenticate and verify `body` as a correctly signed `event`.
fn verified(call: &WebhookCall<'_>, event: &str, body: &[u8]) -> VerifiedDelivery {
    let plan = call
        .authenticate(&signed(event, body))
        .expect("ran")
        .expect("plan");
    call.verify(plan, SECRET, body.to_vec(), NOW)
        .expect("verified")
}

fn delivery(settings: &str) -> Delivery<'_> {
    Delivery {
        channel: CHANNEL,
        settings_json: settings,
        headers: &[],
    }
}

fn project(
    call: &WebhookCall<'_>,
    event: &str,
    settings: &str,
    body: &[u8],
) -> Result<WebhookOutcome, InvokeError> {
    call.project(&verified(call, event, body), delivery(settings))
}

/// Limits that keep a spinning call in its slot for its whole deadline
/// (fuel at the ceiling), so a test has time to observe the slot held.
const SLOW: &str = "fuel = 50000000000\ndeadline_ms = 1500\n";

/// Wait until `n` plugin calls hold their slots.
fn wait_for_running(host: &PluginHost, n: usize) {
    let start = Instant::now();
    while host.running_calls() < n {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "only {} of {n} calls started",
            host.running_calls()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn the_type_prefix_is_the_operators_grant() {
    let file = artifacts().join("test-webhook.wasm");
    let error = plugin_error(
        PluginHost::load_with(
            &webhook_registry(
                &file,
                "test-webhook",
                "[plugin.grants]\ntype_prefix = \"com.github\"\n",
            ),
            config(),
        )
        .err()
        .expect("refused"),
    );
    assert!(
        error.contains("requests event type prefix \"com.example\""),
        "{error}"
    );
    let host = fixture("");
    let loaded = host.webhook_plugin("test-webhook").expect("loaded");
    assert_eq!(loaded.manifest.type_prefix, "com.example");
    // A webhook plugin is not an operation provider: nothing to list or invoke.
    assert_eq!(host.plugins().count(), 0);
    assert!(host.get("test-webhook").is_none());
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn a_webhook_plugin_registered_under_another_name_is_refused() {
    let file = artifacts().join("test-webhook.wasm");
    let error = plugin_error(
        PluginHost::load_with(&webhook_registry(&file, "hook", GRANT), config())
            .err()
            .expect("refused"),
    );
    assert!(error.contains("registered as \"hook\""), "{error}");
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn the_body_cap_is_the_smaller_of_registry_and_manifest() {
    let host = fixture("");
    let call = host.webhook_call("test-webhook", "i").expect("call");
    assert_eq!(call.max_body_bytes(), 65_536, "manifest asks for less");
    let host = fixture("[plugin.limits]\nmax_body_bytes = 1000\n");
    let call = host.webhook_call("test-webhook", "i").expect("call");
    assert_eq!(call.max_body_bytes(), 1000);
    let long = [b'x'; 1001];
    let plan = call
        .authenticate(&signed("push", &long))
        .expect("ran")
        .expect("plan");
    assert_eq!(
        call.verify(plan, SECRET, long.to_vec(), NOW).unwrap_err(),
        VerifyError::BodyTooLarge {
            size: 1001,
            limit: 1000
        }
    );
    assert!(project(&call, "push", "{}", &[b'x'; 1000]).is_ok());
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn one_installation_cannot_take_every_slot() {
    let host = fixture(&format!(
        "[plugin.limits]\n{SLOW}max_concurrent = 4\nmax_concurrent_per_installation = 1\n"
    ));
    std::thread::scope(|scope| {
        let spinner = scope.spawn(|| {
            host.webhook_call("test-webhook", "inst-a")
                .expect("call")
                .authenticate(&spinning())
        });
        wait_for_running(&host, 1);
        let a = host.webhook_call("test-webhook", "inst-a").expect("call");
        assert!(matches!(
            a.authenticate(&signed("push", b"")),
            Err(InvokeError::Busy(_))
        ));
        let b = host.webhook_call("test-webhook", "inst-b").expect("call");
        assert!(
            b.authenticate(&signed("push", b"")).expect("ran").is_ok(),
            "another installation still runs"
        );
        let spun = spinner.join().expect("thread");
        assert!(matches!(spun, Err(InvokeError::Trapped { .. })), "{spun:?}");
    });
    assert_eq!(host.running_calls(), 0);
    let a = host.webhook_call("test-webhook", "inst-a").expect("call");
    assert!(
        a.authenticate(&signed("push", b"")).expect("ran").is_ok(),
        "slot freed when the call returned"
    );
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn the_relay_wide_cap_holds_across_installations_and_plugins() {
    // Default relay cap is far above 4: the plugin's own cap binds.
    let host = fixture(&format!("[plugin.limits]\n{SLOW}max_concurrent = 4\n"));
    std::thread::scope(|scope| {
        let spinners: Vec<_> = (0..4)
            .map(|i| {
                let host = &host;
                scope.spawn(move || {
                    host.webhook_call("test-webhook", &format!("i{i}"))
                        .expect("call")
                        .authenticate(&spinning())
                })
            })
            .collect();
        wait_for_running(&host, 4);
        let ninth = host.webhook_call("test-webhook", "i9").expect("call");
        assert!(matches!(
            ninth.authenticate(&signed("push", b"")),
            Err(InvokeError::Busy(_))
        ));
        for spinner in spinners {
            assert!(spinner.join().expect("thread").is_err());
        }
    });

    let host = fixture(&format!(
        "[plugin.limits]\n{SLOW}max_concurrent = 4\n[host]\nmax_concurrent_calls = 2\n"
    ));
    std::thread::scope(|scope| {
        let spinners: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|installation| {
                let host = &host;
                scope.spawn(move || {
                    host.webhook_call("test-webhook", installation)
                        .expect("call")
                        .authenticate(&spinning())
                })
            })
            .collect();
        wait_for_running(&host, 2);
        let c = host.webhook_call("test-webhook", "c").expect("call");
        assert!(matches!(
            c.authenticate(&signed("push", b"")),
            Err(InvokeError::Busy(_))
        ));
        assert_eq!(host.running_calls(), 2);
        for spinner in spinners {
            assert!(spinner.join().expect("thread").is_err());
        }
    });
    let c = host.webhook_call("test-webhook", "c").expect("call");
    assert!(
        c.authenticate(&signed("push", b"")).expect("ran").is_ok(),
        "relay slot freed"
    );
}

/// A source with nothing in it.
struct Empty;

impl RecordSource for Empty {
    fn query(
        &self,
        _: &[u32],
        _: Option<u64>,
        _: Option<&str>,
        _: u32,
    ) -> Result<(Vec<StoredRecord>, Option<String>), SourceError> {
        Ok((Vec::new(), None))
    }
}

/// The one admission outcome Planning did not have before the webhook world:
/// with every relay-wide slot held by a webhook call, a Planning call is
/// `Busy`, though Planning's own `max_concurrent` has room.
#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn webhook_calls_holding_the_relay_wide_cap_make_planning_busy() {
    let dir = artifacts();
    let pinned = |name: &str, world: &str| {
        let file = dir.join(format!("{name}.wasm"));
        format!(
            "[[plugin]]\nname = \"{name}\"\nworld = \"{world}\"\npath = \"{}\"\nsha256 = \"{}\"\n",
            file.display(),
            sha256_hex(&std::fs::read(&file).expect("read plugin"))
        )
    };
    let text = format!(
        "{}[plugin.grants]\nread_kinds = [44248]\ndraft_kinds = [44248]\n{}{GRANT}[plugin.limits]\n{SLOW}max_concurrent_per_installation = 1\n[host]\nmax_concurrent_calls = 1\n",
        pinned("planning", "operation-provider@1"),
        pinned("test-webhook", "webhook-plugin@1"),
    );
    let ok = |_: &[Vec<String>], _: &str| Ok(DraftReferences::default());
    let host = PluginHost::load_with(
        &Registry::from_toml(&text, Path::new("/")).expect("registry"),
        HostConfig {
            validators: DraftValidators::new().with(
                44248,
                DraftKind::new(
                    &beekeeper_nip_td::project_todo::PROJECT_TODO_DATA_TAG_KEYS,
                    Arc::new(ok),
                ),
            ),
            ..config()
        },
    )
    .expect("loads");
    let read = || {
        host.invoke(
            "planning",
            Invocation {
                operation: "read".to_owned(),
                input: serde_json::json!({}),
                caller: "a".repeat(64),
                scope: Scope::Project(format!("30621:{}:demo", "1".repeat(64))),
                access: Access::Read,
                now: NOW,
                entropy: [7; 32],
            },
            Arc::new(Empty),
        )
    };
    read().expect("planning runs while the relay-wide slot is free");
    std::thread::scope(|scope| {
        let spinner = scope.spawn(|| {
            host.webhook_call("test-webhook", "i")
                .expect("call")
                .authenticate(&spinning())
        });
        wait_for_running(&host, 1);
        assert!(matches!(read(), Err(InvokeError::Busy(ref p)) if p == "planning"));
        assert!(spinner.join().expect("thread").is_err());
    });
    read().expect("the slot is freed");
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn no_slot_is_held_between_authenticate_and_project() {
    // One slot in all: relay-wide, for the plugin and for the installation.
    let host = fixture(
        "[plugin.limits]\nmax_concurrent = 1\nmax_concurrent_per_installation = 1\n[host]\nmax_concurrent_calls = 1\n",
    );
    let call = host.webhook_call("test-webhook", "inst-a").expect("call");
    let plan = call
        .authenticate(&signed("push", b"first"))
        .expect("ran")
        .expect("plan");
    assert_eq!(host.running_calls(), 0, "authenticate returned its slots");

    // While the first delivery's body is read and verified, a second one to
    // the same installation runs end to end through the only slot.
    let other = host.webhook_call("test-webhook", "inst-a").expect("call");
    assert!(matches!(
        project(&other, "push", "{}", b"second"),
        Ok(WebhookOutcome::Accept(_))
    ));

    let verified = call
        .verify(plan, SECRET, b"first".to_vec(), NOW)
        .expect("verified");
    assert_eq!(host.running_calls(), 0, "verifying takes no slot");
    let WebhookOutcome::Accept(event) = call.project(&verified, delivery("{}")).expect("ok") else {
        panic!("expected accept");
    };
    let content: serde_json::Value = serde_json::from_str(&event.content).expect("json");
    assert_eq!(content["data"]["body"], "first");
    assert_eq!(host.running_calls(), 0);
}

/// A busy `project` leaves the caller its delivery: the retry projects the
/// same verified body, and the event keeps the receive time it was verified
/// at, so it is the same event a first-time projection would have built.
#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn a_busy_project_is_retried_with_the_same_delivery_and_receive_time() {
    let host = fixture(&format!(
        "[plugin.limits]\n{SLOW}max_concurrent = 4\nmax_concurrent_per_installation = 1\n"
    ));
    let call = host.webhook_call("test-webhook", "inst-a").expect("call");
    let held = verified(&call, "push", b"retry me");
    std::thread::scope(|scope| {
        let spinner = scope.spawn(|| {
            host.webhook_call("test-webhook", "inst-a")
                .expect("call")
                .authenticate(&spinning())
        });
        wait_for_running(&host, 1);
        let busy = call.project(&held, delivery("{}"));
        assert!(matches!(busy, Err(InvokeError::Busy(_))), "{busy:?}");
        assert!(spinner.join().expect("thread").is_err());
    });
    assert_eq!(host.running_calls(), 0);

    let WebhookOutcome::Accept(event) =
        call.project(&held, delivery("{}")).expect("the retry runs")
    else {
        panic!("expected accept");
    };
    assert_eq!(held.received_at(), NOW);
    assert_eq!(event.created_at, NOW, "the original receive time");
    let content: serde_json::Value = serde_json::from_str(&event.content).expect("json");
    assert_eq!(content["data"]["body"], "retry me");
    // Projecting it once more builds the identical event (same id once the
    // relay signs it): at most once is the B1 store's job, not this call's.
    let again = call.project(&held, delivery("{}")).expect("again");
    assert_eq!(again, WebhookOutcome::Accept(event));
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn authenticate_reads_headers_and_the_host_checks_the_plan() {
    let host = fixture("");
    let call = host.webhook_call("test-webhook", "i").expect("call");
    let plan = call
        .authenticate(&signed("push", b"hi"))
        .expect("ran")
        .expect("plan");
    assert_eq!(plan.signature, mac(SECRET, b"hi"));
    assert_eq!(plan.vendor_event, "push");
    assert_eq!(plan.timestamp_span, None);
    assert!(plan.signed_prefix.is_empty());

    let missing = call
        .authenticate(&headers(&[("x-delivery", "d"), ("x-event", "push")]))
        .expect("ran");
    assert!(
        matches!(missing, Err(AuthFailure::MissingHeader(_))),
        "{missing:?}"
    );

    // A signature that is not exactly 32 bytes never reaches the MAC.
    for signature in ["00ff10", &hex::encode(&mac(SECRET, b"hi")[..31])] {
        let mut short = signed("push", b"hi");
        short[0].value = signature.as_bytes().to_vec();
        let result = call.authenticate(&short).expect("ran");
        assert!(
            matches!(result, Err(AuthFailure::Malformed(ref m)) if m.contains("exactly 32 bytes")),
            "{result:?}"
        );
    }

    // The vendor timestamp's skew is checked by verify, against the receive
    // time the verified delivery carries.
    let plan_at = |at: u64| {
        call.authenticate(&stamped(at, at, b"hi"))
            .expect("ran")
            .expect("plan")
    };
    let result = call.verify(plan_at(NOW - 301), SECRET, b"hi".to_vec(), NOW);
    assert!(
        matches!(result, Err(VerifyError::Malformed(ref m)) if m.contains("timestamp")),
        "{result:?}"
    );
    assert!(call
        .verify(plan_at(NOW - 300), SECRET, b"hi".to_vec(), NOW)
        .is_ok());

    let error = call.authenticate(&spinning()).unwrap_err();
    assert!(
        matches!(
            error,
            InvokeError::Trapped {
                kind: TrapKind::Fuel | TrapKind::Deadline,
                ..
            }
        ),
        "{error:?}"
    );
    // The next call on a fresh instance is clean.
    assert!(call
        .authenticate(&signed("push", b""))
        .expect("ran")
        .is_ok());
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn the_vendor_timestamp_is_read_from_the_signed_prefix() {
    let host = fixture("");
    let call = host.webhook_call("test-webhook", "i").expect("call");
    let verify = |h: Vec<Header>| {
        let plan = call.authenticate(&h).expect("ran").expect("plan");
        call.verify(plan, SECRET, b"hi".to_vec(), NOW)
    };

    let honest = verify(stamped(NOW - 10, NOW - 10, b"hi")).expect("fresh and signed");
    assert_eq!(honest.vendor_timestamp(), Some(NOW - 10));
    // A fresh time in the header over a stale signed one: the prefix the
    // host MACs is built from the claim, so the vendor's MAC does not match.
    assert_eq!(
        verify(stamped(NOW, NOW - 1_000, b"hi")).unwrap_err(),
        VerifyError::BadSignature
    );
    // The stale signed time, honestly reported, is past the skew.
    let stale = verify(stamped(NOW - 1_000, NOW - 1_000, b"hi")).unwrap_err();
    assert!(
        matches!(stale, VerifyError::Malformed(ref m) if m.contains(&(NOW - 1_000).to_string())),
        "{stale:?}"
    );

    for (fault, needle) in [("span-past-end", "outside"), ("span-not-digits", "digits")] {
        let mut h = stamped(NOW, NOW, b"hi");
        h.extend(headers(&[("x-fault", fault)]));
        let result = call.authenticate(&h).expect("ran");
        assert!(
            matches!(result, Err(AuthFailure::Malformed(ref m)) if m.contains(needle)),
            "{fault}: {result:?}"
        );
    }
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn the_host_verifies_the_mac_before_project_sees_the_body() {
    let host = fixture("");
    let call = host.webhook_call("test-webhook", "i").expect("call");
    let plan = || {
        call.authenticate(&signed("push", b"hello"))
            .expect("ran")
            .expect("plan")
    };
    let good = call
        .verify(plan(), SECRET, b"hello".to_vec(), NOW)
        .expect("the right secret passes");
    assert_eq!(good.vendor_event(), "push");
    assert_eq!(good.delivery_id(), "72d3162e-cc78-11e3-81ab-4c9367dc0958");
    assert_eq!(good.received_at(), NOW);

    assert_eq!(
        call.verify(plan(), b"wrong secret", b"hello".to_vec(), NOW)
            .unwrap_err(),
        VerifyError::BadSignature
    );
    assert_eq!(
        call.verify(plan(), SECRET, b"hellp".to_vec(), NOW)
            .unwrap_err(),
        VerifyError::BadSignature,
        "one flipped body byte"
    );
    let mut truncated = plan();
    truncated.signature.truncate(31);
    assert!(matches!(
        call.verify(truncated, SECRET, b"hello".to_vec(), NOW),
        Err(VerifyError::Malformed(_))
    ));
    assert!(matches!(
        call.verify(plan(), SECRET, b"hello".to_vec(), 0),
        Err(VerifyError::Malformed(ref m)) if m.contains("receive time is 0")
    ));

    // The event's type and time are the verified delivery's; the embedder
    // has no field to pass them in.
    let WebhookOutcome::Accept(event) = call.project(&good, delivery("{}")).expect("ok") else {
        panic!("expected accept");
    };
    assert_eq!(event.created_at, NOW);

    // A delivery verified for one installation cannot be projected into another.
    let elsewhere = host.webhook_call("test-webhook", "j").expect("call");
    let error = elsewhere
        .project(&verified(&call, "push", b"hello"), delivery("{}"))
        .unwrap_err();
    assert!(
        matches!(error, InvokeError::Host(ref m) if m.contains("another plugin or installation")),
        "{error:?}"
    );
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn an_accepted_event_is_built_by_the_host() {
    let host = fixture("");
    let call = host.webhook_call("test-webhook", "i").expect("call");
    let WebhookOutcome::Accept(event) = project(&call, "push", "{}", b"hi").expect("ok") else {
        panic!("expected accept");
    };
    assert_eq!(event.kind, KIND_APPLICATION_EVENT);
    assert_eq!(event.created_at, NOW, "the receive time, not the plugin's");
    assert_eq!(
        event.tags,
        vec![
            vec!["h".to_owned(), CHANNEL.to_owned()],
            vec!["L".to_owned(), APPLICATION_EVENT_NAMESPACE.to_owned()],
            vec![
                "l".to_owned(),
                "com.example.push".to_owned(),
                APPLICATION_EVENT_NAMESPACE.to_owned()
            ],
        ]
    );
    let content: serde_json::Value = serde_json::from_str(&event.content).expect("json");
    assert_eq!(content["schema"], 1);
    assert_eq!(content["data"]["body"], "hi");

    // Echoing the installation's own channel once is allowed and changes nothing.
    let settings = format!("{{\"channel\":\"{CHANNEL}\"}}");
    let WebhookOutcome::Accept(echoed) =
        project(&call, "echo-channel", &settings, b"hi").expect("ok")
    else {
        panic!("expected accept");
    };
    assert_eq!(echoed.tags, event.tags);

    assert_eq!(
        project(&call, "ping", "{}", b"").expect("ok"),
        WebhookOutcome::Ignore("ping".to_owned())
    );
    assert!(matches!(
        project(&call, "bad", "{}", b"").expect("ok"),
        WebhookOutcome::Reject(_)
    ));
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn a_plugin_cannot_route_forge_provenance_or_leave_its_prefix() {
    let host = fixture("");
    let call = host.webhook_call("test-webhook", "i").expect("call");
    for (event, expect) in [
        ("forge-h", "scope"),
        ("forge-p", "\"p\""),
        ("forge-l", "\"l\""),
        ("wrong-prefix", "prefix"),
        ("not-object", "object"),
    ] {
        match project(&call, event, "{}", b"") {
            Err(InvokeError::ContractViolation(message)) => {
                assert!(message.contains(expect), "{event}: {message}")
            }
            other => panic!("{event}: expected a contract violation, got {other:?}"),
        }
    }
    // The installation's channel echoed against another channel is refused too.
    let settings = "{\"channel\":\"00000000-0000-4000-8000-000000000000\"}";
    assert!(matches!(
        project(&call, "echo-channel", settings, b""),
        Err(InvokeError::ContractViolation(_))
    ));
    let error = project(&call, "spin", "{}", b"").unwrap_err();
    assert!(matches!(error, InvokeError::Trapped { .. }), "{error:?}");
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn settings_are_validated_by_host_then_plugin() {
    let host = fixture("");
    assert_eq!(
        host.validate_webhook_settings("test-webhook", "{}")
            .expect("ran"),
        Ok(())
    );
    assert!(host
        .validate_webhook_settings("test-webhook", "{\"bad\":true}")
        .expect("ran")
        .is_err());
    assert!(host
        .validate_webhook_settings("test-webhook", "[]")
        .expect("ran")
        .is_err());
    assert!(matches!(
        host.validate_webhook_settings("nope", "{}"),
        Err(InvokeError::NotRegistered(_))
    ));
}
