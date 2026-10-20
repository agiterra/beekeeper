//! Project to-do operations (kind 44248): one field-level edit to a shared,
//! project-scoped to-do list.
//!
//! A to-do list is not an event. It is what a reader gets by folding every
//! op that names it — `list.create` first, then the latest `(created_at,
//! id)` write per field — and that fold lives in
//! [`crate::fold`], pinned by `conformance/project-todo-fold/`.
//! This module owns the **wire contract**: the op vocabulary, the closed
//! content key set, the tag grammar, and the one validator the relay, the
//! SDK builder and `bee todos` all call.
//!
//! Every op sets exactly one thing. That is the whole concurrency story:
//! two people editing different fields of one item never race, and two
//! editing the same field resolve by timestamp then event id, the same
//! order every client sees. There is no "update item" op that carries the
//! whole row, because such an op would silently overwrite the field the
//! other person just changed.
//!
//! A list is **project** (every member reads it) or **personal** (only its
//! author does). Visibility is fixed by `list.create` and repeated on every
//! op as the `td-vis` tag, so the relay withholds a personal op from every
//! reader but its author without parsing content, and the fold ignores an
//! op whose tag disagrees with its list.

use std::fmt;
use std::str::FromStr;

use serde_json::{Map, Value};

use crate::fractional_rank::validate_rank;
use crate::{normalize_project_coordinate, KIND_PROJECT_TODO_OP};

/// Exact `schema` value carried by kind 44248 content.
pub const PROJECT_TODO_SCHEMA: &str = "buzz-project-todo/v1";

/// Exact version carried by the `td-v` tag.
pub const PROJECT_TODO_TAG_VERSION: &str = "td1-1";

/// Maximum UTF-8 byte length of a complete op payload.
pub const MAX_PROJECT_TODO_CONTENT_BYTES: usize = 4 * 1024;

/// Maximum UTF-8 byte length of a list title or an item's text.
pub const MAX_PROJECT_TODO_TEXT_BYTES: usize = 1024;

/// Maximum UTF-8 byte length of an item's description. The whole op is
/// still capped at [`MAX_PROJECT_TODO_CONTENT_BYTES`], so a description
/// whose JSON escaping pushes the op past that cap is refused too.
pub const MAX_PROJECT_TODO_DESCRIPTION_BYTES: usize = 3 * 1024;

/// What one op does. The wire spelling is the `td-op` tag and the content
/// `op` field; [`validate_project_todo_envelope`] requires the two to agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectTodoOpKind {
    /// Bring a list into existence with a title.
    ListCreate,
    /// Retitle a list.
    ListTitle,
    /// Archive or unarchive a list.
    ListArchived,
    /// Pin or unpin a list in the project sidebar (shared by every member).
    ListPinned,
    /// Bring an item into existence with its text and its rank.
    ItemAdd,
    /// Rewrite an item's text.
    ItemText,
    /// Mark an item done or not done.
    ItemDone,
    /// Assign an item to a pubkey, or clear the assignee.
    ItemAssignee,
    /// Set or clear an item's due date.
    ItemDue,
    /// Move an item by giving it a new rank.
    ItemRank,
    /// Remove an item for good.
    ItemRemove,
    /// Set an item's workflow status ([`TodoStatus`]). Shares one field
    /// with `item.done`: the latest of the two wins.
    ItemStatus,
    /// Set an item's priority ([`TodoPriority`]).
    ItemPriority,
    /// Set or clear an item's longer description.
    ItemDescription,
}

impl ProjectTodoOpKind {
    /// Every op kind, in wire order.
    pub const ALL: [Self; 14] = [
        Self::ListCreate,
        Self::ListTitle,
        Self::ListArchived,
        Self::ListPinned,
        Self::ItemAdd,
        Self::ItemText,
        Self::ItemDone,
        Self::ItemAssignee,
        Self::ItemDue,
        Self::ItemRank,
        Self::ItemRemove,
        Self::ItemStatus,
        Self::ItemPriority,
        Self::ItemDescription,
    ];

