//! Project to-do operations (kind 44248): one field-level edit to a shared,
//! project-scoped to-do list.
//!
//! A to-do list is not an event. It is what a reader gets by folding every
//! op that names it — `list.create` first, then the latest `(created_at,
//! id)` write per field — and that fold lives in
//! [`crate::project_todo_fold`], pinned by `conformance/project-todo-fold/`.
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
use crate::kind::{
    event_kind_u32, normalize_project_coordinate, project_a_scoped_coordinate, KIND_PROJECT_TODO_OP,
};

/// Exact `schema` value carried by kind 44248 content.
pub const PROJECT_TODO_SCHEMA: &str = "buzz-project-todo/v1";

/// Exact version carried by the `td-v` tag.
pub const PROJECT_TODO_TAG_VERSION: &str = "td1-1";

/// Maximum UTF-8 byte length of a complete op payload.
pub const MAX_PROJECT_TODO_CONTENT_BYTES: usize = 4 * 1024;

/// Maximum UTF-8 byte length of a list title or an item's text.
pub const MAX_PROJECT_TODO_TEXT_BYTES: usize = 1024;

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
}

impl ProjectTodoOpKind {
    /// Every op kind, in wire order.
    pub const ALL: [Self; 11] = [
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
            | ProjectTodoOpValue::ItemRemove { item_id } => Some(item_id),
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
    };
    Ok(ProjectTodoOp {
        list_id,
        visibility,
        value,
    })
}

