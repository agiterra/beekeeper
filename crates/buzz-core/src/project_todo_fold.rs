//! The project to-do fold: from a bag of kind 44248 ops to the lists and
//! items a reader sees.
//!
//! The fold is pure and total: any set of events in, one digest out, the same
//! digest from every client. Its rules are the contract in
//! `conformance/project-todo-fold/CONTRACT.md` and are pinned by the vectors
//! beside it, which the TypeScript fold in Desktop and the Dart fold in
//! Mobile bind to as well. A rule implemented in only one fold is a defect.
//!
//! Rules, in the order the fold applies them:
//!
//! 1. **Decode.** An event that is not a 44248, whose single `a` tag is not
//!    this project, or whose content fails [`crate::project_todo`] is
//!    counted in `ignored` and dropped. Duplicate ids keep the first.
//! 2. **Order.** Ops sort by `(created_at, id)` ascending. That pair is the
//!    only clock; there is no per-author sequence.
//! 3. **Create.** The earliest `list.create` per list id and the earliest
//!    `item.add` per `(list, item)` bring the target into existence with the
//!    op's values; later creates for the same id are ignored. An `item.add`
//!    for a list that was never created is ignored.
//! 4. **Remove.** Any `item.remove` on an existing item is terminal: the
//!    item is dropped from the digest and every other op on it is
//!    disregarded. There is no un-remove; a client that wants the item back
//!    adds a new one.
//! 5. **Fields.** Each remaining op sets one field. Per field, the write with
//!    the greatest `(created_at, id)` wins, and the create op's values take
//!    part with the create's own key — so a `text` op stamped *before* the
//!    `add` (a skewed clock) loses to the add. Ops naming a list or item that
//!    does not exist are ignored.
//! 6. **Done.** `item.done{true}` also records `completedAt`/`completedBy`
//!    from the winning op; `item.done{false}` clears them.
//! 7. **Order out.** Lists sort by `(createdAt, id)`. Within a list, open
//!    items sort by `(rank, id)` and completed items by
//!    `(completedAt, doneOpId)` descending — most recently completed first.
//!    `updatedAt` on an item is the greatest applied op key; on a list, the
//!    greatest over its own ops and its items' (removes included).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::kind::{normalize_project_coordinate, KIND_PROJECT_TODO_OP};
use crate::project_todo::{decode_project_todo_op, ProjectTodoOp, ProjectTodoOpValue};

/// Exact `schema` value carried by a digest.
pub const PROJECT_TODO_DIGEST_SCHEMA: &str = "buzz-project-todo-digest/v1";

/// One stored event as the fold sees it — the subset of a Nostr event the
/// rules read. A relay-stored event converts losslessly; the conformance
/// vectors are written in this shape directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoFoldEvent {
    /// Event id (64 hex).
    pub id: String,
    /// Author (64 hex).
    pub pubkey: String,
    /// Seconds since the epoch.
    pub created_at: u64,
    /// Event kind; anything but 44248 is ignored.
    pub kind: u32,
    /// Tags; only the single `a` tag is read.
    pub tags: Vec<Vec<String>>,
    /// Op content JSON.
    pub content: String,
}

impl From<&nostr::Event> for TodoFoldEvent {
    fn from(event: &nostr::Event) -> Self {
        Self {
            id: event.id.to_hex(),
            pubkey: event.pubkey.to_hex(),
            created_at: event.created_at.as_secs(),
            kind: crate::kind::event_kind_u32(event),
            tags: event
                .tags
                .iter()
                .map(|tag| tag.as_slice().to_vec())
                .collect(),
            content: event.content.clone(),
        }
    }
}

/// One item in the digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    /// Item id.
    pub id: String,
    /// The list it belongs to.
    pub list_id: String,
    /// Text.
    pub text: String,
    /// Done or not.
    pub done: bool,
    /// Rank among the open items.
    pub rank: String,
    /// Assignee pubkey, if any.
    pub assignee: Option<String>,
    /// Due date `YYYY-MM-DD`, if any.
    pub due: Option<String>,
    /// The winning `item.add`'s `created_at`.
    pub created_at: u64,
    /// The winning `item.add`'s author.
    pub created_by: String,
    /// The greatest applied op `created_at`.
    pub updated_at: u64,
    /// When the winning `item.done{true}` was stamped, if done.
    pub completed_at: Option<u64>,
    /// Who stamped it, if done.
    pub completed_by: Option<String>,
}

