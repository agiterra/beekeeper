//! Native tests of the Planning logic. The component runs this same code;
//! the relay's end-to-end tests run it through the host.

use super::*;
use beekeeper_nip_td::project_todo::validate_project_todo_parts;

const PROJECT: &str = "30621:1111111111111111111111111111111111111111111111111111111111111111:demo";
const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NOW: u64 = 1_800_000_000;
const LIST: &str = "0123456789abcdef0123456789abcdef";
const ITEM: &str = "fedcba9876543210fedcba9876543210";

struct Mem {
    events: Vec<TodoFoldEvent>,
    pages: usize,
}

impl Mem {
    fn new(events: Vec<TodoFoldEvent>) -> Self {
        Self { events, pages: 0 }
    }
}

impl Records for Mem {
    fn page(
        &mut self,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<TodoFoldEvent>, Option<String>), Error> {
        self.pages += 1;
        let mut ordered: Vec<&TodoFoldEvent> = self.events.iter().collect();
        ordered.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
        let skip: usize = cursor.map_or(0, |c| c.parse().unwrap());
        let page: Vec<TodoFoldEvent> = ordered
            .iter()
            .skip(skip)
            .take(limit as usize)
            .map(|e| (*e).clone())
            .collect();
        let end = skip + page.len();
        Ok((page, (end < ordered.len()).then(|| end.to_string())))
    }
}

fn ctx() -> Context {
    let mut entropy = [0u8; 32];
    for (i, b) in entropy.iter_mut().enumerate() {
        *b = i as u8;
    }
    Context {
        caller: ALICE.to_owned(),
        coordinate: PROJECT.to_owned(),
        now: NOW,
        entropy,
    }
}

fn event(n: u64, by: &str, at: u64, op: ProjectTodoOp) -> TodoFoldEvent {
    TodoFoldEvent {
        id: format!("{n:064x}"),
        pubkey: by.to_owned(),
        created_at: at,
        kind: KIND_PROJECT_TODO_OP,
        tags: op.tags(PROJECT),
        content: op.to_content(),
    }
}

fn op(list: &str, vis: TodoVisibility, value: ProjectTodoOpValue) -> ProjectTodoOp {
    ProjectTodoOp {
        list_id: list.to_owned(),
        visibility: vis,
        value,
    }
}

fn list_create(n: u64, at: u64, list: &str, vis: TodoVisibility, title: &str) -> TodoFoldEvent {
    event(
        n,
        ALICE,
        at,
        op(
            list,
            vis,
            ProjectTodoOpValue::ListCreate {
                title: title.to_owned(),
                visibility: vis,
            },
        ),
    )
}

fn item_add(
    n: u64,
    at: u64,
    list: &str,
    vis: TodoVisibility,
    item: &str,
    text: &str,
) -> TodoFoldEvent {
    event(
        n,
        ALICE,
        at,
        op(
            list,
            vis,
            ProjectTodoOpValue::ItemAdd {
                item_id: item.to_owned(),
                text: text.to_owned(),
                rank: "a0".to_owned(),
            },
        ),
    )
}

fn project_value(n: u64, by: &str, at: u64, value: ProjectTodoOpValue) -> TodoFoldEvent {
    event(n, by, at, op(LIST, TodoVisibility::Project, value))
}

fn base() -> Vec<TodoFoldEvent> {
    vec![
        list_create(1, 100, LIST, TodoVisibility::Project, "Issues"),
        item_add(2, 101, LIST, TodoVisibility::Project, ITEM, "Ship it"),
    ]
}

fn run(operation: &str, input: Value, events: Vec<TodoFoldEvent>) -> Result<Output, Error> {
    invoke(operation, &input, &ctx(), &mut Mem::new(events))
}

/// Sign-free stand-in for "the caller published these drafts": each becomes
/// an event by `by`, so a re-read shows whose edit it was.
fn apply(events: &mut Vec<TodoFoldEvent>, drafts: &[Draft], by: &str) {
    for (i, d) in drafts.iter().enumerate() {
        events.push(TodoFoldEvent {
            id: format!("{:064x}", 9_000 + events.len() as u64 + i as u64),
            pubkey: by.to_owned(),
            created_at: d.created_at,
            kind: d.kind,
            tags: d.tags.clone(),
            content: d.content.clone(),
        });
    }
}

