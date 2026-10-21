//! Beekeeper's first-party Planning provider.
//!
//! A project's issues are the items of its shared NIP-TD lists (kind 44248).
//! This plugin owns what Planning means over those records — which lists are
//! issue containers, how an item maps onto an issue, what an issue's history
//! says, and which ops a change needs and in what order — and nothing else.
//! It holds no key and signs nothing: a write returns unsigned drafts the
//! caller signs as themselves, so every edit stays the member's own.
//!
//! What is and is not an issue:
//! - every item of a **project**-visibility list that is not archived;
//! - not a **personal** list's items: those are one person's to-dos, which the
//!   relay withholds from everyone else, and a shared board must not show
//!   something only its viewer can see as if it were shared;
//! - not an archived list's items.
//!
//! Not NIP-34 repository issues (kind 1621): Planning neither reads nor
//! converts them.
//!
//! The logic is plain Rust over [`Records`], tested natively; `wasm` is the
//! thin component glue.

use std::collections::{BTreeMap, HashSet};
use std::str::FromStr;

use beekeeper_nip_td::fold::{fold_project_todos, TodoFoldEvent, TodoItem, TodoList};
use beekeeper_nip_td::fractional_rank::rank_between;
use beekeeper_nip_td::project_todo::{
    decode_project_todo_op, validate_todo_description, validate_todo_text, ProjectTodoOp,
    ProjectTodoOpValue, TodoPriority, TodoStatus, TodoVisibility,
    MAX_PROJECT_TODO_DESCRIPTION_BYTES, MAX_PROJECT_TODO_TEXT_BYTES,
};
use beekeeper_nip_td::KIND_PROJECT_TODO_OP;
use serde::Serialize;
use serde_json::{json, Value};

#[cfg(target_arch = "wasm32")]
mod wasm;

/// The registered name. The host refuses the plugin under any other.
pub const NAME: &str = "planning";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The operation API this plugin implements; clients bind to this.
pub const API: &str = "beekeeper.planning/1";
/// The title of the shared list a first issue creates when none is named.
pub const DEFAULT_CONTAINER_TITLE: &str = "Issues";

/// Pages of 1000 ops read before the read says it is truncated.
const PAGE: u32 = 1_000;
const MAX_PAGES: usize = 10;
/// Stay inside the window every checker holds a draft to.
const MAX_FUTURE_SKEW_SECS: u64 = beekeeper_nip_td::DRAFT_FUTURE_SECS;

/// One operation's declaration.
pub struct Operation {
    pub name: &'static str,
    pub draft: bool,
    pub summary: &'static str,
}

pub const OPERATIONS: &[Operation] = &[
    Operation {
        name: "capabilities",
        draft: false,
        summary: "Statuses, priorities and limits this provider supports.",
    },
    Operation {
        name: "read",
        draft: false,
        summary: "The project's issues and containers.",
    },
    Operation {
        name: "activity",
        draft: false,
        summary: "One issue's history, one entry per signed op.",
    },
    Operation {
        name: "create-issue",
        draft: true,
        summary: "Create an issue, and its default list if none is named.",
    },
    Operation {
        name: "set-title",
        draft: true,
        summary: "Rename an issue.",
    },
    Operation {
        name: "set-description",
        draft: true,
        summary: "Set or clear an issue's description.",
    },
    Operation {
        name: "set-status",
        draft: true,
        summary: "Move an issue to a workflow status.",
    },
    Operation {
        name: "set-priority",
        draft: true,
        summary: "Set an issue's priority.",
    },
    Operation {
        name: "set-assignee",
        draft: true,
        summary: "Assign or unassign an issue.",
    },
];

/// What the host supplies to a call.
pub struct Context {
    pub caller: String,
    pub coordinate: String,
    pub now: u64,
    pub entropy: [u8; 32],
}