/// One list in the digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoList {
    /// List id.
    pub id: String,
    /// Title.
    pub title: String,
    /// Archived or not.
    pub archived: bool,
    /// The winning `list.create`'s `created_at`.
    pub created_at: u64,
    /// The winning `list.create`'s author.
    pub created_by: String,
    /// The greatest applied op `created_at` over the list and its items.
    pub updated_at: u64,
    /// Not-done items, by rank.
    pub open: Vec<TodoItem>,
    /// Done items, most recently completed first.
    pub completed: Vec<TodoItem>,
}

/// The fold's output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTodoDigest {
    /// Always [`PROJECT_TODO_DIGEST_SCHEMA`].
    pub schema: String,
    /// The project coordinate the ops were folded for.
    pub project: String,
    /// Events that did not decode or named no existing target.
    pub ignored: usize,
    /// Lists, by `(createdAt, id)`.
    pub lists: Vec<TodoList>,
}

/// `(created_at, id)` — the fold's only clock.
type OpKey = (u64, String);

struct ListState {
    title: (OpKey, String),
    archived: (OpKey, bool),
    created_at: u64,
    created_by: String,
    updated_at: u64,
}

struct ItemState {
    text: (OpKey, String),
    rank: (OpKey, String),
    done: (OpKey, bool, Option<String>),
    assignee: (OpKey, Option<String>),
    due: (OpKey, Option<String>),
    created_at: u64,
    created_by: String,
    updated_at: u64,
}

struct Decoded<'a> {
    key: OpKey,
    pubkey: &'a str,
    op: ProjectTodoOp,
}

fn decode(project: &str, event: &TodoFoldEvent) -> Option<ProjectTodoOp> {
    if event.kind != KIND_PROJECT_TODO_OP {
        return None;
    }
    let mut a_tags = event
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("a"))
        .filter_map(|t| t.get(1));
    let coordinate = a_tags.next()?;
    if a_tags.next().is_some() {
        return None;
    }
    if normalize_project_coordinate(coordinate).as_deref() != Some(project) {
        return None;
    }
    decode_project_todo_op(&event.content).ok()
}

fn set_field<T>(slot: &mut (OpKey, T), key: &OpKey, value: T) {
    if *key > slot.0 {
        *slot = (key.clone(), value);
    }
}