    /// The wire spelling, identical in the `td-op` tag and in content.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ListCreate => "list.create",
            Self::ListTitle => "list.title",
            Self::ListArchived => "list.archived",
            Self::ListPinned => "list.pinned",
            Self::ItemAdd => "item.add",
            Self::ItemText => "item.text",
            Self::ItemDone => "item.done",
            Self::ItemAssignee => "item.assignee",
            Self::ItemDue => "item.due",
            Self::ItemRank => "item.rank",
            Self::ItemRemove => "item.remove",
            Self::ItemStatus => "item.status",
            Self::ItemPriority => "item.priority",
            Self::ItemDescription => "item.description",
        }
    }

    /// `true` for the ops that name an item and therefore carry `itemId`
    /// in content and a `td-item` tag.
    pub const fn is_item_op(self) -> bool {
        !matches!(
            self,
            Self::ListCreate | Self::ListTitle | Self::ListArchived | Self::ListPinned
        )
    }

    /// The exact content key set for this op, in canonical order. Absent is
    /// not null: a nullable field (`assignee`, `due`) must be present.
    pub const fn content_keys(self) -> &'static [&'static str] {
        match self {
            Self::ListCreate => &["schema", "op", "listId", "title", "visibility"],
            Self::ListTitle => &["schema", "op", "listId", "title"],
            Self::ListArchived => &["schema", "op", "listId", "archived"],
            Self::ListPinned => &["schema", "op", "listId", "pinned"],
            Self::ItemAdd => &["schema", "op", "listId", "itemId", "text", "rank"],
            Self::ItemText => &["schema", "op", "listId", "itemId", "text"],
            Self::ItemDone => &["schema", "op", "listId", "itemId", "done"],
            Self::ItemAssignee => &["schema", "op", "listId", "itemId", "assignee"],
            Self::ItemDue => &["schema", "op", "listId", "itemId", "due"],
            Self::ItemRank => &["schema", "op", "listId", "itemId", "rank"],
            Self::ItemRemove => &["schema", "op", "listId", "itemId"],
            Self::ItemStatus => &["schema", "op", "listId", "itemId", "status"],
            Self::ItemPriority => &["schema", "op", "listId", "itemId", "priority"],
            Self::ItemDescription => &["schema", "op", "listId", "itemId", "description"],
        }
    }
}

impl fmt::Display for ProjectTodoOpKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProjectTodoOpKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| format!("unknown project todo op {value:?}"))
    }
}

/// The payload of one op: what it sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectTodoOpValue {
    /// `list.create` — the initial title and the list's fixed visibility.
    ListCreate {
        /// The list's title.
        title: String,
        /// Who may read the list; fixed for its lifetime.
        visibility: TodoVisibility,
    },
    /// `list.title`.
    ListTitle {
        /// The new title.
        title: String,
    },
    /// `list.archived`.
    ListArchived {
        /// `true` to archive, `false` to restore.
        archived: bool,
    },
    /// `list.pinned`.
    ListPinned {
        /// `true` to show the list in every member's project sidebar.
        pinned: bool,
    },
    /// `item.add` — the initial text and rank.
    ItemAdd {
        /// The item.
        item_id: String,
        /// Its text.
        text: String,
        /// Its rank ([`crate::fractional_rank`]).
        rank: String,
    },
    /// `item.text`.
    ItemText {
        /// The item.
        item_id: String,
        /// The new text.
        text: String,
    },
    /// `item.done`.
    ItemDone {
        /// The item.
        item_id: String,
        /// Done or not.
        done: bool,
    },
    /// `item.assignee`.
    ItemAssignee {
        /// The item.
        item_id: String,
        /// The assignee's pubkey (64 lowercase hex), or `None` to clear.
        assignee: Option<String>,
    },
    /// `item.due`.
    ItemDue {
        /// The item.
        item_id: String,
        /// The due date as `YYYY-MM-DD`, or `None` to clear.
        due: Option<String>,
    },
    /// `item.rank`.
    ItemRank {
        /// The item.
        item_id: String,
        /// The new rank.
        rank: String,
    },
    /// `item.remove`.
    ItemRemove {
        /// The item.
        item_id: String,
    },
    /// `item.status`.
    ItemStatus {
        /// The item.
        item_id: String,
        /// The new status.
        status: TodoStatus,
    },
    /// `item.priority`.
    ItemPriority {
        /// The item.
        item_id: String,
        /// The new priority.
        priority: TodoPriority,
    },
    /// `item.description`.
    ItemDescription {
        /// The item.
        item_id: String,
        /// The description, or `None` to clear it.
        description: Option<String>,
    },
}