/// The host's scoped read: newest first, at most `limit`, continuing from a
/// previous page's cursor. Returns the next cursor while older records may
/// exist.
pub trait Records {
    fn page(
        &mut self,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<TodoFoldEvent>, Option<String>), Error>;
}

/// Mirrors the WIT `if-refused`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfRefused {
    Abort,
    SkipGroup,
}

/// Mirrors the WIT `event-draft`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub kind: u32,
    pub created_at: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub group: String,
    pub if_refused: IfRefused,
    pub partial_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub result: Value,
    pub drafts: Vec<Draft>,
}

/// Mirrors the WIT `op-error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidInput(String),
    NotFound(String),
    Conflict(String),
    Unsupported(String),
    /// A host capability refused; the message is the host's.
    Host(String),
    /// A per-call host budget (queries, records, bytes) ran out.
    Budget(String),
    Internal(String),
}

// ---------------------------------------------------------------- vocabulary

struct Status {
    id: TodoStatus,
    label: &'static str,
    category: &'static str,
}

const STATUSES: &[Status] = &[
    Status {
        id: TodoStatus::Triage,
        label: "Triage",
        category: "triage",
    },
    Status {
        id: TodoStatus::Backlog,
        label: "Backlog",
        category: "backlog",
    },
    Status {
        id: TodoStatus::Todo,
        label: "Todo",
        category: "unstarted",
    },
    Status {
        id: TodoStatus::InProgress,
        label: "In Progress",
        category: "started",
    },
    Status {
        id: TodoStatus::Done,
        label: "Done",
        category: "completed",
    },
    Status {
        id: TodoStatus::Canceled,
        label: "Canceled",
        category: "canceled",
    },
];

struct Priority {
    id: TodoPriority,
    label: &'static str,
    level: &'static str,
    order: u8,
}

const PRIORITIES: &[Priority] = &[
    Priority {
        id: TodoPriority::Urgent,
        label: "Urgent",
        level: "urgent",
        order: 0,
    },
    Priority {
        id: TodoPriority::High,
        label: "High",
        level: "high",
        order: 1,
    },
    Priority {
        id: TodoPriority::Medium,
        label: "Medium",
        level: "medium",
        order: 2,
    },
    Priority {
        id: TodoPriority::Low,
        label: "Low",
        level: "low",
        order: 3,
    },
    Priority {
        id: TodoPriority::None,
        label: "No priority",
        level: "none",
        order: 4,
    },
];

fn status_info(id: TodoStatus) -> &'static Status {
    STATUSES.iter().find(|s| s.id == id).unwrap_or(&STATUSES[2])
}

fn priority_info(id: TodoPriority) -> &'static Priority {
    PRIORITIES
        .iter()
        .find(|p| p.id == id)
        .unwrap_or(&PRIORITIES[4])
}

fn status_json(id: TodoStatus) -> Value {
    let s = status_info(id);
    json!({ "id": s.id.as_str(), "label": s.label, "category": s.category })
}

fn priority_json(id: TodoPriority) -> Value {
    let p = priority_info(id);
    json!({ "id": p.id.as_str(), "label": p.label, "level": p.level, "order": p.order })
}

fn capabilities() -> Value {
    json!({
        "statuses": STATUSES.iter().map(|s| status_json(s.id)).collect::<Vec<_>>(),
        "priorities": PRIORITIES.iter().map(|p| priority_json(p.id)).collect::<Vec<_>>(),
        // NIP-TD has no comment op and Beekeeper has no generic comment kind
        // for a project item; the detail says so instead of drawing a box
        // that cannot save.
        "comments": false,
        "createsDefaultContainer": true,
        "defaultContainerTitle": DEFAULT_CONTAINER_TITLE,
        "provider": {
            "label": "Beekeeper",
            "description": "Issues live on this community's relay as the project's shared lists. Every member and agent reads the same records; `bee planning` and `bee todos` operate them headless.",
        },
        "personIds": "beekeeper-pubkey",
        "limits": {
            "titleBytes": MAX_PROJECT_TODO_TEXT_BYTES,
            "descriptionBytes": MAX_PROJECT_TODO_DESCRIPTION_BYTES,
        },
    })
}