/// Validate a signed op end to end: kind, tag grammar, canonical project
/// coordinate, content envelope, and tag/content agreement.
///
/// This is the single validator. The relay calls it and defines no local
/// copy; the buzz-sdk builder delegates to it rather than repeating the
/// rules.
///
/// **Tag grammar** — position-independent, multiplicity-constrained, closed
/// key set: exactly one each of `a`, `td-v`, `td-op`, `td-list`, `td-vis`;
/// exactly one `td-item` on an item op and none on a list op; every tag
/// exactly two fields; any other key — including `h` — is a rejection. A
/// to-do op is never channel-scoped.
pub fn validate_project_todo_envelope(event: &nostr::Event) -> Result<ProjectTodoOp, String> {
    if event_kind_u32(event) != KIND_PROJECT_TODO_OP {
        return Err("event is not a project todo op (kind 44248)".to_owned());
    }

    let mut coordinate: Option<&str> = None;
    let mut version: Option<&str> = None;
    let mut tag_op: Option<&str> = None;
    let mut tag_list: Option<&str> = None;
    let mut tag_vis: Option<&str> = None;
    let mut tag_item: Option<&str> = None;
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
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
    let op = decode_project_todo_op(&event.content, visibility)?;
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
    // The extractor and the check above agree by construction; this keeps
    // the validator honest if either changes.
    if project_a_scoped_coordinate(event).as_deref() != Some(coordinate) {
        return Err("todo op a tag did not resolve to its coordinate".to_owned());
    }
    Ok(op)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    const COORD: &str =
        "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:tank-loop";
    const LIST: &str = "0123456789abcdef0123456789abcdef";
    const ITEM: &str = "fedcba9876543210fedcba9876543210";

    fn add_op() -> ProjectTodoOp {
        ProjectTodoOp {
            list_id: LIST.to_owned(),
            visibility: TodoVisibility::Project,
            value: ProjectTodoOpValue::ItemAdd {
                item_id: ITEM.to_owned(),
                text: "Write the NIP".to_owned(),
                rank: "a0".to_owned(),
            },
        }
    }

    fn event_with(content: &str, tags: &[&[&str]]) -> nostr::Event {
        let keys = Keys::generate();
        let tag_vec: Vec<Tag> = tags
            .iter()
            .map(|parts| Tag::parse(parts.iter().copied()).expect("test tag parses"))
            .collect();
        EventBuilder::new(
            Kind::Custom(KIND_PROJECT_TODO_OP as u16),
            content.to_owned(),
        )
        .tags(tag_vec)
        .sign_with_keys(&keys)
        .expect("test event signs")
    }

    fn event_for(op: &ProjectTodoOp) -> nostr::Event {
        let tags = op.tags(COORD);
        let borrowed: Vec<Vec<&str>> = tags
            .iter()
            .map(|t| t.iter().map(String::as_str).collect())
            .collect();
        let slices: Vec<&[&str]> = borrowed.iter().map(Vec::as_slice).collect();
        event_with(&op.to_content(), &slices)
    }

    #[test]
    fn every_op_round_trips_through_content_and_envelope() {
        let ops = vec![
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ListCreate {
                    title: "Launch".to_owned(),
                    visibility: TodoVisibility::Project,
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ListTitle {
                    title: "Launch v2".to_owned(),
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ListArchived { archived: true },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ListPinned { pinned: true },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Personal,
                value: ProjectTodoOpValue::ListCreate {
                    title: "Mine".to_owned(),
                    visibility: TodoVisibility::Personal,
                },
            },
            add_op(),
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemText {
                    item_id: ITEM.to_owned(),
                    text: "Write the NIP\nwith examples".to_owned(),
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemDone {
                    item_id: ITEM.to_owned(),
                    done: true,
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemAssignee {
                    item_id: ITEM.to_owned(),
                    assignee: Some("b".repeat(64)),
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemAssignee {
                    item_id: ITEM.to_owned(),
                    assignee: None,
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemDue {
                    item_id: ITEM.to_owned(),
                    due: Some("2028-02-29".to_owned()),
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemDue {
                    item_id: ITEM.to_owned(),
                    due: None,
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemRank {
                    item_id: ITEM.to_owned(),
                    rank: "a0V".to_owned(),
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemRemove {
                    item_id: ITEM.to_owned(),
                },
            },
        ];
        assert_eq!(ops.len(), ProjectTodoOpKind::ALL.len() + 3);
        for op in ops {
            let decoded =
                decode_project_todo_op(&op.to_content(), op.visibility).expect("content decodes");
            assert_eq!(decoded, op);
            let validated = validate_project_todo_envelope(&event_for(&op)).expect("envelope");
            assert_eq!(validated, op);
            assert_eq!(
                ProjectTodoOpKind::from_str(op.kind().as_str()),
                Ok(op.kind())
            );
        }
    }

    #[test]
    fn content_key_set_is_exact() {
        let content = add_op().to_content();
        let mut object: Map<String, Value> = serde_json::from_str(&content).unwrap();
        object.insert("priority".into(), Value::from("high"));
        let err = decode_project_todo_op(
            &Value::Object(object.clone()).to_string(),
            TodoVisibility::Project,
        )
        .unwrap_err();
        assert!(err.contains("unsupported field \"priority\""), "{err}");

        object.remove("priority");
        object.remove("rank");
        let err =
            decode_project_todo_op(&Value::Object(object).to_string(), TodoVisibility::Project)
                .unwrap_err();
        assert!(err.contains("missing field \"rank\""), "{err}");

        // Absent is not null: an assignee op must say null explicitly.
        let missing_assignee = format!(
            r#"{{"schema":"{PROJECT_TODO_SCHEMA}","op":"item.assignee","listId":"{LIST}","itemId":"{ITEM}"}}"#
        );
        assert!(
            decode_project_todo_op(&missing_assignee, TodoVisibility::Project)
                .unwrap_err()
                .contains("missing field \"assignee\"")
        );
    }

    #[test]
    fn rejects_bad_values() {
        let bad = |patch: &str| {
            let mut object: Map<String, Value> =
                serde_json::from_str(&add_op().to_content()).unwrap();
            let (k, v): (String, Value) = serde_json::from_str(patch).unwrap();
            object.insert(k, v);
            decode_project_todo_op(&Value::Object(object).to_string(), TodoVisibility::Project)
                .unwrap_err()
        };
        assert!(bad(r#"["listId","short"]"#).contains("listId"));
        assert!(bad(r#"["itemId","FEDCBA9876543210FEDCBA9876543210"]"#).contains("itemId"));
        assert!(bad(r#"["text","   "]"#).contains("blank"));
        assert!(bad("[\"text\",\"a\\u0007b\"]").contains("control"));
        assert!(bad(r#"["rank","a0V0"]"#).contains("rank"));
        assert!(bad(r#"["schema","buzz-project-todo/v0"]"#).contains("schema"));
        assert!(bad(r#"["op","item.rename"]"#).contains("unknown project todo op"));

        let long = "x".repeat(MAX_PROJECT_TODO_TEXT_BYTES + 1);
        assert!(validate_todo_text("text", &long).is_err());
        let oversize = format!(
            r#"{{"schema":"{PROJECT_TODO_SCHEMA}","op":"item.text","listId":"{LIST}","itemId":"{ITEM}","text":"{}"}}"#,
            "y".repeat(MAX_PROJECT_TODO_CONTENT_BYTES)
        );
        assert!(decode_project_todo_op(&oversize, TodoVisibility::Project)
            .unwrap_err()
            .contains("exceeds"));
    }

    #[test]
    fn due_dates_are_real_calendar_days() {
        for ok in [
            "1970-01-01",
            "2026-02-28",
            "2024-02-29",
            "2000-02-29",
            "9999-12-31",
        ] {
            validate_due_date(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
        }
        for bad in [
            "2026-2-28",
            "2026-02-30",
            "2026-13-01",
            "2100-02-29",
            "1969-12-31",
            "2026/02/28",
            "2026-02-28T00:00",
            "",
        ] {
            assert!(validate_due_date(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn assignee_must_be_a_lowercase_pubkey() {
        let content = |assignee: &str| {
            format!(
                r#"{{"schema":"{PROJECT_TODO_SCHEMA}","op":"item.assignee","listId":"{LIST}","itemId":"{ITEM}","assignee":{assignee}}}"#
            )
        };
        assert!(decode_project_todo_op(&content("null"), TodoVisibility::Project).is_ok());
        assert!(decode_project_todo_op(
            &content(&format!("\"{}\"", "c".repeat(64))),
            TodoVisibility::Project
        )
        .is_ok());
        assert!(decode_project_todo_op(
            &content(&format!("\"{}\"", "C".repeat(64))),
            TodoVisibility::Project
        )
        .is_err());
        assert!(decode_project_todo_op(&content("\"abc\""), TodoVisibility::Project).is_err());
        assert!(decode_project_todo_op(&content("7"), TodoVisibility::Project).is_err());
    }

    #[test]
    fn tag_grammar_is_closed_and_agrees_with_content() {
        let op = add_op();
        let content = op.to_content();
        let ok = validate_project_todo_envelope(&event_with(
            &content,
            &[
                &["td-item", ITEM],
                &["td-list", LIST],
                &["td-vis", "project"],
                &["td-op", "item.add"],
                &["td-v", PROJECT_TODO_TAG_VERSION],
                &["a", COORD],
            ],
        ));
        assert!(ok.is_ok(), "tag order is free: {ok:?}");

        let cases: Vec<(&str, Vec<&[&str]>)> = vec![
            ("requires one a tag", vec![&["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM]]),
            ("canonical", vec![&["a", "30621:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:tank-loop"], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM]]),
            ("tag version", vec![&["a", COORD], &["td-v", "td1-2"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM]]),
            ("does not match content op", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.text"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM]]),
            ("does not match content listId", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", ITEM], &["td-vis", "project"], &["td-item", ITEM]]),
            ("does not match content itemId", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", LIST]]),
            ("requires one td-item tag", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"]]),
            ("requires one td-vis tag", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-item", ITEM]]),
            ("visibility must be project or personal", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "team"], &["td-item", ITEM]]),
            ("must not carry an h tag", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM], &["h", "0c3b3f8e-1c1d-4f7e-9a2b-3c4d5e6f7a8b"]]),
            ("unsupported tag key", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM], &["p", "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"]]),
            ("more than one a tag", vec![&["a", COORD], &["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM]]),
            ("exactly two fields", vec![&["a", COORD, "extra"], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM]]),
        ];
        for (needle, tags) in cases {
            let err = validate_project_todo_envelope(&event_with(&content, &tags)).unwrap_err();
            assert!(err.contains(needle), "expected {needle:?} in {err:?}");
        }

        // A list op must not carry td-item.
        let list_op = ProjectTodoOp {
            list_id: LIST.to_owned(),
            visibility: TodoVisibility::Project,
            value: ProjectTodoOpValue::ListArchived { archived: false },
        };
        let err = validate_project_todo_envelope(&event_with(
            &list_op.to_content(),
            &[
                &["a", COORD],
                &["td-v", "td1-1"],
                &["td-op", "list.archived"],
                &["td-list", LIST],
                &["td-vis", "project"],
                &["td-item", ITEM],
            ],
        ))
        .unwrap_err();
        assert!(err.contains("must not carry a td-item tag"), "{err}");
    }

    #[test]
    fn create_visibility_must_match_its_tag() {
        let content = format!(
            r#"{{"schema":"{PROJECT_TODO_SCHEMA}","op":"list.create","listId":"{LIST}","title":"Mine","visibility":"personal"}}"#
        );
        assert!(decode_project_todo_op(&content, TodoVisibility::Personal).is_ok());
        let err = decode_project_todo_op(&content, TodoVisibility::Project).unwrap_err();
        assert!(err.contains("does not match its td-vis tag"), "{err}");
        let err = validate_project_todo_envelope(&event_with(
            &content,
            &[
                &["a", COORD],
                &["td-v", "td1-1"],
                &["td-op", "list.create"],
                &["td-list", LIST],
                &["td-vis", "project"],
            ],
        ))
        .unwrap_err();
        assert!(err.contains("does not match its td-vis tag"), "{err}");
    }

    #[test]
    fn wrong_kind_is_rejected() {
        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::Custom(44240), add_op().to_content())
            .sign_with_keys(&keys)
            .unwrap();
        assert!(validate_project_todo_envelope(&event)
            .unwrap_err()
            .contains("kind 44248"));
    }
}