/// An item's workflow status. The vocabulary is closed and native: a
/// provider with its own workflow (Linear's per-team states) keeps its own
/// ids and never writes these.
///
/// `done` and `canceled` are the two **closed** states; an item in either is
/// listed among the completed items, exactly as `item.done{true}` lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TodoStatus {
    /// Intake: reported, not yet accepted into the backlog.
    Triage,
    /// Accepted, not yet planned.
    Backlog,
    /// Planned, not started. The state of an item nobody has touched.
    Todo,
    /// Being worked on.
    InProgress,
    /// Finished.
    Done,
    /// Closed without being done.
    Canceled,
}

impl TodoStatus {
    /// Every status, in workflow order.
    pub const ALL: [Self; 6] = [
        Self::Triage,
        Self::Backlog,
        Self::Todo,
        Self::InProgress,
        Self::Done,
        Self::Canceled,
    ];

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Triage => "triage",
            Self::Backlog => "backlog",
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Canceled => "canceled",
        }
    }

    /// `true` for `done` and `canceled`.
    pub const fn is_closed(self) -> bool {
        matches!(self, Self::Done | Self::Canceled)
    }
}

impl fmt::Display for TodoStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TodoStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
            .ok_or_else(|| {
                format!(
                    "todo status must be one of triage, backlog, todo, in_progress, done, canceled (got {value:?})"
                )
            })
    }
}

/// An item's priority. A string on the wire, never a number, so every
/// client's JSON reader agrees on what is valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TodoPriority {
    /// No priority set. The priority of an item nobody has touched.
    None,
    /// Urgent.
    Urgent,
    /// High.
    High,
    /// Medium.
    Medium,
    /// Low.
    Low,
}

impl TodoPriority {
    /// Every priority, most urgent first, `none` last.
    pub const ALL: [Self; 5] = [
        Self::Urgent,
        Self::High,
        Self::Medium,
        Self::Low,
        Self::None,
    ];

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Urgent => "urgent",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

impl fmt::Display for TodoPriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TodoPriority {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|priority| priority.as_str() == value)
            .ok_or_else(|| {
                format!(
                    "todo priority must be one of urgent, high, medium, low, none (got {value:?})"
                )
            })
    }
}

/// Who may read a list. Carried on every op as the `td-vis` tag and, for
/// `list.create`, in content as `visibility`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TodoVisibility {
    /// Every member of the project reads it.
    Project,
    /// Only its author reads it; the relay withholds every op from anyone
    /// else, and the fold ignores ops on it from anyone else.
    Personal,
}

impl TodoVisibility {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Personal => "personal",
        }
    }
}

impl fmt::Display for TodoVisibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TodoVisibility {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "project" => Ok(Self::Project),
            "personal" => Ok(Self::Personal),
            other => Err(format!(
                "todo visibility must be project or personal (got {other:?})"
            )),
        }
    }
}