// ---------------------------------------------------------------- the read

/// Everything one call learned from the records.
struct Snapshot {
    events: Vec<TodoFoldEvent>,
    lists: Vec<TodoList>,
    truncated: bool,
    /// Ops the fold dropped, minus pre-`td-vis` ops no deployed relay holds.
    unreadable: usize,
    /// Greatest `created_at` per `list/item` and per `list/`.
    latest: BTreeMap<String, u64>,
}

fn tag<'a>(event: &'a TodoFoldEvent, name: &str) -> Option<&'a str> {
    event
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some(name))
        .and_then(|t| t.get(1))
        .map(String::as_str)
}

fn target_key(list_id: &str, item_id: Option<&str>) -> String {
    format!("{list_id}/{}", item_id.unwrap_or(""))
}

fn load(ctx: &Context, records: &mut dyn Records) -> Result<Snapshot, Error> {
    let mut events = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor: Option<String> = None;
    let mut truncated = false;
    for page in 0..MAX_PAGES {
        let (batch, next) = match records.page(cursor.as_deref(), PAGE) {
            Ok(page) => page,
            // The call's budget ran out after some history was read. What
            // was read is still true, and older history exists: say
            // truncated rather than fail the whole read.
            Err(Error::Budget(_)) if page > 0 => {
                truncated = true;
                break;
            }
            Err(error) => return Err(error),
        };
        for event in batch {
            if seen.insert(event.id.clone()) {
                events.push(event);
            }
        }
        match next {
            None => break,
            // Older history exists and this read will not fetch it: say so.
            Some(_) if page + 1 == MAX_PAGES => truncated = true,
            Some(next) => cursor = Some(next),
        }
    }
    let mut legacy = 0usize;
    let mut latest = BTreeMap::new();
    for event in &events {
        if tag(event, "td-vis").is_none() {
            legacy += 1;
        }
        if let Some(list_id) = tag(event, "td-list") {
            for key in [
                target_key(list_id, tag(event, "td-item")),
                target_key(list_id, None),
            ] {
                let slot = latest.entry(key).or_insert(0);
                *slot = (*slot).max(event.created_at);
            }
        }
    }
    let digest = fold_project_todos(&ctx.coordinate, &events);
    Ok(Snapshot {
        unreadable: digest.ignored.saturating_sub(legacy),
        lists: digest.lists,
        events,
        truncated,
        latest,
    })
}

/// `true` for a list whose items Planning shows.
fn is_planning_list(list: &TodoList) -> bool {
    list.visibility == TodoVisibility::Project.as_str() && !list.archived
}

fn issue_json(item: &TodoItem) -> Value {
    let status = TodoStatus::from_str(&item.status).unwrap_or(TodoStatus::Todo);
    let priority = TodoPriority::from_str(&item.priority).unwrap_or(TodoPriority::None);
    json!({
        "key": format!("{}/{}", item.list_id, item.id),
        "containerId": item.list_id,
        "issueId": item.id,
        "title": item.text,
        "description": item.description,
        "status": status_json(status),
        "priority": priority_json(priority),
        "assignee": item.assignee,
        "due": item.due,
        "rank": item.rank,
        "createdAt": item.created_at,
        "createdBy": item.created_by,
        "updatedAt": item.updated_at,
        "completedAt": item.completed_at,
    })
}

