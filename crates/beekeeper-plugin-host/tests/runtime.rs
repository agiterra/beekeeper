//! The host's load checks and per-call containment, against real components.
//!
//! Tests that need the built plugins are `#[ignore]`d; `just test-plugins`
//! builds them (`scripts/build-plugins.sh`) and runs these with `--ignored`.
//! The rest need nothing but this crate.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use beekeeper_plugin_host::{
    sha256_hex, Access, DraftKind, DraftReferences, DraftValidators, Invocation, InvokeError,
    LoadError, OpError, PluginHost, RecordSource, Registry, Scope, SourceError, StoredRecord,
    TrapKind,
};
use serde_json::{json, Value};

const PROJECT: &str = "30621:1111111111111111111111111111111111111111111111111111111111111111:demo";
const CALLER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const KIND: u32 = 44248;

fn artifacts() -> PathBuf {
    let dir = std::env::var_os("BEEKEEPER_PLUGIN_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugins"));
    assert!(
        dir.join("planning.wasm").exists(),
        "no built plugins in {} — run scripts/build-plugins.sh",
        dir.display()
    );
    dir
}

fn validators() -> DraftValidators {
    DraftValidators::new().with(
        KIND,
        DraftKind::new(
            &beekeeper_nip_td::project_todo::PROJECT_TODO_DATA_TAG_KEYS,
            Arc::new(|tags, content| {
                let op = beekeeper_nip_td::project_todo::validate_project_todo_parts(
                    KIND, tags, content,
                )?;
                Ok(DraftReferences {
                    people: op.person().map(str::to_owned).into_iter().collect(),
                })
            }),
        ),
    )
}

/// A registry with one entry for `file`, pinned to its real digest.
fn registry_for(file: &Path, name: &str, extra: &str) -> Registry {
    let sha = sha256_hex(&std::fs::read(file).expect("read plugin"));
    let text = format!(
        "[[plugin]]\nname = \"{name}\"\nworld = \"operation-provider@1\"\npath = \"{}\"\nsha256 = \"{sha}\"\n{extra}",
        file.display()
    );
    Registry::from_toml(&text, Path::new("/")).expect("registry")
}

const GRANTS: &str = "[plugin.grants]\nread_kinds = [44248]\ndraft_kinds = [44248]\n";

fn faults(limits: &str) -> PluginHost {
    let file = artifacts().join("test-faults.wasm");
    PluginHost::load(
        &registry_for(&file, "test-faults", &format!("{GRANTS}{limits}")),
        validators(),
    )
    .expect("faults plugin loads")
}

/// One `query` as the source saw it: kinds, until, limit.
type QueryCall = (Vec<u32>, Option<u64>, u32);

#[derive(Default)]
struct Source {
    records: Vec<StoredRecord>,
    calls: Mutex<Vec<QueryCall>>,
    delay: Option<Duration>,
}

impl RecordSource for Source {
    fn query(
        &self,
        kinds: &[u32],
        until: Option<u64>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<StoredRecord>, Option<String>), SourceError> {
        self.calls
            .lock()
            .expect("lock")
            .push((kinds.to_vec(), until, limit));
        if let Some(delay) = self.delay {
            std::thread::sleep(delay);
        }
        let mut out: Vec<StoredRecord> = self
            .records
            .iter()
            .filter(|r| kinds.contains(&r.kind))
            .filter(|r| until.is_none_or(|u| r.created_at <= u))
            .cloned()
            .collect();
        out.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
        let skip: usize = cursor.and_then(|c| c.parse().ok()).unwrap_or(0);
        let page: Vec<StoredRecord> = out
            .iter()
            .skip(skip)
            .take(limit as usize)
            .cloned()
            .collect();
        let end = skip + page.len();
        Ok((page, (end < out.len()).then(|| end.to_string())))
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

fn call(operation: &str, input: Value, access: Access) -> Invocation {
    Invocation {
        operation: operation.to_owned(),
        input,
        caller: CALLER.to_owned(),
        scope: Scope::Project(PROJECT.to_owned()),
        access,
        now: now(),
        entropy: [7; 32],
    }
}

fn run(host: &PluginHost, plugin: &str, operation: &str) -> Result<Value, InvokeError> {
    host.invoke(
        plugin,
        call(operation, json!({}), Access::Write),
        Arc::new(Source::default()),
    )
    .map(|outcome| outcome.result)
}

fn plugin_error(error: LoadError) -> String {
    match error {
        LoadError::Plugin { message, .. } => message,
        other => other.to_string(),
    }
}

// ------------------------------------------------------------ no plugins needed

#[test]
fn a_digest_mismatch_refuses_the_plugin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("p.wasm");
    std::fs::write(&file, b"\0asm whatever").expect("write");
    let text = format!(
        "[[plugin]]\nname = \"p\"\nworld = \"operation-provider@1\"\npath = \"p.wasm\"\nsha256 = \"{}\"\n",
        "0".repeat(64)
    );
    let registry = Registry::from_toml(&text, dir.path()).expect("registry");
    let error = plugin_error(
        PluginHost::load(&registry, validators())
            .err()
            .expect("refused"),
    );
    assert!(error.contains("but the registry pins"), "{error}");
}

#[test]
fn a_file_that_is_not_a_component_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("p.wasm");
    std::fs::write(&file, b"definitely not wasm").expect("write");
    let error = plugin_error(
        PluginHost::load(&registry_for(&file, "p", ""), validators())
            .err()
            .expect("refused"),
    );
    assert!(error.contains("not a loadable component"), "{error}");
}