/// One decoded op: the list it names, the list's visibility, and what it
/// sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTodoOp {
    /// The list (32 lowercase hex).
    pub list_id: String,
    /// The list's visibility, repeated on every op as the `td-vis` tag.
    pub visibility: TodoVisibility,
    /// The edit.
    pub value: ProjectTodoOpValue,
}

impl ProjectTodoOp {
    /// Which op this is.
    pub const fn kind(&self) -> ProjectTodoOpKind {
        match self.value {
            ProjectTodoOpValue::ListCreate { .. } => ProjectTodoOpKind::ListCreate,
            ProjectTodoOpValue::ListTitle { .. } => ProjectTodoOpKind::ListTitle,
            ProjectTodoOpValue::ListArchived { .. } => ProjectTodoOpKind::ListArchived,
            ProjectTodoOpValue::ListPinned { .. } => ProjectTodoOpKind::ListPinned,
            ProjectTodoOpValue::ItemAdd { .. } => ProjectTodoOpKind::ItemAdd,
            ProjectTodoOpValue::ItemText { .. } => ProjectTodoOpKind::ItemText,
            ProjectTodoOpValue::ItemDone { .. } => ProjectTodoOpKind::ItemDone,
            ProjectTodoOpValue::ItemAssignee { .. } => ProjectTodoOpKind::ItemAssignee,
            ProjectTodoOpValue::ItemDue { .. } => ProjectTodoOpKind::ItemDue,
            ProjectTodoOpValue::ItemRank { .. } => ProjectTodoOpKind::ItemRank,
            ProjectTodoOpValue::ItemRemove { .. } => ProjectTodoOpKind::ItemRemove,
            ProjectTodoOpValue::ItemStatus { .. } => ProjectTodoOpKind::ItemStatus,
            ProjectTodoOpValue::ItemPriority { .. } => ProjectTodoOpKind::ItemPriority,
            ProjectTodoOpValue::ItemDescription { .. } => ProjectTodoOpKind::ItemDescription,
        }
    }

    /// The item this op names, if it is an item op.
    pub fn item_id(&self) -> Option<&str> {
        match &self.value {
            ProjectTodoOpValue::ListCreate { .. }
            | ProjectTodoOpValue::ListTitle { .. }
            | ProjectTodoOpValue::ListArchived { .. }
            | ProjectTodoOpValue::ListPinned { .. } => None,
            ProjectTodoOpValue::ItemAdd { item_id, .. }
            | ProjectTodoOpValue::ItemText { item_id, .. }
            | ProjectTodoOpValue::ItemDone { item_id, .. }
            | ProjectTodoOpValue::ItemAssignee { item_id, .. }
            | ProjectTodoOpValue::ItemDue { item_id, .. }
            | ProjectTodoOpValue::ItemRank { item_id, .. }
            | ProjectTodoOpValue::ItemRemove { item_id }
            | ProjectTodoOpValue::ItemStatus { item_id, .. }
            | ProjectTodoOpValue::ItemPriority { item_id, .. }
            | ProjectTodoOpValue::ItemDescription { item_id, .. } => Some(item_id),
        }
    }

