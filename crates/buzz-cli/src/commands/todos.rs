//! `bee todos` — a project's shared to-do lists (NIP-TD, kind 44248).
//!
//! Reads fetch every op for the project's coordinate and fold them with
//! `buzz_core::project_todo_fold`, the same fold Desktop and Mobile bind to
//! through `conformance/project-todo-fold/`. Writes publish one op per
//! field: `add --assignee --due` is therefore up to three events, each
//! reported, because an op that carried the whole row would silently
//! overwrite the field somebody else just changed.
//!
//! Every write reads the current fold first, both to resolve the id or
//! prefix the caller typed and to stamp `created_at` past the latest op on
//! the same target (`next_replaceable_created_at`), so a write made after
//! looking at the list wins the field even on a slightly slow clock.

use buzz_core::fractional_rank::rank_between;
use buzz_core::kind::KIND_PROJECT_TODO_OP;
use buzz_core::project_todo::{ProjectTodoOp, ProjectTodoOpValue, TodoVisibility};
use buzz_core::project_todo_fold::{
    fold_project_todos, ProjectTodoDigest, TodoFoldEvent, TodoItem, TodoList,
};
use nostr::Timestamp;
use serde_json::{json, Value};
use std::str::FromStr;

use super::pulse::resolve_project;
use super::repos::next_replaceable_created_at;
use crate::client::BuzzClient;
use crate::error::CliError;

/// The relay's ingest window is ±900 s; a bump that would land past this
/// margin is refused here rather than by the relay.
const MAX_FUTURE_SKEW_SECS: u64 = 890;

/// Everything a command needs about the project's current lists.
struct Snapshot {
    coordinate: String,
    digest: ProjectTodoDigest,
    /// Latest `created_at` per `(list, item)` target across every decoded
    /// op, for the write bump. `item == ""` is the list itself.
    latest: std::collections::HashMap<(String, String), u64>,
}

async fn snapshot(client: &BuzzClient, project: Option<&str>) -> Result<Snapshot, CliError> {
    let coordinate = resolve_project(client, project).await?;
    let raw = client
        .query_all(json!({ "kinds": [KIND_PROJECT_TODO_OP], "#a": [coordinate] }))
        .await?;
    let mut events = Vec::with_capacity(raw.len());
    let mut latest = std::collections::HashMap::new();
    for value in raw {
        let Ok(event) = serde_json::from_value::<TodoFoldEvent>(value) else {
            continue;
        };
        let visibility = event
            .tags
            .iter()
            .find(|t| t.first().map(String::as_str) == Some("td-vis"))
            .and_then(|t| t.get(1))
            .and_then(|v| TodoVisibility::from_str(v).ok());
        if let Some(op) = visibility.and_then(|vis| {
            buzz_core::project_todo::decode_project_todo_op(&event.content, vis).ok()
        }) {
            let key = (
                op.list_id.clone(),
                op.item_id().unwrap_or_default().to_owned(),
            );
            let slot = latest.entry(key).or_insert(0);
            *slot = (*slot).max(event.created_at);
            // A list-level op also counts as activity on the list target.
            let list_key = (op.list_id.clone(), String::new());
            let slot = latest.entry(list_key).or_insert(0);
            *slot = (*slot).max(event.created_at);
        }
        events.push(event);
    }
    let digest = fold_project_todos(&coordinate, &events);
    Ok(Snapshot {
        coordinate,
        digest,
        latest,
    })
}

/// Match `key` against an id: exact, or a unique prefix of at least six
/// characters.
fn id_matches(id: &str, key: &str) -> bool {
    id == key || (key.len() >= 6 && id.starts_with(key))
}

