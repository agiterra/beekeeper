//! The project to-do fold — moved to [`beekeeper_nip_td::fold`] so the
//! Planning plugin folds with the same code; re-exported here unchanged.

pub use beekeeper_nip_td::fold::*;

// What the tests below reached through the module's own imports before the
// fold moved out.
#[cfg(test)]
use crate::kind::KIND_PROJECT_TODO_OP;
#[cfg(test)]
use crate::project_todo::{ProjectTodoOp, ProjectTodoOpValue, TodoStatus, TodoVisibility};
#[cfg(test)]
use serde::Deserialize;
#[cfg(test)]
use std::str::FromStr;

/// One stored event as the fold sees it. (A `From` impl cannot live here:
/// neither type is local to this crate.)
pub fn todo_fold_event(event: &nostr::Event) -> TodoFoldEvent {
    TodoFoldEvent {
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

    const PROJECT: &str =
        "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:p";
    const LIST: &str = "11111111111111111111111111111111";
    const ITEM: &str = "22222222222222222222222222222222";

    fn op_event(n: u64, author: char, created_at: u64, value: ProjectTodoOpValue) -> TodoFoldEvent {
        let op = ProjectTodoOp {
            list_id: LIST.to_owned(),
            visibility: TodoVisibility::Project,
            value,
        };
        TodoFoldEvent {
            id: format!("{n:064x}"),
            pubkey: author.to_string().repeat(64),
            created_at,
            kind: KIND_PROJECT_TODO_OP,
            tags: op.tags(PROJECT),
            content: op.to_content(),
        }
    }

    fn seed_events() -> Vec<TodoFoldEvent> {
        vec![
            op_event(
                1,
                'a',
                1,
                ProjectTodoOpValue::ListCreate {
                    title: "L".into(),
                    visibility: TodoVisibility::Project,
                },
            ),
            op_event(
                2,
                'a',
                1,
                ProjectTodoOpValue::ItemAdd {
                    item_id: ITEM.into(),
                    text: "t".into(),
                    rank: "a0".into(),
                },
            ),
        ]
    }

    fn done_op(done: bool) -> ProjectTodoOpValue {
        ProjectTodoOpValue::ItemDone {
            item_id: ITEM.into(),
            done,
        }
    }

    fn status_op(status: TodoStatus) -> ProjectTodoOpValue {
        ProjectTodoOpValue::ItemStatus {
            item_id: ITEM.into(),
            status,
        }
    }

    fn only_item(digest: &ProjectTodoDigest) -> &TodoItem {
        let list = &digest.lists[0];
        list.open
            .iter()
            .chain(&list.completed)
            .next()
            .expect("the item is listed")
    }

    /// A reader that predates `item.status` decodes it as an unknown op and
    /// drops it; this is that reader.
    fn fold_as_old_reader(events: &[TodoFoldEvent]) -> ProjectTodoDigest {
        let old: Vec<TodoFoldEvent> = events
            .iter()
            .filter(|event| !event.content.contains("\"op\":\"item.status\""))
            .cloned()
            .collect();
        fold_project_todos(PROJECT, &old)
    }

    const STATUSES: [TodoStatus; 6] = [
        TodoStatus::Triage,
        TodoStatus::Backlog,
        TodoStatus::Todo,
        TodoStatus::InProgress,
        TodoStatus::Done,
        TodoStatus::Canceled,
    ];

    /// Any interleaving of `item.done` and `item.status` from any writers —
    /// including a writer whose second op was refused, or whose read was
    /// stale — leaves a reader that knows `item.status` and one that does
    /// not agreeing on whether the item is done, when, and by whom.
    #[test]
    fn readers_of_different_ages_agree_on_done_under_any_interleaving() {
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = |bound: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % bound
        };
        for _ in 0..2000 {
            let mut events = seed_events();
            let count = 1 + next(8);
            for n in 0..count {
                let author = if next(2) == 0 { 'b' } else { 'c' };
                // Ties on created_at are deliberate: they break on id.
                let created_at = 1 + next(6);
                let value = match next(3) {
                    0 => done_op(next(2) == 0),
                    _ => status_op(STATUSES[next(6) as usize]),
                };
                events.push(op_event(100 + n * 7 + next(5), author, created_at, value));
            }
            let new = fold_project_todos(PROJECT, &events);
            let old = fold_as_old_reader(&events);
            let (new_item, old_item) = (only_item(&new), only_item(&old));
            assert_eq!(new_item.done, old_item.done, "{events:#?}");
            assert_eq!(new_item.completed_at, old_item.completed_at, "{events:#?}");
            assert_eq!(new_item.completed_by, old_item.completed_by, "{events:#?}");
            let status = TodoStatus::from_str(&new_item.status).expect("status");
            assert_eq!(status.is_closed(), new_item.done, "{events:#?}");
        }
    }

    /// B read the item as Done; A reopened it; B, still reading Done, sets
    /// Canceled. B's writer always publishes `item.done` before
    /// `item.status`, so B's later intent wins for both readers. Had B
    /// published only the status (its stale read said no boundary was
    /// crossed), the status would disagree with the newer `item.done{false}`
    /// and both readers would show the item open — never one each way.
    #[test]
    fn a_stale_status_write_never_splits_old_and_new_readers() {
        let mut base = seed_events();
        base.push(op_event(10, 'b', 2, done_op(true)));
        base.push(op_event(11, 'b', 3, status_op(TodoStatus::Done)));
        base.push(op_event(12, 'a', 4, done_op(false)));

        let mut paired = base.clone();
        paired.push(op_event(13, 'b', 5, done_op(true)));
        paired.push(op_event(14, 'b', 6, status_op(TodoStatus::Canceled)));
        let new = fold_project_todos(PROJECT, &paired);
        assert_eq!(only_item(&new).status, "canceled");
        assert!(only_item(&new).done && only_item(&fold_as_old_reader(&paired)).done);

        let mut status_only = base;
        status_only.push(op_event(13, 'b', 5, status_op(TodoStatus::Canceled)));
        let new = fold_project_todos(PROJECT, &status_only);
        assert_eq!(only_item(&new).status, "todo");
        assert!(!only_item(&new).done && !only_item(&fold_as_old_reader(&status_only)).done);
    }

    /// The pair's second op refused (an older relay rejects `item.status`
    /// outright): the item reads what `item.done` says, for every reader.
    #[test]
    fn a_refused_status_leaves_what_item_done_says() {
        let mut events = seed_events();
        events.push(op_event(10, 'b', 2, status_op(TodoStatus::InProgress)));
        events.push(op_event(11, 'b', 3, done_op(true)));
        let new = fold_project_todos(PROJECT, &events);
        assert_eq!(only_item(&new).status, "done");
        assert_eq!(only_item(&new).completed_at, Some(3));
        events.push(op_event(12, 'b', 4, done_op(false)));
        // Reopened by a checkbox: the older in-progress status does not come back.
        assert_eq!(
            only_item(&fold_project_todos(PROJECT, &events)).status,
            "todo"
        );
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