    /// Encode as canonical content JSON (keys in [`ProjectTodoOpKind::content_keys`]
    /// order, no whitespace). This is the inverse of [`decode_project_todo_op`].
    pub fn to_content(&self) -> String {
        let mut object = Map::new();
        object.insert("schema".into(), Value::from(PROJECT_TODO_SCHEMA));
        object.insert("op".into(), Value::from(self.kind().as_str()));
        object.insert("listId".into(), Value::from(self.list_id.as_str()));
        if let Some(item_id) = self.item_id() {
            object.insert("itemId".into(), Value::from(item_id));
        }
        match &self.value {
            ProjectTodoOpValue::ListCreate { title, visibility } => {
                object.insert("title".into(), Value::from(title.as_str()));
                object.insert("visibility".into(), Value::from(visibility.as_str()));
            }
            ProjectTodoOpValue::ListTitle { title } => {
                object.insert("title".into(), Value::from(title.as_str()));
            }
            ProjectTodoOpValue::ListArchived { archived } => {
                object.insert("archived".into(), Value::from(*archived));
            }
            ProjectTodoOpValue::ListPinned { pinned } => {
                object.insert("pinned".into(), Value::from(*pinned));
            }
            ProjectTodoOpValue::ItemAdd { text, rank, .. } => {
                object.insert("text".into(), Value::from(text.as_str()));
                object.insert("rank".into(), Value::from(rank.as_str()));
            }
            ProjectTodoOpValue::ItemText { text, .. } => {
                object.insert("text".into(), Value::from(text.as_str()));
            }
            ProjectTodoOpValue::ItemDone { done, .. } => {
                object.insert("done".into(), Value::from(*done));
            }
            ProjectTodoOpValue::ItemAssignee { assignee, .. } => {
                object.insert(
                    "assignee".into(),
                    assignee.as_deref().map(Value::from).unwrap_or(Value::Null),
                );
            }
            ProjectTodoOpValue::ItemDue { due, .. } => {
                object.insert(
                    "due".into(),
                    due.as_deref().map(Value::from).unwrap_or(Value::Null),
                );
            }
            ProjectTodoOpValue::ItemRank { rank, .. } => {
                object.insert("rank".into(), Value::from(rank.as_str()));
            }
            ProjectTodoOpValue::ItemRemove { .. } => {}
            ProjectTodoOpValue::ItemStatus { status, .. } => {
                object.insert("status".into(), Value::from(status.as_str()));
            }
            ProjectTodoOpValue::ItemPriority { priority, .. } => {
                object.insert("priority".into(), Value::from(priority.as_str()));
            }
            ProjectTodoOpValue::ItemDescription { description, .. } => {
                object.insert(
                    "description".into(),
                    description
                        .as_deref()
                        .map(Value::from)
                        .unwrap_or(Value::Null),
                );
            }
        }
        // serde_json's default Map preserves insertion order only with the
        // `preserve_order` feature; without it keys sort alphabetically. Either
        // is a valid encoding — the decoder is order-independent — so the
        // canonical form is simply "whatever `serde_json` emits here".
        Value::Object(object).to_string()
    }

    /// The tags this op carries, in canonical order: `a`, `td-v`, `td-op`,
    /// `td-list`, and `td-item` for item ops.
    pub fn tags(&self, coordinate: &str) -> Vec<Vec<String>> {
        let mut tags = vec![
            vec!["a".to_owned(), coordinate.to_owned()],
            vec!["td-v".to_owned(), PROJECT_TODO_TAG_VERSION.to_owned()],
            vec!["td-op".to_owned(), self.kind().as_str().to_owned()],
            vec!["td-list".to_owned(), self.list_id.clone()],
            vec!["td-vis".to_owned(), self.visibility.as_str().to_owned()],
        ];
        if let Some(item_id) = self.item_id() {
            tags.push(vec!["td-item".to_owned(), item_id.to_owned()]);
        }
        tags
    }
}

/// `true` for a 32-character lowercase hex id (a list or item id).
pub fn is_todo_id(value: &str) -> bool {
    value.len() == 32 && is_lower_hex(value)
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_id(field: &str, value: &str) -> Result<(), String> {
    if !is_todo_id(value) {
        return Err(format!("todo {field} must be 32 lowercase hex characters"));
    }
    Ok(())
}

/// Validate a list title or an item's text: non-blank, within
/// [`MAX_PROJECT_TODO_TEXT_BYTES`], no control characters other than newline
/// and tab.
pub fn validate_todo_text(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("todo {field} must not be blank"));
    }
    if value.len() > MAX_PROJECT_TODO_TEXT_BYTES {
        return Err(format!(
            "todo {field} exceeds {MAX_PROJECT_TODO_TEXT_BYTES} bytes"
        ));
    }
    if value
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(format!("todo {field} must not contain control characters"));
    }
    Ok(())
}