fn read(snapshot: &Snapshot) -> Value {
    let mut containers = Vec::new();
    let mut issues = Vec::new();
    for list in snapshot.lists.iter().filter(|l| is_planning_list(l)) {
        containers.push(json!({ "id": list.id, "title": list.title }));
        for item in list.open.iter().chain(&list.completed) {
            issues.push(issue_json(item));
        }
    }
    json!({
        "issues": issues,
        "containers": containers,
        "truncated": snapshot.truncated,
        "unreadable": snapshot.unreadable,
        "people": [],
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityEntry {
    id: String,
    actor: String,
    created_at: u64,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    subject: Option<String>,
}

fn describe(value: &ProjectTodoOpValue) -> Option<(String, Option<String>)> {
    let plain = |s: &str| Some((s.to_owned(), None));
    match value {
        ProjectTodoOpValue::ItemAdd { .. } => plain("created the issue"),
        ProjectTodoOpValue::ItemText { .. } => plain("renamed the issue"),
        ProjectTodoOpValue::ItemDescription { description, .. } => {
            plain(if description.is_none() {
                "cleared the description"
            } else {
                "edited the description"
            })
        }
        ProjectTodoOpValue::ItemStatus { status, .. } => Some((
            format!("set status to {}", status_info(*status).label),
            None,
        )),
        ProjectTodoOpValue::ItemDone { done, .. } => plain(if *done {
            "marked it done"
        } else {
            "marked it not done"
        }),
        ProjectTodoOpValue::ItemPriority { priority, .. } => match priority {
            TodoPriority::None => plain("removed the priority"),
            p => Some((format!("set priority to {}", priority_info(*p).label), None)),
        },
        ProjectTodoOpValue::ItemAssignee { assignee, .. } => match assignee {
            None => plain("unassigned it"),
            Some(pubkey) => Some(("assigned".to_owned(), Some(pubkey.clone()))),
        },
        ProjectTodoOpValue::ItemDue { due, .. } => match due {
            None => plain("cleared the due date"),
            Some(date) => Some((format!("set due {date}"), None)),
        },
        ProjectTodoOpValue::ItemRank { .. } => plain("moved it"),
        _ => None,
    }
}

/// Every decodable op on `(list, item)`, oldest first. An op that lost a
/// same-field race still happened, so it is listed.
fn activity(snapshot: &Snapshot, list_id: &str, item_id: &str) -> Value {
    let mut entries: Vec<(u64, &str, ActivityEntry)> = Vec::new();
    for event in &snapshot.events {
        if tag(event, "td-list") != Some(list_id) || tag(event, "td-item") != Some(item_id) {
            continue;
        }
        // Planning shows project lists only; an op tagged otherwise is one
        // the fold ignores, so the history leaves it out too.
        if tag(event, "td-vis") != Some(TodoVisibility::Project.as_str()) {
            continue;
        }
        let Ok(op) = decode_project_todo_op(&event.content, TodoVisibility::Project) else {
            continue;
        };
        let Some((summary, subject)) = describe(&op.value) else {
            continue;
        };
        entries.push((
            event.created_at,
            event.id.as_str(),
            ActivityEntry {
                id: event.id.clone(),
                actor: event.pubkey.clone(),
                created_at: event.created_at,
                summary,
                subject,
            },
        ));
    }
    entries.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    json!({ "entries": entries.into_iter().map(|e| e.2).collect::<Vec<_>>() })
}

// ---------------------------------------------------------------- writes

/// Builds a call's drafts in order, stamping each one later than everything
/// before it on its target and than the draft before it: the fold orders by
/// `(created_at, id)`, so equal stamps would let an `item.status` lose to the
/// `item.done` it follows, or an `item.add` sort before its list exists.
struct Drafter<'a> {
    ctx: &'a Context,
    snapshot: &'a Snapshot,
    last: u64,
    drafts: Vec<Draft>,
}

impl<'a> Drafter<'a> {
    fn new(ctx: &'a Context, snapshot: &'a Snapshot) -> Self {
        Self {
            ctx,
            snapshot,
            last: 0,
            drafts: Vec::new(),
        }
    }

    fn push(
        &mut self,
        op: ProjectTodoOp,
        group: &str,
        if_refused: IfRefused,
        partial_summary: Option<String>,
    ) -> Result<(), Error> {
        let head = self
            .snapshot
            .latest
            .get(&target_key(&op.list_id, op.item_id()))
            .copied()
            .unwrap_or(0);
        let created_at = self.ctx.now.max(head + 1).max(self.last + 1);
        if created_at > self.ctx.now + MAX_FUTURE_SKEW_SECS {
            return Err(Error::Conflict(format!(
                "The latest change to this item is stamped {}s in the future; try again in a moment.",
                head.saturating_sub(self.ctx.now)
            )));
        }
        self.last = created_at;
        self.drafts.push(Draft {
            kind: KIND_PROJECT_TODO_OP,
            created_at,
            tags: op.tags(&self.ctx.coordinate),
            content: op.to_content(),
            group: group.to_owned(),
            if_refused,
            partial_summary,
        });
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn str_field<'v>(input: &'v Value, key: &str) -> Result<&'v str, Error> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::InvalidInput(format!("{key} must be a string")))
}

fn nullable_str_field<'v>(input: &'v Value, key: &str) -> Result<Option<&'v str>, Error> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(Error::InvalidInput(format!(
            "{key} must be a string or null"
        ))),
    }
}