/// Fold `events` for `project` into a digest. See the module doc for the
/// rules. `project` must already be a canonical coordinate; events naming
/// any other coordinate are ignored.
pub fn fold_project_todos(project: &str, events: &[TodoFoldEvent]) -> ProjectTodoDigest {
    let mut ignored = 0usize;
    let mut seen = HashSet::new();
    let mut ops: Vec<Decoded<'_>> = Vec::new();
    for event in events {
        if !seen.insert(event.id.as_str()) {
            continue;
        }
        match decode(project, event) {
            Some(op) => ops.push(Decoded {
                key: (event.created_at, event.id.clone()),
                pubkey: &event.pubkey,
                op,
            }),
            None => ignored += 1,
        }
    }
    ops.sort_by(|a, b| a.key.cmp(&b.key));

    // Creates, earliest first.
    let mut lists: HashMap<&str, ListState> = HashMap::new();
    for d in &ops {
        if let ProjectTodoOpValue::ListCreate { title } = &d.op.value {
            if lists.contains_key(d.op.list_id.as_str()) {
                ignored += 1;
                continue;
            }
            lists.insert(
                &d.op.list_id,
                ListState {
                    title: (d.key.clone(), title.clone()),
                    archived: (d.key.clone(), false),
                    created_at: d.key.0,
                    created_by: d.pubkey.to_owned(),
                    updated_at: d.key.0,
                },
            );
        }
    }
    let mut items: HashMap<(&str, &str), ItemState> = HashMap::new();
    let mut removed: HashSet<(&str, &str)> = HashSet::new();
    for d in &ops {
        if let ProjectTodoOpValue::ItemAdd {
            item_id,
            text,
            rank,
        } = &d.op.value
        {
            let Some(list) = lists.get_mut(d.op.list_id.as_str()) else {
                ignored += 1;
                continue;
            };
            if items.contains_key(&(d.op.list_id.as_str(), item_id.as_str())) {
                ignored += 1;
                continue;
            }
            list.updated_at = list.updated_at.max(d.key.0);
            items.insert(
                (&d.op.list_id, item_id),
                ItemState {
                    text: (d.key.clone(), text.clone()),
                    rank: (d.key.clone(), rank.clone()),
                    done: (d.key.clone(), false, None),
                    assignee: (d.key.clone(), None),
                    due: (d.key.clone(), None),
                    created_at: d.key.0,
                    created_by: d.pubkey.to_owned(),
                    updated_at: d.key.0,
                },
            );
        }
    }
    // Removes are terminal, whenever they were stamped.
    for d in &ops {
        if let ProjectTodoOpValue::ItemRemove { item_id } = &d.op.value {
            let target = (d.op.list_id.as_str(), item_id.as_str());
            if items.contains_key(&target) {
                removed.insert(target);
                if let Some(list) = lists.get_mut(d.op.list_id.as_str()) {
                    list.updated_at = list.updated_at.max(d.key.0);
                }
            } else {
                ignored += 1;
            }
        }
    }

    // Field writes, latest key per field.
    for d in &ops {
        let list_id = d.op.list_id.as_str();
        let Some(list) = lists.get_mut(list_id) else {
            // Creates and removes on a missing list were already counted in
            // their own passes; every other op on a missing list is counted
            // here.
            if !matches!(
                d.op.value,
                ProjectTodoOpValue::ListCreate { .. }
                    | ProjectTodoOpValue::ItemAdd { .. }
                    | ProjectTodoOpValue::ItemRemove { .. }
            ) {
                ignored += 1;
            }
            continue;
        };
        match &d.op.value {
            ProjectTodoOpValue::ListCreate { .. } | ProjectTodoOpValue::ItemAdd { .. } => {
                // Already folded as creates; an ItemAdd for a missing list was
                // counted there too.
            }
            ProjectTodoOpValue::ItemRemove { .. } => {}
            ProjectTodoOpValue::ListTitle { title } => {
                set_field(&mut list.title, &d.key, title.clone());
                list.updated_at = list.updated_at.max(d.key.0);
            }
            ProjectTodoOpValue::ListArchived { archived } => {
                set_field(&mut list.archived, &d.key, *archived);
                list.updated_at = list.updated_at.max(d.key.0);
            }
            ProjectTodoOpValue::ItemText { item_id, text } => {
                let target = (list_id, item_id.as_str());
                if removed.contains(&target) {
                    continue;
                }
                let Some(item) = items.get_mut(&target) else {
                    ignored += 1;
                    continue;
                };
                set_field(&mut item.text, &d.key, text.clone());
                item.updated_at = item.updated_at.max(d.key.0);
                list.updated_at = list.updated_at.max(d.key.0);
            }
            ProjectTodoOpValue::ItemDone { item_id, done } => {
                let target = (list_id, item_id.as_str());
                if removed.contains(&target) {
                    continue;
                }
                let Some(item) = items.get_mut(&target) else {
                    ignored += 1;
                    continue;
                };
                if d.key > item.done.0 {
                    item.done = (d.key.clone(), *done, done.then(|| d.pubkey.to_owned()));
                }
                item.updated_at = item.updated_at.max(d.key.0);
                list.updated_at = list.updated_at.max(d.key.0);
            }
            ProjectTodoOpValue::ItemAssignee { item_id, assignee } => {
                let target = (list_id, item_id.as_str());
                if removed.contains(&target) {
                    continue;
                }
                let Some(item) = items.get_mut(&target) else {
                    ignored += 1;
                    continue;
                };
                set_field(&mut item.assignee, &d.key, assignee.clone());
                item.updated_at = item.updated_at.max(d.key.0);
                list.updated_at = list.updated_at.max(d.key.0);
            }
            ProjectTodoOpValue::ItemDue { item_id, due } => {
                let target = (list_id, item_id.as_str());
                if removed.contains(&target) {
                    continue;
                }
                let Some(item) = items.get_mut(&target) else {
                    ignored += 1;
                    continue;
                };
                set_field(&mut item.due, &d.key, due.clone());
                item.updated_at = item.updated_at.max(d.key.0);
                list.updated_at = list.updated_at.max(d.key.0);
            }
            ProjectTodoOpValue::ItemRank { item_id, rank } => {
                let target = (list_id, item_id.as_str());
                if removed.contains(&target) {
                    continue;
                }
                let Some(item) = items.get_mut(&target) else {
                    ignored += 1;
                    continue;
                };
                set_field(&mut item.rank, &d.key, rank.clone());
                item.updated_at = item.updated_at.max(d.key.0);
                list.updated_at = list.updated_at.max(d.key.0);
            }
        }
    }

    // Assemble.
    let mut out_lists: Vec<TodoList> = lists
        .iter()
        .map(|(list_id, state)| {
            let mut open: Vec<(String, TodoItem)> = Vec::new();
            let mut completed: Vec<(OpKey, TodoItem)> = Vec::new();
            for ((owner, item_id), item) in &items {
                if owner != list_id || removed.contains(&(owner, item_id)) {
                    continue;
                }
                let done = item.done.1;
                let row = TodoItem {
                    id: (*item_id).to_owned(),
                    list_id: (*list_id).to_owned(),
                    text: item.text.1.clone(),
                    done,
                    rank: item.rank.1.clone(),
                    assignee: item.assignee.1.clone(),
                    due: item.due.1.clone(),
                    created_at: item.created_at,
                    created_by: item.created_by.clone(),
                    updated_at: item.updated_at,
                    completed_at: done.then_some(item.done.0 .0),
                    completed_by: item.done.2.clone(),
                };
                if done {
                    completed.push((item.done.0.clone(), row));
                } else {
                    open.push((item.rank.1.clone(), row));
                }
            }
            open.sort_by(|a, b| {
                (a.0.as_str(), a.1.id.as_str()).cmp(&(b.0.as_str(), b.1.id.as_str()))
            });
            completed.sort_by(|a, b| b.0.cmp(&a.0));
            TodoList {
                id: (*list_id).to_owned(),
                title: state.title.1.clone(),
                archived: state.archived.1,
                created_at: state.created_at,
                created_by: state.created_by.clone(),
                updated_at: state.updated_at,
                open: open.into_iter().map(|(_, row)| row).collect(),
                completed: completed.into_iter().map(|(_, row)| row).collect(),
            }
        })
        .collect();
    out_lists.sort_by(|a, b| (a.created_at, a.id.as_str()).cmp(&(b.created_at, b.id.as_str())));

    ProjectTodoDigest {
        schema: PROJECT_TODO_DIGEST_SCHEMA.to_owned(),
        project: project.to_owned(),
        ignored,
        lists: out_lists,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[derive(Deserialize)]
    struct Vectors {
        schema: String,
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        name: String,
        project: String,
        events: Vec<TodoFoldEvent>,
        expected: Value,
    }

    #[test]
    fn conformance_vectors_fold_identically() {
        let raw = include_str!("../../../conformance/project-todo-fold/fixtures/fold-vectors.json");
        let vectors: Vectors = serde_json::from_str(raw).expect("vectors parse");
        assert_eq!(vectors.schema, "buzz-project-todo-fold-vectors/v1");
        assert!(!vectors.cases.is_empty());
        for case in vectors.cases {
            let digest = fold_project_todos(&case.project, &case.events);
            let actual = serde_json::to_value(&digest).expect("digest serializes");
            assert_eq!(
                actual,
                case.expected,
                "case {:?}\nactual: {}",
                case.name,
                serde_json::to_string_pretty(&actual).unwrap()
            );
        }
    }

    #[test]
    fn rank_vectors_agree() {
        #[derive(Deserialize)]
        struct RankVectors {
            schema: String,
            between: Vec<(Option<String>, Option<String>, String)>,
            invalid: Vec<String>,
        }
        let raw = include_str!("../../../conformance/project-todo-fold/fixtures/rank-vectors.json");
        let vectors: RankVectors = serde_json::from_str(raw).expect("rank vectors parse");
        assert_eq!(vectors.schema, "buzz-project-todo-rank-vectors/v1");
        for (after, before, expected) in vectors.between {
            let minted = crate::fractional_rank::rank_between(after.as_deref(), before.as_deref())
                .unwrap_or_else(|e| panic!("{after:?}..{before:?}: {e}"));
            assert_eq!(minted, expected, "{after:?}..{before:?}");
        }
        for rank in vectors.invalid {
            assert!(
                crate::fractional_rank::validate_rank(&rank).is_err(),
                "{rank:?} should be invalid"
            );
        }
    }
}