/// Validate an item's description: non-blank (clear it with `null`, not an
/// empty string), within [`MAX_PROJECT_TODO_DESCRIPTION_BYTES`], no control
/// characters other than newline and tab.
pub fn validate_todo_description(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("todo description must not be blank; clear it with null".to_owned());
    }
    if value.len() > MAX_PROJECT_TODO_DESCRIPTION_BYTES {
        return Err(format!(
            "todo description exceeds {MAX_PROJECT_TODO_DESCRIPTION_BYTES} bytes"
        ));
    }
    if value
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err("todo description must not contain control characters".to_owned());
    }
    Ok(())
}

/// Validate a due date: `YYYY-MM-DD`, a real calendar date, year 1970–9999.
pub fn validate_due_date(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    let well_formed = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|&i| bytes[i].is_ascii_digit());
    if !well_formed {
        return Err("todo due date must be YYYY-MM-DD".to_owned());
    }
    let year: u32 = value[0..4]
        .parse()
        .map_err(|_| "todo due year".to_owned())?;
    let month: u32 = value[5..7]
        .parse()
        .map_err(|_| "todo due month".to_owned())?;
    let day: u32 = value[8..10]
        .parse()
        .map_err(|_| "todo due day".to_owned())?;
    if !(1970..=9999).contains(&year) {
        return Err("todo due year must be 1970–9999".to_owned());
    }
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err("todo due month must be 01–12".to_owned()),
    };
    if day == 0 || day > days_in_month {
        return Err("todo due day is not a calendar day of that month".to_owned());
    }
    Ok(())
}

fn take_str<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("todo op {key} must be a string"))
}

fn take_bool(object: &Map<String, Value>, key: &str) -> Result<bool, String> {
    object
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("todo op {key} must be a boolean"))
}

fn take_nullable_str<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, String> {
    match object.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        _ => Err(format!("todo op {key} must be a string or null")),
    }
}