fn parse_status(id: &str) -> Result<TodoStatus, Error> {
    TodoStatus::from_str(id).map_err(|_| Error::InvalidInput(format!("Unknown status {id}.")))
}

fn parse_priority(id: &str) -> Result<TodoPriority, Error> {
    TodoPriority::from_str(id).map_err(|_| Error::InvalidInput(format!("Unknown priority {id}.")))
}

fn parse_assignee(value: Option<&str>) -> Result<Option<String>, Error> {
    match value {
        None => Ok(None),
        Some(hex) if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Ok(Some(hex.to_ascii_lowercase()))
        }
        Some(_) => Err(Error::InvalidInput(
            "assignee must be a 64-character hex pubkey".to_owned(),
        )),
    }
}

fn planning_list<'s>(snapshot: &'s Snapshot, list_id: &str) -> Result<&'s TodoList, Error> {
    snapshot
        .lists
        .iter()
        .find(|l| l.id == list_id && is_planning_list(l))
        .ok_or_else(|| {
            Error::NotFound(
                "That list is not a shared, unarchived list of this project; refresh and try again."
                    .to_owned(),
            )
        })
}

/// The issue named by `containerId`/`issueId`, which must be in a Planning
/// list as of this read.
fn issue_target(snapshot: &Snapshot, input: &Value) -> Result<(String, String), Error> {
    let list_id = str_field(input, "containerId")?;
    let item_id = str_field(input, "issueId")?;
    let list = planning_list(snapshot, list_id)?;
    if !list
        .open
        .iter()
        .chain(&list.completed)
        .any(|item| item.id == item_id)
    {
        return Err(Error::NotFound(
            "That issue is not in the current read; it may have been removed.".to_owned(),
        ));
    }
    Ok((list_id.to_owned(), item_id.to_owned()))
}

fn item_op(list_id: &str, value: ProjectTodoOpValue) -> ProjectTodoOp {
    ProjectTodoOp {
        list_id: list_id.to_owned(),
        visibility: TodoVisibility::Project,
        value,
    }
}

/// Status is two ops: `item.done` carrying the new status's closed-ness,
/// then `item.status`. Always both — this read can be stale, and done-ness
/// is decided by `item.done` alone by every reader of any age (NIP-TD, fold
/// rule 6), so the pair never splits old and new readers. When the second
/// is refused after the first landed, the summary says exactly what landed.
fn push_status(
    drafter: &mut Drafter<'_>,
    list_id: &str,
    item_id: &str,
    status: TodoStatus,
    group: &str,
    if_refused: IfRefused,
) -> Result<(), Error> {
    drafter.push(
        item_op(
            list_id,
            ProjectTodoOpValue::ItemDone {
                item_id: item_id.to_owned(),
                done: status.is_closed(),
            },
        ),
        group,
        if_refused,
        None,
    )?;
    let reads = if status.is_closed() { "done" } else { "todo" };
    drafter.push(
        item_op(
            list_id,
            ProjectTodoOpValue::ItemStatus {
                item_id: item_id.to_owned(),
                status,
            },
        ),
        group,
        if_refused,
        Some(format!(
            "Only part of the change was confirmed: the item is marked “{reads}”, but the relay did not confirm the status “{}”",
            status.as_str()
        )),
    )
}