#[test]
fn a_component_that_imports_wasi_is_refused_before_linking() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("wasi.wasm");
    let bytes = wat::parse_str(
        r#"(component
             (import "wasi:cli/environment@0.2.0" (instance))
           )"#,
    )
    .expect("wat");
    std::fs::write(&file, bytes).expect("write");
    let error = plugin_error(
        PluginHost::load(&registry_for(&file, "p", ""), validators())
            .err()
            .expect("refused"),
    );
    assert!(error.contains("no WASI"), "{error}");
    assert!(error.contains("wasi:cli/environment"), "{error}");
}

#[test]
fn a_component_without_the_operations_export_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("empty.wasm");
    std::fs::write(&file, wat::parse_str("(component)").expect("wat")).expect("write");
    let error = plugin_error(
        PluginHost::load(&registry_for(&file, "p", ""), validators())
            .err()
            .expect("refused"),
    );
    assert!(error.contains("beekeeper:plugin/operations"), "{error}");
}

#[test]
fn an_empty_registry_loads_and_names_nothing() {
    let host = PluginHost::empty().expect("empty host");
    assert_eq!(host.plugins().count(), 0);
    assert!(matches!(
        run(&host, "planning", "read"),
        Err(InvokeError::NotRegistered(_))
    ));
}

// ------------------------------------------------------------ load checks

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn a_plugin_registered_under_another_name_is_refused() {
    let file = artifacts().join("test-faults.wasm");
    let error = plugin_error(
        PluginHost::load(&registry_for(&file, "faults", GRANTS), validators())
            .err()
            .expect("refused"),
    );
    assert!(error.contains("registered as \"faults\""), "{error}");
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn a_plugin_asking_for_more_than_its_grants_is_refused() {
    let file = artifacts().join("test-faults.wasm");
    let error = plugin_error(
        PluginHost::load(
            &registry_for(
                &file,
                "test-faults",
                "[plugin.grants]\nread_kinds = [44248]\ndraft_kinds = []\n",
            ),
            validators(),
        )
        .err()
        .expect("refused"),
    );
    assert!(error.contains("asks to draft kind 44248"), "{error}");
    let error = plugin_error(
        PluginHost::load(
            &registry_for(&file, "test-faults", GRANTS),
            DraftValidators::new(),
        )
        .err()
        .expect("refused"),
    );
    assert!(error.contains("no validator"), "{error}");
}

// ------------------------------------------------------------ containment

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn running_out_of_fuel_traps_and_the_next_call_is_clean() {
    let host = faults("[plugin.limits]\nfuel = 50000000\n");
    match run(&host, "test-faults", "spin") {
        Err(InvokeError::Trapped {
            kind: TrapKind::Fuel,
            ..
        }) => {}
        other => panic!("expected a fuel trap, got {other:?}"),
    }
    let echo = run(&host, "test-faults", "echo").expect("next call runs");
    assert_eq!(echo["caller"], CALLER);
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn the_deadline_stops_a_call_that_still_has_fuel() {
    let host = faults("[plugin.limits]\nfuel = 50000000000\ndeadline_ms = 50\n");
    let started = Instant::now();
    match run(&host, "test-faults", "spin") {
        Err(InvokeError::Trapped {
            kind: TrapKind::Deadline,
            ..
        }) => {}
        other => panic!("expected a deadline trap, got {other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn growing_past_the_memory_limit_traps_as_memory() {
    let host = faults("[plugin.limits]\nmemory_bytes = 16777216\n");
    match run(&host, "test-faults", "grow") {
        Err(InvokeError::Trapped {
            kind: TrapKind::Memory,
            ..
        }) => {}
        other => panic!("expected a memory trap, got {other:?}"),
    }
    run(&host, "test-faults", "echo").expect("next call runs");
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn a_panic_is_a_trap_not_a_crash() {
    let host = faults("");
    match run(&host, "test-faults", "panic") {
        Err(InvokeError::Trapped {
            kind: TrapKind::Trap,
            ..
        }) => {}
        other => panic!("expected a trap, got {other:?}"),
    }
    run(&host, "test-faults", "echo").expect("next call runs");
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn reads_are_limited_to_granted_kinds_and_the_query_budget() {
    let host = faults("[plugin.limits]\nmax_queries = 3\n");
    let denied = run(&host, "test-faults", "read-ungranted-kind").expect("runs");
    assert_eq!(denied["denied"], true);
    let flood = run(&host, "test-faults", "query-flood").expect("runs");
    assert_eq!(flood, json!({ "served": 3, "stopped": "budget" }));
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn the_plugin_sees_only_what_the_source_serves_and_the_hosts_identity() {
    let host = faults("");
    let mine = StoredRecord {
        id: "1".repeat(64),
        pubkey: CALLER.to_owned(),
        created_at: 10,
        kind: KIND,
        tags: vec![vec!["a".to_owned(), PROJECT.to_owned()]],
        content: "{}".to_owned(),
    };
    let source = Arc::new(Source {
        records: vec![mine],
        ..Source::default()
    });
    let outcome = host
        .invoke(
            "test-faults",
            call("echo", json!({ "hello": 1 }), Access::Read),
            source.clone(),
        )
        .expect("echo");
    assert_eq!(outcome.result["records"], 1);
    assert_eq!(outcome.result["coordinates"], json!([PROJECT]));
    assert_eq!(outcome.result["caller"], CALLER);
    assert_eq!(outcome.result["scope"], PROJECT);
    assert_eq!(outcome.result["access"], false);
    assert_eq!(outcome.result["input"], json!({ "hello": 1 }));
    assert_eq!(outcome.result["entropyBytes"], 32);
    assert_eq!(
        source.calls.lock().expect("lock").clone(),
        vec![(vec![KIND], None, 1000)]
    );
    assert!(outcome.logs.iter().any(|(_, m)| m == "echo ran"));
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn a_write_needs_write_access_and_never_runs_without_it() {
    let host = faults("");
    let source = Arc::new(Source::default());
    let error = host
        .invoke(
            "test-faults",
            call("draft-ok", json!({}), Access::Read),
            source.clone(),
        )
        .expect_err("refused");
    assert!(
        matches!(error, InvokeError::WriteNotAllowed(_)),
        "{error:?}"
    );
    let ok = host
        .invoke(
            "test-faults",
            call("draft-ok", json!({}), Access::Write),
            source,
        )
        .expect("drafts");
    assert_eq!(ok.drafts.len(), 1);
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn output_outside_the_contract_is_discarded_whole() {
    let host = faults("");
    for (operation, needle) in [
        ("drafts-from-a-read", "returned drafts"),
        (
            "draft-ungranted-kind",
            "kind 1 is not one this plugin drafts",
        ),
        ("draft-other-scope", "proposes scope"),
        ("draft-invalid-op", "proposes a \"h\" tag"),
        ("draft-far-future", "outside"),
        ("not-json", "not JSON"),
        ("huge-result", "the limit is"),
    ] {
        match run(&host, "test-faults", operation) {
            Err(InvokeError::ContractViolation(message)) => {
                assert!(message.contains(needle), "{operation}: {message}")
            }
            other => panic!("{operation}: expected a contract violation, got {other:?}"),
        }
    }
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn input_over_the_limit_and_unknown_operations_are_refused_before_running() {
    let host = faults("[plugin.limits]\nmax_input_bytes = 16\n");
    let error = host
        .invoke(
            "test-faults",
            call(
                "echo",
                json!({ "pad": "xxxxxxxxxxxxxxxxxxxx" }),
                Access::Read,
            ),
            Arc::new(Source::default()),
        )
        .expect_err("refused");
    assert!(
        matches!(error, InvokeError::InputTooLarge { .. }),
        "{error:?}"
    );
    assert!(matches!(
        run(&host, "test-faults", "no-such-op"),
        Err(InvokeError::UnknownOperation { .. })
    ));
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn calls_past_the_concurrency_limit_are_refused_as_busy() {
    let host = Arc::new(faults("[plugin.limits]\nmax_concurrent = 1\n"));
    let slow = Arc::new(Source {
        delay: Some(Duration::from_millis(600)),
        ..Source::default()
    });
    let first = {
        let host = host.clone();
        let slow = slow.clone();
        std::thread::spawn(move || {
            host.invoke("test-faults", call("echo", json!({}), Access::Read), slow)
        })
    };
    std::thread::sleep(Duration::from_millis(150));
    let second = host.invoke(
        "test-faults",
        call("echo", json!({}), Access::Read),
        Arc::new(Source::default()),
    );
    assert!(matches!(second, Err(InvokeError::Busy(_))), "{second:?}");
    first.join().expect("thread").expect("first call finishes");
    run(&host, "test-faults", "echo").expect("permit released");
}

// ------------------------------------------------------------ the real plugins

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn the_planning_plugin_drafts_through_the_host_and_reads_its_own_drafts_back() {
    let dir = artifacts();
    let host = PluginHost::load(
        &Registry::from_file(&dir.join("registry.toml")).expect("first-party registry"),
        validators(),
    )
    .expect("loads");
    let planning = host.get("planning").expect("planning");
    assert_eq!(planning.manifest.api, vec!["beekeeper.planning/1"]);

    let created = host
        .invoke(
            "planning",
            call(
                "create-issue",
                json!({ "title": "Through the host", "description": null, "statusId": "done",
                        "priorityId": "urgent", "assignee": null, "containerId": null }),
                Access::Write,
            ),
            Arc::new(Source::default()),
        )
        .expect("create");
    assert_eq!(created.drafts.len(), 5, "list, add, done, status, priority");
    let records: Vec<StoredRecord> = created
        .drafts
        .iter()
        .enumerate()
        .map(|(i, d)| StoredRecord {
            id: format!("{i:064x}"),
            pubkey: CALLER.to_owned(),
            created_at: d.created_at,
            kind: d.kind,
            tags: d.tags.clone(),
            content: d.content.clone(),
        })
        .collect();
    let read = host
        .invoke(
            "planning",
            call("read", json!({}), Access::Read),
            Arc::new(Source {
                records,
                ..Source::default()
            }),
        )
        .expect("read");
    let issue = &read.result["issues"][0];
    assert_eq!(issue["title"], "Through the host");
    assert_eq!(issue["status"]["id"], "done");
    assert_eq!(issue["priority"]["id"], "urgent");
    assert_eq!(issue["createdBy"], CALLER);
    assert_eq!(issue["key"], created.result["key"]);
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn the_fixture_provider_loads_through_the_same_registry_and_refuses_writes_honestly() {
    let dir = artifacts();
    let host = PluginHost::load(
        &Registry::from_file(&dir.join("registry.test.toml")).expect("test registry"),
        validators(),
    )
    .expect("loads");
    let fixture = host.get("planning-fixture").expect("fixture");
    assert_eq!(fixture.manifest.api, vec!["beekeeper.planning/1"]);
    let read = run(&host, "planning-fixture", "read").expect("read");
    assert_eq!(read["issues"][0]["title"], "Served by the fixture provider");
    match run(&host, "planning-fixture", "set-title") {
        Err(InvokeError::Operation(OpError::Unsupported(message))) => {
            assert!(message.contains("read-only"), "{message}")
        }
        other => panic!("expected unsupported, got {other:?}"),
    }
}

// ------------------------------------------------- what a draft may say

const ELSEWHERE: &str =
    "30621:2222222222222222222222222222222222222222222222222222222222222222:elsewhere";
const LIST: &str = "abcdefabcdefabcdefabcdefabcdefab";
const MEMBER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn tags(raw: &[&[&str]]) -> Vec<Vec<String>> {
    raw.iter()
        .map(|t| t.iter().map(|v| (*v).to_owned()).collect())
        .collect()
}

/// A valid `list.create`'s data tags and content: everything but the scope.
fn list_create_data() -> (Vec<Vec<String>>, String) {
    let content = json!({ "schema": "buzz-project-todo/v1", "op": "list.create",
        "listId": LIST, "title": "Told", "visibility": "project" })
    .to_string();
    (
        tags(&[
            &["td-v", "td1-1"],
            &["td-op", "list.create"],
            &["td-list", LIST],
            &["td-vis", "project"],
        ]),
        content,
    )
}

/// An `item.assignee` naming `assignee`.
fn assign_data(assignee: &str) -> (Vec<Vec<String>>, String) {
    let item = "1234567890abcdef1234567890abcdef";
    let content = json!({ "schema": "buzz-project-todo/v1", "op": "item.assignee",
        "listId": LIST, "itemId": item, "assignee": assignee })
    .to_string();
    (
        tags(&[
            &["td-v", "td1-1"],
            &["td-op", "item.assignee"],
            &["td-list", LIST],
            &["td-vis", "project"],
            &["td-item", item],
        ]),
        content,
    )
}

/// Ask the fault plugin to propose exactly these drafts.
fn told(
    host: &PluginHost,
    drafts: &[(Vec<Vec<String>>, String)],
) -> Result<beekeeper_plugin_host::InvokeOutcome, InvokeError> {
    let drafts: Vec<Value> = drafts
        .iter()
        .map(|(tags, content)| json!({ "kind": KIND, "tags": tags, "content": content }))
        .collect();
    host.invoke(
        "test-faults",
        call("draft-as-told", json!({ "drafts": drafts }), Access::Write),
        Arc::new(Source::default()),
    )
}

fn violation(result: Result<beekeeper_plugin_host::InvokeOutcome, InvokeError>) -> String {
    match result {
        Err(InvokeError::ContractViolation(message)) => message,
        other => panic!("expected a contract violation, got {other:?}"),
    }
}

fn scopes(draft: &beekeeper_plugin_host::Draft) -> Vec<&Vec<String>> {
    draft.tags.iter().filter(|t| t[0] == "a").collect()
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn the_host_writes_the_scope_from_the_invocation_whether_or_not_the_plugin_proposed_it() {
    let host = faults("");
    let (data, content) = list_create_data();
    let mut echoed = data.clone();
    echoed.push(vec!["a".to_owned(), PROJECT.to_owned()]);
    for proposed in [data.clone(), echoed] {
        let outcome = told(&host, &[(proposed, content.clone())]).expect("sealed");
        let draft = &outcome.drafts[0];
        assert_eq!(
            scopes(draft),
            vec![&vec!["a".to_owned(), PROJECT.to_owned()]]
        );
        assert_eq!(
            draft.tags[0],
            vec!["a".to_owned(), PROJECT.to_owned()],
            "scope first"
        );
        assert_eq!(&draft.tags[1..], data.as_slice(), "data tags kept in order");
        assert!(outcome.people.is_empty());
    }
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn a_proposed_scope_naming_anything_else_or_twice_is_refused_whole() {
    let host = faults("");
    let (data, content) = list_create_data();
    for extra in [
        tags(&[&["a", ELSEWHERE]]),
        tags(&[&["a", PROJECT], &["a", ELSEWHERE]]),
        tags(&[&["a", ELSEWHERE], &["a", PROJECT]]),
        tags(&[&["a", PROJECT], &["a", PROJECT]]),
        tags(&[&["a", PROJECT, "wss://elsewhere.example"]]),
    ] {
        let mut proposed = data.clone();
        proposed.extend(extra.clone());
        let message = violation(told(&host, &[(proposed, content.clone())]));
        assert!(message.contains("scope"), "{extra:?}: {message}");
    }
    // One bad draft among good ones discards the whole output.
    let mut bad = data.clone();
    bad.extend(tags(&[&["a", ELSEWHERE]]));
    let message = violation(told(
        &host,
        &[(data.clone(), content.clone()), (bad, content)],
    ));
    assert!(message.starts_with("draft 1:"), "{message}");
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn routing_and_unknown_tags_are_refused_by_the_kinds_allowlist() {
    let host = faults("");
    let (data, content) = list_create_data();
    for extra in [
        tags(&[&["p", MEMBER]]),
        tags(&[&["e", &"e".repeat(64)]]),
        tags(&[&["h", "a-channel"]]),
        tags(&[&["q", &"f".repeat(64)]]),
        tags(&[&["k", "1"]]),
        tags(&[&["community", "elsewhere"]]),
        tags(&[&["client", "the-plugin"]]),
        tags(&[&["td-extra", "x"]]),
    ] {
        let mut proposed = data.clone();
        proposed.extend(extra.clone());
        let message = violation(told(&host, &[(proposed, content.clone())]));
        assert!(message.contains("may carry only"), "{extra:?}: {message}");
    }
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn caps_hold_on_the_proposal_and_after_the_host_adds_the_scope() {
    let host = faults("");
    let (_, content) = list_create_data();
    let filler: Vec<Vec<String>> = (0..64)
        .map(|_| tags(&[&["td-v", "td1-1"]])[0].clone())
        .collect();
    let message = violation(told(&host, &[(filler.clone(), content.clone())]));
    assert!(message.contains("constructed tags: 65 tags"), "{message}");
    let mut over = filler;
    over.push(tags(&[&["td-v", "td1-1"]])[0].clone());
    let message = violation(told(&host, &[(over, content.clone())]));
    assert!(message.contains("proposed tags: 65 tags"), "{message}");
    let huge = "x".repeat(2_000);
    let message = violation(told(&host, &[(tags(&[&["td-list", &huge]]), content)]));
    assert!(
        message.contains("proposed tags: a value is 2000 bytes"),
        "{message}"
    );
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn tag_and_content_must_agree_after_sealing() {
    let host = faults("");
    let (data, content) = list_create_data();
    // A list id in the tag that the content does not name.
    let mut forged = data.clone();
    forged[2][1] = "fedcbafedcbafedcbafedcbafedcbafe".to_owned();
    let message = violation(told(&host, &[(forged, content.clone())]));
    assert!(message.contains("td-list"), "{message}");
    // A personal op dressed as a project one.
    let mut vis = data;
    vis[3][1] = "personal".to_owned();
    let message = violation(told(&host, &[(vis, content)]));
    assert!(
        message.contains("visibility") || message.contains("td-vis"),
        "{message}"
    );
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn the_people_a_draft_names_are_reported_for_the_embedder_to_authorize() {
    let host = faults("");
    let outcome = told(&host, &[assign_data(MEMBER), list_create_data()]).expect("assign");
    assert_eq!(outcome.people, vec![MEMBER.to_owned()]);
    let message = violation(told(&host, &[assign_data("not-a-pubkey")]));
    assert!(message.contains("assignee"), "{message}");
}

#[test]
#[ignore = "requires built plugins: just test-plugins"]
fn planning_creates_edits_and_assigns_through_the_sealed_contract() {
    let dir = artifacts();
    let host = PluginHost::load(
        &Registry::from_file(&dir.join("registry.toml")).expect("first-party registry"),
        validators(),
    )
    .expect("loads");
    let created = host
        .invoke(
            "planning",
            call(
                "create-issue",
                json!({ "title": "Assigned on create", "description": "d", "statusId": "todo",
                        "priorityId": "none", "assignee": MEMBER, "containerId": null }),
                Access::Write,
            ),
            Arc::new(Source::default()),
        )
        .expect("create");
    assert_eq!(created.people, vec![MEMBER.to_owned()]);
    for draft in &created.drafts {
        assert_eq!(
            scopes(draft),
            vec![&vec!["a".to_owned(), PROJECT.to_owned()]]
        );
    }
    let records: Vec<StoredRecord> = created
        .drafts
        .iter()
        .enumerate()
        .map(|(i, d)| StoredRecord {
            id: format!("{i:064x}"),
            pubkey: CALLER.to_owned(),
            created_at: d.created_at,
            kind: d.kind,
            tags: d.tags.clone(),
            content: d.content.clone(),
        })
        .collect();
    let key = created.result["key"].as_str().expect("key").to_owned();
    let (container, issue) = key.split_once('/').expect("list/item");
    for (operation, extra, people) in [
        ("set-title", json!({ "title": "Retitled" }), vec![]),
        (
            "set-assignee",
            json!({ "assignee": CALLER }),
            vec![CALLER.to_owned()],
        ),
        ("set-assignee", json!({ "assignee": null }), vec![]),
    ] {
        let mut input = json!({ "containerId": container, "issueId": issue });
        for (k, v) in extra.as_object().expect("object") {
            input[k] = v.clone();
        }
        let edited = host
            .invoke(
                "planning",
                call(operation, input, Access::Write),
                Arc::new(Source {
                    records: records.clone(),
                    ..Source::default()
                }),
            )
            .unwrap_or_else(|e| panic!("{operation}: {e:?}"));
        assert!(!edited.drafts.is_empty(), "{operation}");
        assert_eq!(edited.people, people, "{operation}");
        for draft in &edited.drafts {
            assert_eq!(
                scopes(draft),
                vec![&vec!["a".to_owned(), PROJECT.to_owned()]]
            );
        }
    }
}