/// Strictly decode and validate op content for a list of `visibility` (the
/// event's `td-vis` tag, which the caller has already read).
///
/// The whole-content cap is checked before any parse. The key set is
/// **exact** per op — every key in [`ProjectTodoOpKind::content_keys`] must
/// be present and no other may be — so absent and `null` are different
/// things, and a client that forgets a field is told which one. A
/// `list.create` whose content `visibility` disagrees with the tag is an
/// error: the tag is what the relay gates on, and the two must never drift.
pub fn decode_project_todo_op(
    content: &str,
    visibility: TodoVisibility,
) -> Result<ProjectTodoOp, String> {
    if content.len() > MAX_PROJECT_TODO_CONTENT_BYTES {
        return Err(format!(
            "todo op content exceeds {MAX_PROJECT_TODO_CONTENT_BYTES} bytes"
        ));
    }
    let value: Value =
        serde_json::from_str(content).map_err(|_| "malformed todo op payload".to_owned())?;
    let object = value
        .as_object()
        .ok_or_else(|| "todo op payload must be an object".to_owned())?;
    match object.get("schema").and_then(Value::as_str) {
        Some(PROJECT_TODO_SCHEMA) => {}
        _ => return Err(format!("todo op schema must be {PROJECT_TODO_SCHEMA:?}")),
    }
    let kind = ProjectTodoOpKind::from_str(take_str(object, "op")?)?;
    let expected = kind.content_keys();
    if let Some(unknown) = object.keys().find(|key| !expected.contains(&key.as_str())) {
        return Err(format!(
            "todo op {} has unsupported field {unknown:?}",
            kind.as_str()
        ));
    }
    if let Some(missing) = expected.iter().find(|key| !object.contains_key(**key)) {
        return Err(format!(
            "todo op {} is missing field {missing:?}",
            kind.as_str()
        ));
    }
    let list_id = take_str(object, "listId")?.to_owned();
    validate_id("listId", &list_id)?;
    let item_id = if kind.is_item_op() {
        let id = take_str(object, "itemId")?.to_owned();
        validate_id("itemId", &id)?;
        Some(id)
    } else {
        None
    };
    let item = || item_id.clone().unwrap_or_default();
    let value = match kind {
        ProjectTodoOpKind::ListCreate => {
            let title = take_str(object, "title")?.to_owned();
            validate_todo_text("title", &title)?;
            let declared = TodoVisibility::from_str(take_str(object, "visibility")?)?;
            if declared != visibility {
                return Err(format!(
                    "todo list.create content visibility {:?} does not match its td-vis tag {:?}",
                    declared.as_str(),
                    visibility.as_str()
                ));
            }
            ProjectTodoOpValue::ListCreate {
                title,
                visibility: declared,
            }
        }
        ProjectTodoOpKind::ListTitle => {
            let title = take_str(object, "title")?.to_owned();
            validate_todo_text("title", &title)?;
            ProjectTodoOpValue::ListTitle { title }
        }
        ProjectTodoOpKind::ListArchived => ProjectTodoOpValue::ListArchived {
            archived: take_bool(object, "archived")?,
        },
        ProjectTodoOpKind::ListPinned => ProjectTodoOpValue::ListPinned {
            pinned: take_bool(object, "pinned")?,
        },
        ProjectTodoOpKind::ItemAdd => {
            let text = take_str(object, "text")?.to_owned();
            validate_todo_text("text", &text)?;
            let rank = take_str(object, "rank")?.to_owned();
            validate_rank(&rank)?;
            ProjectTodoOpValue::ItemAdd {
                item_id: item(),
                text,
                rank,
            }
        }
        ProjectTodoOpKind::ItemText => {
            let text = take_str(object, "text")?.to_owned();
            validate_todo_text("text", &text)?;
            ProjectTodoOpValue::ItemText {
                item_id: item(),
                text,
            }
        }
        ProjectTodoOpKind::ItemDone => ProjectTodoOpValue::ItemDone {
            item_id: item(),
            done: take_bool(object, "done")?,
        },
        ProjectTodoOpKind::ItemAssignee => {
            let assignee = take_nullable_str(object, "assignee")?.map(str::to_owned);
            if let Some(hex) = &assignee {
                if hex.len() != 64 || !is_lower_hex(hex) {
                    return Err(
                        "todo assignee must be a 64-character lowercase hex pubkey".to_owned()
                    );
                }
            }
            ProjectTodoOpValue::ItemAssignee {
                item_id: item(),
                assignee,
            }
        }
        ProjectTodoOpKind::ItemDue => {
            let due = take_nullable_str(object, "due")?.map(str::to_owned);
            if let Some(date) = &due {
                validate_due_date(date)?;
            }
            ProjectTodoOpValue::ItemDue {
                item_id: item(),
                due,
            }
        }
        ProjectTodoOpKind::ItemRank => {
            let rank = take_str(object, "rank")?.to_owned();
            validate_rank(&rank)?;
            ProjectTodoOpValue::ItemRank {
                item_id: item(),
                rank,
            }
        }
        ProjectTodoOpKind::ItemRemove => ProjectTodoOpValue::ItemRemove { item_id: item() },
        ProjectTodoOpKind::ItemStatus => ProjectTodoOpValue::ItemStatus {
            item_id: item(),
            status: TodoStatus::from_str(take_str(object, "status")?)?,
        },
        ProjectTodoOpKind::ItemPriority => ProjectTodoOpValue::ItemPriority {
            item_id: item(),
            priority: TodoPriority::from_str(take_str(object, "priority")?)?,
        },
        ProjectTodoOpKind::ItemDescription => {
            let description = take_nullable_str(object, "description")?.map(str::to_owned);
            if let Some(text) = &description {
                validate_todo_description(text)?;
            }
            ProjectTodoOpValue::ItemDescription {
                item_id: item(),
                description,
            }
        }
    };
    Ok(ProjectTodoOp {
        list_id,
        visibility,
        value,
    })
}