/// Create: a list if none is named, one `item.add`, then one op group per
/// further field. Separate signed events, so no all-or-nothing claim: once
/// the `item.add` is accepted the issue exists, every later field is
/// attempted and a refused one is reported — never an error the caller
/// would retry, minting a second issue. The result names the issue's ids up
/// front so a caller keeps them whatever happens to the fields.
fn create_issue(ctx: &Context, snapshot: &Snapshot, input: &Value) -> Result<Output, Error> {
    let title = str_field(input, "title")?;
    validate_todo_text("title", title).map_err(Error::InvalidInput)?;
    let description = nullable_str_field(input, "description")?;
    if let Some(text) = description {
        validate_todo_description(text).map_err(Error::InvalidInput)?;
    }
    let status = parse_status(str_field(input, "statusId")?)?;
    let priority = parse_priority(str_field(input, "priorityId")?)?;
    let assignee = parse_assignee(nullable_str_field(input, "assignee")?)?;
    let container = nullable_str_field(input, "containerId")?;

    let mut drafter = Drafter::new(ctx, snapshot);
    // No container named: the project's first shared list, the same one
    // `read` lists first and a client offers by default. A list is created
    // only when the project has none, so a headless caller filing issue
    // after issue never mints a new "Issues" list per issue.
    let existing = match container {
        Some(id) => Some(planning_list(snapshot, id)?),
        None => snapshot.lists.iter().find(|l| is_planning_list(l)),
    };
    let (list_id, last_rank, created_list) = match existing {
        Some(list) => (
            list.id.clone(),
            list.open.last().map(|item| item.rank.clone()),
            false,
        ),
        None => {
            let id = hex(&ctx.entropy[..16]);
            drafter.push(
                item_op(
                    &id,
                    ProjectTodoOpValue::ListCreate {
                        title: DEFAULT_CONTAINER_TITLE.to_owned(),
                        visibility: TodoVisibility::Project,
                    },
                ),
                "issue",
                IfRefused::Abort,
                None,
            )?;
            (id, None, true)
        }
    };
    let item_id = hex(&ctx.entropy[16..]);
    let rank = rank_between(last_rank.as_deref(), None).map_err(Error::Internal)?;
    drafter.push(
        item_op(
            &list_id,
            ProjectTodoOpValue::ItemAdd {
                item_id: item_id.clone(),
                text: title.to_owned(),
                rank,
            },
        ),
        "issue",
        IfRefused::Abort,
        created_list.then(|| {
            format!(
                "Only part of the change was confirmed: the list “{DEFAULT_CONTAINER_TITLE}” was created, but the relay did not confirm the issue itself"
            )
        }),
    )?;
    if let Some(pubkey) = assignee {
        drafter.push(
            item_op(
                &list_id,
                ProjectTodoOpValue::ItemAssignee {
                    item_id: item_id.clone(),
                    assignee: Some(pubkey),
                },
            ),
            "assignee",
            IfRefused::SkipGroup,
            None,
        )?;
    }
    if status != TodoStatus::Todo {
        push_status(
            &mut drafter,
            &list_id,
            &item_id,
            status,
            "status",
            IfRefused::SkipGroup,
        )?;
    }
    if priority != TodoPriority::None {
        drafter.push(
            item_op(
                &list_id,
                ProjectTodoOpValue::ItemPriority {
                    item_id: item_id.clone(),
                    priority,
                },
            ),
            "priority",
            IfRefused::SkipGroup,
            None,
        )?;
    }
    if let Some(text) = description {
        drafter.push(
            item_op(
                &list_id,
                ProjectTodoOpValue::ItemDescription {
                    item_id: item_id.clone(),
                    description: Some(text.to_owned()),
                },
            ),
            "description",
            IfRefused::SkipGroup,
            None,
        )?;
    }
    Ok(Output {
        result: json!({
            "key": format!("{list_id}/{item_id}"),
            "containerId": list_id,
            "issueId": item_id,
        }),
        drafts: drafter.drafts,
    })
}