fn issues(output: &Output) -> Vec<Value> {
    output.result["issues"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[test]
fn every_operation_is_declared_once() {
    let mut names: Vec<&str> = OPERATIONS.iter().map(|o| o.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), OPERATIONS.len());
    assert_eq!(
        OPERATIONS.iter().filter(|o| o.draft).count(),
        6,
        "six writes"
    );
}

#[test]
fn capabilities_name_the_native_vocabulary_and_limits() {
    let out = run("capabilities", json!({}), vec![]).unwrap();
    assert_eq!(out.result["statuses"].as_array().unwrap().len(), 6);
    assert_eq!(out.result["priorities"][4]["id"], "none");
    assert_eq!(out.result["comments"], false);
    assert_eq!(out.result["limits"]["titleBytes"], 1024);
    assert!(out.drafts.is_empty());
}

#[test]
fn read_maps_project_items_and_leaves_out_personal_and_archived_lists() {
    let personal = "11111111111111111111111111111111";
    let archived = "22222222222222222222222222222222";
    let mut events = base();
    events.push(list_create(
        3,
        102,
        personal,
        TodoVisibility::Personal,
        "Mine",
    ));
    events.push(item_add(
        4,
        103,
        personal,
        TodoVisibility::Personal,
        "33333333333333333333333333333333",
        "private",
    ));
    events.push(list_create(
        5,
        104,
        archived,
        TodoVisibility::Project,
        "Old",
    ));
    events.push(item_add(
        6,
        105,
        archived,
        TodoVisibility::Project,
        "44444444444444444444444444444444",
        "old",
    ));
    events.push(event(
        7,
        ALICE,
        106,
        op(
            archived,
            TodoVisibility::Project,
            ProjectTodoOpValue::ListArchived { archived: true },
        ),
    ));
    let out = run("read", json!({}), events).unwrap();
    let issues = issues(&out);
    assert_eq!(issues.len(), 1, "{issues:#?}");
    assert_eq!(issues[0]["key"], format!("{LIST}/{ITEM}"));
    assert_eq!(issues[0]["title"], "Ship it");
    assert_eq!(issues[0]["status"]["category"], "unstarted");
    assert_eq!(issues[0]["priority"]["level"], "none");
    assert_eq!(issues[0]["createdBy"], ALICE);
    assert_eq!(
        out.result["containers"],
        json!([{ "id": LIST, "title": "Issues" }])
    );
    assert_eq!(out.result["truncated"], false);
    assert_eq!(out.result["unreadable"], 0);
}

#[test]
fn item_done_alone_decides_done_ness() {
    // done=true, then a *newer* status that says the item is open: the status
    // disagrees with the winning item.done, so it is not applied.
    let mut events = base();
    events.push(project_value(
        3,
        ALICE,
        110,
        ProjectTodoOpValue::ItemDone {
            item_id: ITEM.to_owned(),
            done: true,
        },
    ));
    events.push(project_value(
        4,
        BOB,
        111,
        ProjectTodoOpValue::ItemStatus {
            item_id: ITEM.to_owned(),
            status: TodoStatus::InProgress,
        },
    ));
    let out = run("read", json!({}), events.clone()).unwrap();
    assert_eq!(issues(&out)[0]["status"]["id"], "done");
    assert!(issues(&out)[0]["completedAt"].is_u64());

    // A newer status that agrees on closed-ness refines it.
    events.push(project_value(
        5,
        BOB,
        112,
        ProjectTodoOpValue::ItemStatus {
            item_id: ITEM.to_owned(),
            status: TodoStatus::Canceled,
        },
    ));
    let out = run("read", json!({}), events.clone()).unwrap();
    assert_eq!(issues(&out)[0]["status"]["id"], "canceled");

    // A status older than the winning item.done never applies.
    events.push(project_value(
        6,
        ALICE,
        113,
        ProjectTodoOpValue::ItemDone {
            item_id: ITEM.to_owned(),
            done: false,
        },
    ));
    let out = run("read", json!({}), events).unwrap();
    assert_eq!(issues(&out)[0]["status"]["id"], "todo");
    assert!(issues(&out)[0]["completedAt"].is_null());
}

#[test]
fn unreadable_counts_bad_ops_but_not_pre_visibility_ones() {
    let mut events = base();
    let mut bad = project_value(
        3,
        ALICE,
        120,
        ProjectTodoOpValue::ItemText {
            item_id: ITEM.to_owned(),
            text: "x".to_owned(),
        },
    );
    bad.content = "{\"schema\":\"nope\"}".to_owned();
    events.push(bad);
    let mut legacy = project_value(
        4,
        ALICE,
        121,
        ProjectTodoOpValue::ItemText {
            item_id: ITEM.to_owned(),
            text: "y".to_owned(),
        },
    );
    legacy.tags.retain(|t| t[0] != "td-vis");
    events.push(legacy);
    let out = run("read", json!({}), events).unwrap();
    assert_eq!(out.result["unreadable"], 1);
}

fn create_input(container: Value) -> Value {
    json!({
        "title": "Write the plan",
        "description": "Explain the seam.",
        "statusId": "in_progress",
        "priorityId": "high",
        "assignee": BOB,
        "containerId": container,
    })
}

#[test]
fn create_with_no_container_drafts_the_list_then_the_issue_then_each_field() {
    let out = run("create-issue", create_input(Value::Null), vec![]).unwrap();
    let ops: Vec<String> = out
        .drafts
        .iter()
        .map(|d| d.tags.iter().find(|t| t[0] == "td-op").unwrap()[1].clone())
        .collect();
    assert_eq!(
        ops,
        [
            "list.create",
            "item.add",
            "item.assignee",
            "item.done",
            "item.status",
            "item.priority",
            "item.description"
        ]
    );
    let groups: Vec<&str> = out.drafts.iter().map(|d| d.group.as_str()).collect();
    assert_eq!(
        groups,
        [
            "issue",
            "issue",
            "assignee",
            "status",
            "status",
            "priority",
            "description"
        ]
    );
    assert_eq!(out.drafts[0].if_refused, IfRefused::Abort);
    assert_eq!(out.drafts[1].if_refused, IfRefused::Abort);
    assert!(out.drafts[1]
        .partial_summary
        .as_deref()
        .unwrap()
        .contains("did not confirm the issue itself"));
    assert!(out.drafts[2..]
        .iter()
        .all(|d| d.if_refused == IfRefused::SkipGroup));
    // Strictly increasing stamps: the fold orders by (created_at, id).
    assert!(out
        .drafts
        .windows(2)
        .all(|w| w[0].created_at < w[1].created_at));
    assert_eq!(out.drafts[0].created_at, NOW);
    // The ids come from the host's entropy and are named up front.
    let list_id = out.result["containerId"].as_str().unwrap();
    let item_id = out.result["issueId"].as_str().unwrap();
    assert_eq!(list_id, "000102030405060708090a0b0c0d0e0f");
    assert_eq!(item_id, "101112131415161718191a1b1c1d1e1f");
    for draft in &out.drafts {
        validate_project_todo_parts(draft.kind, &draft.tags, &draft.content)
            .expect("every draft passes the relay's own validator");
        assert!(draft
            .tags
            .contains(&vec!["a".to_owned(), PROJECT.to_owned()]));
    }

    // Published as Bob, the issue reads as Bob's, with every field.
    let mut events = Vec::new();
    apply(&mut events, &out.drafts, BOB);
    let read = run("read", json!({}), events).unwrap();
    let issue = &issues(&read)[0];
    assert_eq!(issue["key"], format!("{list_id}/{item_id}"));
    assert_eq!(issue["createdBy"], BOB);
    assert_eq!(issue["status"]["id"], "in_progress");
    assert_eq!(issue["priority"]["id"], "high");
    assert_eq!(issue["assignee"], BOB);
    assert_eq!(issue["description"], "Explain the seam.");
}

#[test]
fn create_into_an_existing_list_ranks_last_and_creates_no_list() {
    let out = run("create-issue", create_input(json!(LIST)), base()).unwrap();
    assert!(out.drafts[0]
        .tags
        .contains(&vec!["td-op".to_owned(), "item.add".to_owned()]));
    assert!(out.drafts[0].partial_summary.is_none());
    let content: Value = serde_json::from_str(&out.drafts[0].content).unwrap();
    assert!(content["rank"].as_str().unwrap() > "a0");
    assert_eq!(out.result["containerId"], LIST);
}

#[test]
fn create_with_defaults_is_one_add() {
    let out = run(
        "create-issue",
        json!({ "title": "T", "description": null, "statusId": "todo",
                "priorityId": "none", "assignee": null, "containerId": LIST }),
        base(),
    )
    .unwrap();
    assert_eq!(out.drafts.len(), 1);
}

#[test]
fn create_refuses_bad_input_before_drafting() {
    let mut input = create_input(json!(LIST));
    input["statusId"] = json!("started");
    assert!(matches!(
        run("create-issue", input, base()),
        Err(Error::InvalidInput(m)) if m.contains("Unknown status")
    ));
    let mut input = create_input(json!(LIST));
    input["title"] = json!("x".repeat(MAX_PROJECT_TODO_TEXT_BYTES + 1));
    assert!(matches!(
        run("create-issue", input, base()),
        Err(Error::InvalidInput(_))
    ));
    let input = create_input(json!("99999999999999999999999999999999"));
    assert!(matches!(
        run("create-issue", input, base()),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn set_status_is_done_then_status_with_an_exact_partial_summary() {
    let out = run(
        "set-status",
        json!({ "containerId": LIST, "issueId": ITEM, "statusId": "canceled" }),
        base(),
    )
    .unwrap();
    assert_eq!(out.drafts.len(), 2);
    let done: Value = serde_json::from_str(&out.drafts[0].content).unwrap();
    assert_eq!(done["op"], "item.done");
    assert_eq!(done["done"], true);
    let status: Value = serde_json::from_str(&out.drafts[1].content).unwrap();
    assert_eq!(status["status"], "canceled");
    assert!(out.drafts[0].created_at < out.drafts[1].created_at);
    assert!(out.drafts.iter().all(|d| d.if_refused == IfRefused::Abort));
    assert_eq!(out.drafts[0].partial_summary, None);
    assert_eq!(
        out.drafts[1].partial_summary.as_deref(),
        Some("Only part of the change was confirmed: the item is marked “done”, but the relay did not confirm the status “canceled”")
    );

    let reopen = run(
        "set-status",
        json!({ "containerId": LIST, "issueId": ITEM, "statusId": "in_progress" }),
        base(),
    )
    .unwrap();
    assert!(reopen.drafts[1]
        .partial_summary
        .as_deref()
        .unwrap()
        .contains("marked “todo”"));
}

#[test]
fn a_write_stamps_after_the_items_latest_op() {
    let mut events = base();
    events.push(project_value(
        3,
        BOB,
        NOW + 30,
        ProjectTodoOpValue::ItemText {
            item_id: ITEM.to_owned(),
            text: "skewed".to_owned(),
        },
    ));
    let out = run(
        "set-priority",
        json!({ "containerId": LIST, "issueId": ITEM, "priorityId": "low" }),
        events,
    )
    .unwrap();
    assert_eq!(out.drafts[0].created_at, NOW + 31);
}

#[test]
fn a_write_refuses_when_the_item_is_stamped_too_far_ahead() {
    let mut events = base();
    events.push(project_value(
        3,
        BOB,
        NOW + 2_000,
        ProjectTodoOpValue::ItemText {
            item_id: ITEM.to_owned(),
            text: "far".to_owned(),
        },
    ));
    assert!(matches!(
        run(
            "set-title",
            json!({ "containerId": LIST, "issueId": ITEM, "title": "x" }),
            events
        ),
        Err(Error::Conflict(m)) if m.contains("in the future")
    ));
}

#[test]
fn writes_refuse_issues_outside_planning() {
    let personal = "11111111111111111111111111111111";
    let item = "33333333333333333333333333333333";
    let mut events = base();
    events.push(list_create(
        3,
        102,
        personal,
        TodoVisibility::Personal,
        "Mine",
    ));
    events.push(item_add(
        4,
        103,
        personal,
        TodoVisibility::Personal,
        item,
        "p",
    ));
    assert!(matches!(
        run(
            "set-status",
            json!({ "containerId": personal, "issueId": item, "statusId": "done" }),
            events.clone()
        ),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        run(
            "set-title",
            json!({ "containerId": LIST, "issueId": item, "title": "x" }),
            events
        ),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn set_assignee_and_description_accept_null() {
    let out = run(
        "set-assignee",
        json!({ "containerId": LIST, "issueId": ITEM, "assignee": null }),
        base(),
    )
    .unwrap();
    let content: Value = serde_json::from_str(&out.drafts[0].content).unwrap();
    assert!(content["assignee"].is_null());
    let out = run(
        "set-description",
        json!({ "containerId": LIST, "issueId": ITEM, "description": null }),
        base(),
    )
    .unwrap();
    let content: Value = serde_json::from_str(&out.drafts[0].content).unwrap();
    assert!(content["description"].is_null());
}

#[test]
fn activity_lists_each_op_on_the_issue_oldest_first() {
    let mut events = base();
    events.push(project_value(
        3,
        BOB,
        130,
        ProjectTodoOpValue::ItemStatus {
            item_id: ITEM.to_owned(),
            status: TodoStatus::InProgress,
        },
    ));
    events.push(project_value(
        4,
        BOB,
        131,
        ProjectTodoOpValue::ItemAssignee {
            item_id: ITEM.to_owned(),
            assignee: Some(ALICE.to_owned()),
        },
    ));
    let out = run(
        "activity",
        json!({ "containerId": LIST, "issueId": ITEM }),
        events,
    )
    .unwrap();
    let entries = out.result["entries"].as_array().unwrap();
    let summaries: Vec<&str> = entries
        .iter()
        .map(|e| e["summary"].as_str().unwrap())
        .collect();
    assert_eq!(
        summaries,
        ["created the issue", "set status to In Progress", "assigned"]
    );
    assert_eq!(entries[2]["subject"], ALICE);
    assert_eq!(entries[1]["actor"], BOB);
}

/// A host that hides rows returns short pages, even empty ones, with a
/// cursor; the read must follow the cursor, not stop at a short page.
struct Sparse {
    events: Vec<TodoFoldEvent>,
}

impl Records for Sparse {
    fn page(
        &mut self,
        cursor: Option<&str>,
        _limit: u32,
    ) -> Result<(Vec<TodoFoldEvent>, Option<String>), Error> {
        // Pages "0" and "1" are all hidden rows; the last holds the issue.
        match cursor {
            None => Ok((vec![], Some("1".into()))),
            Some("1") => Ok((vec![], Some("2".into()))),
            _ => Ok((self.events.clone(), None)),
        }
    }
}

#[test]
fn short_and_empty_pages_with_a_cursor_are_followed_to_the_end() {
    let out = invoke("read", &json!({}), &ctx(), &mut Sparse { events: base() }).unwrap();
    assert_eq!(issues(&out).len(), 1);
    assert_eq!(out.result["truncated"], false);
}

#[test]
fn a_long_history_is_paged_and_a_very_long_one_says_truncated() {
    let mut events = base();
    for n in 0..1_500u64 {
        events.push(project_value(
            10 + n,
            ALICE,
            200 + n,
            ProjectTodoOpValue::ItemRank {
                item_id: ITEM.to_owned(),
                rank: "a1".to_owned(),
            },
        ));
    }
    let mut mem = Mem::new(events.clone());
    let out = invoke("read", &json!({}), &ctx(), &mut mem).unwrap();
    assert_eq!(mem.pages, 2);
    assert_eq!(out.result["truncated"], false);
    assert_eq!(issues(&out).len(), 1);

    for n in 0..9_000u64 {
        events.push(project_value(
            20_000 + n,
            ALICE,
            5_000 + n,
            ProjectTodoOpValue::ItemRank {
                item_id: ITEM.to_owned(),
                rank: "a1".to_owned(),
            },
        ));
    }
    let mut mem = Mem::new(events);
    let out = invoke("read", &json!({}), &ctx(), &mut mem).unwrap();
    assert_eq!(mem.pages, MAX_PAGES);
    assert_eq!(out.result["truncated"], true);
}

/// The desktop's view tests draw with the capabilities this plugin really
/// returns, through a checked-in copy. This keeps the copy honest; set
/// `UPDATE_PLANNING_FIXTURE=1` to rewrite it after a deliberate change.
#[test]
fn capabilities_match_the_desktop_test_fixture() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../desktop/src/features/planning/lib/planningCapabilities.fixture.json");
    let actual = serde_json::to_string_pretty(&capabilities()).unwrap() + "\n";
    if std::env::var_os("UPDATE_PLANNING_FIXTURE").is_some() {
        std::fs::write(&path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path).expect("fixture exists");
    assert_eq!(expected, actual, "rerun with UPDATE_PLANNING_FIXTURE=1");
}