fn resolve_list<'a>(digest: &'a ProjectTodoDigest, key: &str) -> Result<&'a TodoList, CliError> {
    let by_id: Vec<&TodoList> = digest
        .lists
        .iter()
        .filter(|list| id_matches(&list.id, key))
        .collect();
    if by_id.len() == 1 {
        return Ok(by_id[0]);
    }
    if by_id.len() > 1 {
        return Err(CliError::Usage(format!(
            "list prefix {key:?} is ambiguous: {}",
            by_id
                .iter()
                .map(|l| l.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let by_title: Vec<&TodoList> = digest
        .lists
        .iter()
        .filter(|list| list.title.eq_ignore_ascii_case(key))
        .collect();
    match by_title.len() {
        1 => Ok(by_title[0]),
        0 => Err(CliError::Usage(format!(
            "no list named or identified by {key:?} in this project"
        ))),
        _ => Err(CliError::Usage(format!(
            "list title {key:?} is ambiguous; pass an id: {}",
            by_title
                .iter()
                .map(|l| l.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn resolve_item<'a>(
    digest: &'a ProjectTodoDigest,
    key: &str,
) -> Result<(&'a TodoList, &'a TodoItem), CliError> {
    let mut hits: Vec<(&TodoList, &TodoItem)> = Vec::new();
    for list in &digest.lists {
        for item in list.open.iter().chain(list.completed.iter()) {
            if id_matches(&item.id, key) {
                hits.push((list, item));
            }
        }
    }
    match hits.len() {
        1 => Ok(hits[0]),
        0 => Err(CliError::Usage(format!(
            "no item identified by {key:?} in this project"
        ))),
        _ => Err(CliError::Usage(format!(
            "item prefix {key:?} is ambiguous: {}",
            hits.iter()
                .map(|(_, i)| i.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// A list's visibility as the fold reported it. The fold only ever emits the
/// two wire spellings, so a parse failure is a bug, reported rather than
/// guessed at.
fn visibility_of(list: &TodoList) -> Result<TodoVisibility, CliError> {
    TodoVisibility::from_str(&list.visibility)
        .map_err(|e| CliError::Other(format!("fold reported an unknown visibility: {e}")))
}

/// Publish one op, stamped past the latest op on its target — including one
/// this same command just published, so `add --assignee` lands its assignee
/// op strictly after its add op and the per-field fold cannot let the add's
/// empty assignee outrank it.
async fn publish(
    snap: &mut Snapshot,
    client: &BuzzClient,
    op: &ProjectTodoOp,
) -> Result<Value, CliError> {
    let target = (
        op.list_id.clone(),
        op.item_id().unwrap_or_default().to_owned(),
    );
    let now = Timestamp::now().as_secs();
    let head = snap.latest.get(&target).copied().unwrap_or(0);
    let created_at = next_replaceable_created_at(head, now)
        .ok_or_else(|| CliError::Other("todo timestamp cannot be advanced".into()))?;
    if created_at > now + MAX_FUTURE_SKEW_SECS {
        return Err(CliError::Other(format!(
            "the latest op on this target is stamped {} s in the future; retry later rather than \
             publishing outside the relay's window",
            head.saturating_sub(now)
        )));
    }
    let builder = buzz_sdk::builders::build_project_todo_op(&snap.coordinate, op)
        .map_err(crate::validate::sdk_err)?
        .custom_created_at(Timestamp::from(created_at));
    // Signed verbatim, not through `sign_event`: the tag grammar is a closed
    // key set, so the NIP-OA `auth` tag that method injects would be rejected
    // at ingest. Membership delegation still travels with the request —
    // `submit_event` attaches the same tag as the `x-auth-tag` header.
    let event = client.sign_event_unchecked(builder)?;
    let event_id = event.id.to_hex();
    let raw = client.submit_event(event).await?;
    let normalized = super::parse_write_response(&raw, "todo op was superseded")?;
    snap.latest.insert(target, created_at);
    let mut response: Value = serde_json::from_str(&normalized).unwrap_or(Value::Null);
    if let Some(object) = response.as_object_mut() {
        object.entry("event_id").or_insert(json!(event_id));
        object.insert("op".into(), json!(op.kind().as_str()));
        object.insert("list_id".into(), json!(op.list_id));
        if let Some(item_id) = op.item_id() {
            object.insert("item_id".into(), json!(item_id));
        }
        object.insert("created_at".into(), json!(created_at));
    }
    Ok(response)
}

fn parse_assignee(value: &str) -> Result<Option<String>, CliError> {
    if value.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let hex = value.to_ascii_lowercase();
    if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(Some(hex));
    }
    Err(CliError::Usage(
        "assignee must be a 64-hex pubkey or `none`".to_owned(),
    ))
}

fn parse_due(value: &str) -> Result<Option<String>, CliError> {
    if value.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    buzz_core::project_todo::validate_due_date(value).map_err(CliError::Usage)?;
    Ok(Some(value.to_owned()))
}

/// The rank that places an item at `index` among `open` (0 = first;
/// `None` = last). `moving` is excluded from the neighbour computation so
/// an item can be moved within its own list.
fn rank_for_index(
    open: &[TodoItem],
    index: Option<usize>,
    moving: Option<&str>,
) -> Result<String, CliError> {
    let others: Vec<&TodoItem> = open
        .iter()
        .filter(|item| Some(item.id.as_str()) != moving)
        .collect();
    let index = index.unwrap_or(others.len()).min(others.len());
    let after = index.checked_sub(1).map(|i| others[i].rank.as_str());
    let before = others.get(index).map(|item| item.rank.as_str());
    rank_between(after, before).map_err(CliError::Other)
}

fn list_summary(list: &TodoList, format: &crate::OutputFormat) -> Value {
    match format {
        crate::OutputFormat::Compact => json!({
            "id": list.id,
            "title": list.title,
            "visibility": list.visibility,
            "archived": list.archived,
            "pinned": list.pinned,
            "open": list.open.len(),
            "completed": list.completed.len(),
        }),
        crate::OutputFormat::Json => json!({
            "id": list.id,
            "title": list.title,
            "visibility": list.visibility,
            "archived": list.archived,
            "pinned": list.pinned,
            "open": list.open.len(),
            "completed": list.completed.len(),
            "createdAt": list.created_at,
            "createdBy": list.created_by,
            "updatedAt": list.updated_at,
        }),
    }
}

fn item_row(item: &TodoItem, format: &crate::OutputFormat) -> Value {
    match format {
        crate::OutputFormat::Compact => json!({
            "id": item.id,
            "text": item.text,
            "done": item.done,
            "assignee": item.assignee,
            "due": item.due,
        }),
        crate::OutputFormat::Json => serde_json::to_value(item).unwrap_or(Value::Null),
    }
}

async fn cmd_lists(
    client: &BuzzClient,
    project: Option<&str>,
    archived: bool,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let snap = snapshot(client, project).await?;
    let rows: Vec<Value> = snap
        .digest
        .lists
        .iter()
        .filter(|list| archived || !list.archived)
        .map(|list| list_summary(list, format))
        .collect();
    let mut out = json!({ "project": snap.coordinate, "lists": rows });
    if snap.digest.ignored > 0 {
        out["ignored"] = json!(snap.digest.ignored);
    }
    println!("{out}");
    Ok(())
}

async fn cmd_show(
    client: &BuzzClient,
    project: Option<&str>,
    list: &str,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let snap = snapshot(client, project).await?;
    let list = resolve_list(&snap.digest, list)?;
    let mut out = json!({
        "project": snap.coordinate,
        "id": list.id,
        "title": list.title,
        "visibility": list.visibility,
        "archived": list.archived,
        "pinned": list.pinned,
        "open": list.open.iter().map(|i| item_row(i, format)).collect::<Vec<_>>(),
        "completed": list.completed.iter().map(|i| item_row(i, format)).collect::<Vec<_>>(),
    });
    if matches!(format, crate::OutputFormat::Json) {
        out["createdAt"] = json!(list.created_at);
        out["createdBy"] = json!(list.created_by);
        out["updatedAt"] = json!(list.updated_at);
    }
    if snap.digest.ignored > 0 {
        out["ignored"] = json!(snap.digest.ignored);
    }
    println!("{out}");
    Ok(())
}

/// Dispatch `bee todos`.
pub async fn dispatch(
    cmd: crate::TodosCmd,
    client: &BuzzClient,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    use crate::TodosCmd;
    match cmd {
        TodosCmd::Lists { project, archived } => {
            cmd_lists(client, project.as_deref(), archived, format).await
        }
        TodosCmd::Show { project, list } => {
            cmd_show(client, project.as_deref(), &list, format).await
        }
        TodosCmd::CreateList {
            project,
            title,
            personal,
            pinned,
        } => {
            let mut snap = snapshot(client, project.as_deref()).await?;
            let visibility = if personal {
                TodoVisibility::Personal
            } else {
                TodoVisibility::Project
            };
            let list_id = new_id();
            let created = publish(
                &mut snap,
                client,
                &ProjectTodoOp {
                    list_id: list_id.clone(),
                    visibility,
                    value: ProjectTodoOpValue::ListCreate { title, visibility },
                },
            )
            .await?;
            if pinned {
                let pin = publish(
                    &mut snap,
                    client,
                    &ProjectTodoOp {
                        list_id,
                        visibility,
                        value: ProjectTodoOpValue::ListPinned { pinned: true },
                    },
                )
                .await?;
                let mut out = created;
                out["ops"] = json!([out.clone(), pin]);
                println!("{out}");
            } else {
                println!("{created}");
            }
            Ok(())
        }
        TodosCmd::RenameList {
            project,
            list,
            title,
        } => {
            let mut snap = snapshot(client, project.as_deref()).await?;
            let target = resolve_list(&snap.digest, &list)?;
            let op = ProjectTodoOp {
                list_id: target.id.clone(),
                visibility: visibility_of(target)?,
                value: ProjectTodoOpValue::ListTitle { title },
            };
            println!("{}", publish(&mut snap, client, &op).await?);
            Ok(())
        }
        TodosCmd::ArchiveList {
            project,
            list,
            undo,
        } => {
            let mut snap = snapshot(client, project.as_deref()).await?;
            let target = resolve_list(&snap.digest, &list)?;
            let op = ProjectTodoOp {
                list_id: target.id.clone(),
                visibility: visibility_of(target)?,
                value: ProjectTodoOpValue::ListArchived { archived: !undo },
            };
            println!("{}", publish(&mut snap, client, &op).await?);
            Ok(())
        }
        TodosCmd::Pin { project, list } => {
            set_pinned(client, project.as_deref(), &list, true).await
        }
        TodosCmd::Unpin { project, list } => {
            set_pinned(client, project.as_deref(), &list, false).await
        }
        TodosCmd::Add {
            project,
            list,
            text,
            assignee,
            due,
            index,
        } => {
            let assignee = assignee
                .as_deref()
                .map(parse_assignee)
                .transpose()?
                .flatten();
            let due = due.as_deref().map(parse_due).transpose()?.flatten();
            let mut snap = snapshot(client, project.as_deref()).await?;
            let target = resolve_list(&snap.digest, &list)?;
            let list_id = target.id.clone();
            let visibility = visibility_of(target)?;
            let rank = rank_for_index(&target.open, index, None)?;
            let item_id = new_id();
            let mut published = vec![
                publish(
                    &mut snap,
                    client,
                    &ProjectTodoOp {
                        list_id: list_id.clone(),
                        visibility,
                        value: ProjectTodoOpValue::ItemAdd {
                            item_id: item_id.clone(),
                            text,
                            rank,
                        },
                    },
                )
                .await?,
            ];
            if let Some(assignee) = assignee {
                published.push(
                    publish(
                        &mut snap,
                        client,
                        &ProjectTodoOp {
                            list_id: list_id.clone(),
                            visibility,
                            value: ProjectTodoOpValue::ItemAssignee {
                                item_id: item_id.clone(),
                                assignee: Some(assignee),
                            },
                        },
                    )
                    .await?,
                );
            }
            if let Some(due) = due {
                published.push(
                    publish(
                        &mut snap,
                        client,
                        &ProjectTodoOp {
                            list_id: list_id.clone(),
                            visibility,
                            value: ProjectTodoOpValue::ItemDue {
                                item_id: item_id.clone(),
                                due: Some(due),
                            },
                        },
                    )
                    .await?,
                );
            }
            let first = published.first().cloned().unwrap_or(Value::Null);
            let mut out = first;
            if published.len() > 1 {
                out["ops"] = json!(published);
            }
            println!("{out}");
            Ok(())
        }
        TodosCmd::Edit {
            project,
            item,
            text,
        } => {
            let mut snap = snapshot(client, project.as_deref()).await?;
            let (list, item) = resolve_item(&snap.digest, &item)?;
            let op = ProjectTodoOp {
                list_id: list.id.clone(),
                visibility: visibility_of(list)?,
                value: ProjectTodoOpValue::ItemText {
                    item_id: item.id.clone(),
                    text,
                },
            };
            println!("{}", publish(&mut snap, client, &op).await?);
            Ok(())
        }
        TodosCmd::Done { project, item } => set_done(client, project.as_deref(), &item, true).await,
        TodosCmd::Undone { project, item } => {
            set_done(client, project.as_deref(), &item, false).await
        }
        TodosCmd::Assign {
            project,
            item,
            assignee,
        } => {
            let assignee = parse_assignee(&assignee)?;
            let mut snap = snapshot(client, project.as_deref()).await?;
            let (list, item) = resolve_item(&snap.digest, &item)?;
            let op = ProjectTodoOp {
                list_id: list.id.clone(),
                visibility: visibility_of(list)?,
                value: ProjectTodoOpValue::ItemAssignee {
                    item_id: item.id.clone(),
                    assignee,
                },
            };
            println!("{}", publish(&mut snap, client, &op).await?);
            Ok(())
        }
        TodosCmd::Due { project, item, due } => {
            let due = parse_due(&due)?;
            let mut snap = snapshot(client, project.as_deref()).await?;
            let (list, item) = resolve_item(&snap.digest, &item)?;
            let op = ProjectTodoOp {
                list_id: list.id.clone(),
                visibility: visibility_of(list)?,
                value: ProjectTodoOpValue::ItemDue {
                    item_id: item.id.clone(),
                    due,
                },
            };
            println!("{}", publish(&mut snap, client, &op).await?);
            Ok(())
        }
        TodosCmd::Move {
            project,
            item,
            index,
        } => {
            let mut snap = snapshot(client, project.as_deref()).await?;
            let (list, item) = resolve_item(&snap.digest, &item)?;
            if item.done {
                return Err(CliError::Usage(
                    "a completed item has no position; `undone` it first".to_owned(),
                ));
            }
            let rank = rank_for_index(&list.open, Some(index), Some(&item.id))?;
            let op = ProjectTodoOp {
                list_id: list.id.clone(),
                visibility: visibility_of(list)?,
                value: ProjectTodoOpValue::ItemRank {
                    item_id: item.id.clone(),
                    rank,
                },
            };
            println!("{}", publish(&mut snap, client, &op).await?);
            Ok(())
        }
        TodosCmd::Remove { project, item } => {
            let mut snap = snapshot(client, project.as_deref()).await?;
            let (list, item) = resolve_item(&snap.digest, &item)?;
            let op = ProjectTodoOp {
                list_id: list.id.clone(),
                visibility: visibility_of(list)?,
                value: ProjectTodoOpValue::ItemRemove {
                    item_id: item.id.clone(),
                },
            };
            println!("{}", publish(&mut snap, client, &op).await?);
            Ok(())
        }
    }
}

async fn set_done(
    client: &BuzzClient,
    project: Option<&str>,
    item: &str,
    done: bool,
) -> Result<(), CliError> {
    let mut snap = snapshot(client, project).await?;
    let (list, item) = resolve_item(&snap.digest, item)?;
    let op = ProjectTodoOp {
        list_id: list.id.clone(),
        visibility: visibility_of(list)?,
        value: ProjectTodoOpValue::ItemDone {
            item_id: item.id.clone(),
            done,
        },
    };
    println!("{}", publish(&mut snap, client, &op).await?);
    Ok(())
}

async fn set_pinned(
    client: &BuzzClient,
    project: Option<&str>,
    list: &str,
    pinned: bool,
) -> Result<(), CliError> {
    let mut snap = snapshot(client, project).await?;
    let target = resolve_list(&snap.digest, list)?;
    let op = ProjectTodoOp {
        list_id: target.id.clone(),
        visibility: visibility_of(target)?,
        value: ProjectTodoOpValue::ListPinned { pinned },
    };
    println!("{}", publish(&mut snap, client, &op).await?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::project_todo::is_todo_id;

    fn item(id: &str, rank: &str, done: bool) -> TodoItem {
        TodoItem {
            id: id.to_owned(),
            list_id: "l".repeat(32),
            text: id.to_owned(),
            done,
            rank: rank.to_owned(),
            assignee: None,
            due: None,
            created_at: 1,
            created_by: "p".repeat(64),
            updated_at: 1,
            completed_at: None,
            completed_by: None,
        }
    }

    fn list(id: &str, title: &str, open: Vec<TodoItem>) -> TodoList {
        TodoList {
            id: id.to_owned(),
            title: title.to_owned(),
            visibility: "project".to_owned(),
            archived: false,
            pinned: false,
            created_at: 1,
            created_by: "p".repeat(64),
            updated_at: 1,
            open,
            completed: Vec::new(),
        }
    }

    fn digest(lists: Vec<TodoList>) -> ProjectTodoDigest {
        ProjectTodoDigest {
            schema: "buzz-project-todo-digest/v1".to_owned(),
            project: "30621:x:y".to_owned(),
            ignored: 0,
            lists,
        }
    }

    #[test]
    fn rank_for_index_places_between_neighbours_and_excludes_the_mover() {
        let open = vec![
            item(&"a".repeat(32), "a0", false),
            item(&"b".repeat(32), "a1", false),
            item(&"c".repeat(32), "a2", false),
        ];
        assert_eq!(rank_for_index(&open, None, None).unwrap(), "a3");
        assert_eq!(rank_for_index(&open, Some(0), None).unwrap(), "Zz");
        assert_eq!(rank_for_index(&open, Some(1), None).unwrap(), "a0V");
        assert_eq!(rank_for_index(&open, Some(99), None).unwrap(), "a3");
        // Moving c to the front: neighbours are (none, a).
        let c = "c".repeat(32);
        assert_eq!(rank_for_index(&open, Some(0), Some(&c)).unwrap(), "Zz");
        // Moving a to the end: neighbours are (c, none).
        let a = "a".repeat(32);
        assert_eq!(rank_for_index(&open, Some(2), Some(&a)).unwrap(), "a3");
        assert_eq!(rank_for_index(&[], Some(0), None).unwrap(), "a0");
    }

    #[test]
    fn lists_resolve_by_id_prefix_or_unique_title() {
        let l1 = "1".repeat(32);
        let l2 = "1".repeat(31) + "2";
        let d = digest(vec![
            list(&l1, "Launch", vec![]),
            list(&l2, "Backlog", vec![]),
        ]);
        assert_eq!(resolve_list(&d, &l1).unwrap().id, l1);
        assert_eq!(resolve_list(&d, "launch").unwrap().title, "Launch");
        assert_eq!(resolve_list(&d, &l2[..32]).unwrap().id, l2);
        assert!(
            resolve_list(&d, "111111").is_err(),
            "shared prefix is ambiguous"
        );
        assert!(resolve_list(&d, "nope").is_err());
        assert!(
            resolve_list(&d, "11111").is_err(),
            "prefix under six chars never matches"
        );
    }

    #[test]
    fn items_resolve_across_lists_including_completed() {
        let i1 = "a".repeat(32);
        let i2 = "b".repeat(32);
        let mut l = list(&"1".repeat(32), "L", vec![item(&i1, "a0", false)]);
        l.completed.push(item(&i2, "a1", true));
        let d = digest(vec![l]);
        assert_eq!(resolve_item(&d, "aaaaaa").unwrap().1.id, i1);
        assert_eq!(resolve_item(&d, &i2).unwrap().1.id, i2);
        assert!(resolve_item(&d, "cccccc").is_err());
    }

    #[test]
    fn assignee_and_due_parse_none() {
        assert_eq!(parse_assignee("none").unwrap(), None);
        assert_eq!(
            parse_assignee(&"A".repeat(64)).unwrap(),
            Some("a".repeat(64))
        );
        assert!(parse_assignee("abc").is_err());
        assert_eq!(parse_due("NONE").unwrap(), None);
        assert_eq!(
            parse_due("2026-12-31").unwrap().as_deref(),
            Some("2026-12-31")
        );
        assert!(parse_due("2026-13-01").is_err());
        assert!(is_todo_id(&new_id()));
    }
}