fn single(
    ctx: &Context,
    snapshot: &Snapshot,
    list_id: &str,
    value: ProjectTodoOpValue,
    group: &str,
) -> Result<Output, Error> {
    let mut drafter = Drafter::new(ctx, snapshot);
    drafter.push(item_op(list_id, value), group, IfRefused::Abort, None)?;
    Ok(Output {
        result: json!({}),
        drafts: drafter.drafts,
    })
}

/// Run `operation`. Unknown operations are the host's to refuse, so one
/// reaching here is an internal error.
pub fn invoke(
    operation: &str,
    input: &Value,
    ctx: &Context,
    records: &mut dyn Records,
) -> Result<Output, Error> {
    let read_only = |result: Value| {
        Ok(Output {
            result,
            drafts: Vec::new(),
        })
    };
    if operation == "capabilities" {
        return read_only(capabilities());
    }
    let snapshot = load(ctx, records)?;
    match operation {
        "read" => read_only(read(&snapshot)),
        "activity" => {
            let list_id = str_field(input, "containerId")?;
            let item_id = str_field(input, "issueId")?;
            read_only(activity(&snapshot, list_id, item_id))
        }
        "create-issue" => create_issue(ctx, &snapshot, input),
        "set-title" => {
            let (list_id, item_id) = issue_target(&snapshot, input)?;
            let title = str_field(input, "title")?;
            validate_todo_text("title", title).map_err(Error::InvalidInput)?;
            single(
                ctx,
                &snapshot,
                &list_id,
                ProjectTodoOpValue::ItemText {
                    item_id,
                    text: title.to_owned(),
                },
                "title",
            )
        }
        "set-description" => {
            let (list_id, item_id) = issue_target(&snapshot, input)?;
            let description = nullable_str_field(input, "description")?;
            if let Some(text) = description {
                validate_todo_description(text).map_err(Error::InvalidInput)?;
            }
            single(
                ctx,
                &snapshot,
                &list_id,
                ProjectTodoOpValue::ItemDescription {
                    item_id,
                    description: description.map(str::to_owned),
                },
                "description",
            )
        }
        "set-priority" => {
            let (list_id, item_id) = issue_target(&snapshot, input)?;
            let priority = parse_priority(str_field(input, "priorityId")?)?;
            single(
                ctx,
                &snapshot,
                &list_id,
                ProjectTodoOpValue::ItemPriority { item_id, priority },
                "priority",
            )
        }
        "set-assignee" => {
            let (list_id, item_id) = issue_target(&snapshot, input)?;
            let assignee = parse_assignee(nullable_str_field(input, "assignee")?)?;
            single(
                ctx,
                &snapshot,
                &list_id,
                ProjectTodoOpValue::ItemAssignee { item_id, assignee },
                "assignee",
            )
        }
        "set-status" => {
            let (list_id, item_id) = issue_target(&snapshot, input)?;
            let status = parse_status(str_field(input, "statusId")?)?;
            let mut drafter = Drafter::new(ctx, &snapshot);
            push_status(
                &mut drafter,
                &list_id,
                &item_id,
                status,
                "status",
                IfRefused::Abort,
            )?;
            Ok(Output {
                result: json!({}),
                drafts: drafter.drafts,
            })
        }
        other => Err(Error::Internal(format!("no operation {other:?}"))),
    }
}

#[cfg(test)]
mod tests;