/// Validate an op from its kind, tags and content: tag grammar, canonical
/// project coordinate, content envelope, and tag/content agreement.
///
/// This is the single validator. The relay calls it through
/// `beekeeper_core::project_todo::validate_project_todo_envelope`, the
/// buzz-sdk builder delegates to that, and a plugin host checks an unsigned
/// draft with it before anyone signs — none defines a local copy.
///
/// **Tag grammar** — position-independent, multiplicity-constrained, closed
/// key set: exactly one each of `a`, `td-v`, `td-op`, `td-list`, `td-vis`;
/// exactly one `td-item` on an item op and none on a list op; every tag
/// exactly two fields; any other key — including `h` — is a rejection. A
/// to-do op is never channel-scoped.
pub fn validate_project_todo_parts(
    kind: u32,
    tags: &[Vec<String>],
    content: &str,
) -> Result<ProjectTodoOp, String> {
    if kind != KIND_PROJECT_TODO_OP {
        return Err("event is not a project todo op (kind 44248)".to_owned());
    }

    let mut coordinate: Option<&str> = None;
    let mut version: Option<&str> = None;
    let mut tag_op: Option<&str> = None;
    let mut tag_list: Option<&str> = None;
    let mut tag_vis: Option<&str> = None;
    let mut tag_item: Option<&str> = None;
    for parts in tags {
        if parts.len() != 2 {
            return Err("todo op tags must have exactly two fields".to_owned());
        }
        let key = parts[0].as_str();
        let slot = match key {
            "a" => &mut coordinate,
            "td-v" => &mut version,
            "td-op" => &mut tag_op,
            "td-list" => &mut tag_list,
            "td-vis" => &mut tag_vis,
            "td-item" => &mut tag_item,
            "h" => return Err("todo op must not carry an h tag; it is project-scoped".to_owned()),
            other => return Err(format!("todo op has unsupported tag key {other:?}")),
        };
        if slot.is_some() {
            return Err(format!("todo op has more than one {key} tag"));
        }
        *slot = Some(parts[1].as_str());
    }

    let coordinate = coordinate.ok_or_else(|| "todo op requires one a tag".to_owned())?;
    if normalize_project_coordinate(coordinate).as_deref() != Some(coordinate) {
        return Err(
            "44248 `a` tag must be a canonical 30621:<lowercase-hex>:<dtag> coordinate".to_owned(),
        );
    }
    match version {
        Some(PROJECT_TODO_TAG_VERSION) => {}
        _ => return Err("unsupported todo op tag version".to_owned()),
    }
    let visibility = TodoVisibility::from_str(
        tag_vis.ok_or_else(|| "todo op requires one td-vis tag".to_owned())?,
    )?;
    let op = decode_project_todo_op(content, visibility)?;
    let tag_op = tag_op.ok_or_else(|| "todo op requires one td-op tag".to_owned())?;
    if tag_op != op.kind().as_str() {
        return Err(format!(
            "todo op td-op tag {tag_op:?} does not match content op {:?}",
            op.kind().as_str()
        ));
    }
    let tag_list = tag_list.ok_or_else(|| "todo op requires one td-list tag".to_owned())?;
    if tag_list != op.list_id {
        return Err("todo op td-list tag does not match content listId".to_owned());
    }
    match (tag_item, op.item_id()) {
        (Some(tag), Some(content)) if tag == content => {}
        (Some(_), Some(_)) => {
            return Err("todo op td-item tag does not match content itemId".to_owned())
        }
        (None, Some(_)) => return Err("todo item op requires one td-item tag".to_owned()),
        (Some(_), None) => return Err("todo list op must not carry a td-item tag".to_owned()),
        (None, None) => {}
    }
    Ok(op)
}
