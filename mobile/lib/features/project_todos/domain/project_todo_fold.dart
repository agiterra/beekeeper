import 'package:flutter/foundation.dart';

import '../../../shared/relay/nostr_models.dart';
import 'project_todo_op.dart';

/// The project to-do fold: from a bag of kind 44248 ops to the lists and
/// items a reader sees.
///
/// A port of `crates/beekeeper-core/src/project_todo_fold.rs`, bound to
/// `conformance/project-todo-fold/CONTRACT.md` and pinned by
/// `fixtures/fold-vectors.json`. The fold is pure and total: any set of
/// events in, one digest out, the same digest from every client. See the
/// contract for the rules; they are applied here in the order it lists them.

/// Exact `schema` value carried by a digest.
const projectTodoDigestSchema = 'buzz-project-todo-digest/v1';

/// One item in the digest.
@immutable
class TodoItem {
  final String id;
  final String listId;
  final String text;
  final bool done;

  /// Rank among the open items.
  final String rank;

  /// Assignee pubkey, if any.
  final String? assignee;

  /// Due date `YYYY-MM-DD`, if any.
  final String? due;

  /// The winning `item.add`'s `created_at`.
  final int createdAt;

  /// The winning `item.add`'s author.
  final String createdBy;

  /// The greatest applied op `created_at`.
  final int updatedAt;

  /// When the winning `item.done{true}` was stamped, if done.
  final int? completedAt;

  /// Who stamped it, if done.
  final String? completedBy;

  const TodoItem({
    required this.id,
    required this.listId,
    required this.text,
    required this.done,
    required this.rank,
    required this.assignee,
    required this.due,
    required this.createdAt,
    required this.createdBy,
    required this.updatedAt,
    required this.completedAt,
    required this.completedBy,
  });

  /// The contract's item shape; every nullable member is emitted as `null`.
  Map<String, Object?> toJson() => {
    'id': id,
    'listId': listId,
    'text': text,
    'done': done,
    'rank': rank,
    'assignee': assignee,
    'due': due,
    'createdAt': createdAt,
    'createdBy': createdBy,
    'updatedAt': updatedAt,
    'completedAt': completedAt,
    'completedBy': completedBy,
  };
}

/// One list in the digest.
@immutable
class TodoList {
  final String id;
  final String title;

  /// Who reads the list, fixed by its `list.create`.
  final TodoVisibility visibility;
  final bool archived;

  /// Shown in the project tree. Shared by every member of a project list;
  /// a personal list's pin is only ever seen by its owner.
  final bool pinned;

  /// The winning `list.create`'s `created_at`.
  final int createdAt;

  /// The winning `list.create`'s author.
  final String createdBy;

  /// The greatest applied op `created_at` over the list and its items.
  final int updatedAt;

  /// Not-done items, by `(rank, id)`.
  final List<TodoItem> open;

  /// Done items, most recently completed first.
  final List<TodoItem> completed;

  const TodoList({
    required this.id,
    required this.title,
    required this.visibility,
    required this.archived,
    required this.pinned,
    required this.createdAt,
    required this.createdBy,
    required this.updatedAt,
    required this.open,
    required this.completed,
  });

  /// `true` for a list only its creator reads.
  bool get personal => visibility == TodoVisibility.personal;

  /// The contract's list shape.
  Map<String, Object?> toJson() => {
    'id': id,
    'title': title,
    'visibility': visibility.wire,
    'archived': archived,
    'pinned': pinned,
    'createdAt': createdAt,
    'createdBy': createdBy,
    'updatedAt': updatedAt,
    'open': [for (final item in open) item.toJson()],
    'completed': [for (final item in completed) item.toJson()],
  };
}

/// The fold's output.
@immutable
class ProjectTodoDigest {
  /// The project coordinate the ops were folded for.
  final String project;

  /// Events that did not decode or named no existing target — the fold's
  /// honesty counter. A client shows it when it is not zero rather than
  /// presenting a list that silently dropped somebody's write.
  final int ignored;

  /// Lists, by `(createdAt, id)`.
  final List<TodoList> lists;

  const ProjectTodoDigest({
    required this.project,
    required this.ignored,
    required this.lists,
  });

  /// The digest of a project nothing has been read for.
  const ProjectTodoDigest.empty(this.project) : ignored = 0, lists = const [];

  /// The list with [id], or `null`.
  TodoList? listById(String id) {
    for (final list in lists) {
      if (list.id == id) return list;
    }
    return null;
  }

  /// The contract's digest shape (`buzz-project-todo-digest/v1`).
  Map<String, Object?> toJson() => {
    'schema': projectTodoDigestSchema,
    'project': project,
    'ignored': ignored,
    'lists': [for (final list in lists) list.toJson()],
  };
}

/// `(created_at, id)` — the fold's only clock. Ids compare as code units,
/// which for lowercase hex is the contract's bytewise order.
@immutable
class _OpKey implements Comparable<_OpKey> {
  final int createdAt;
  final String id;

  const _OpKey(this.createdAt, this.id);

  @override
  int compareTo(_OpKey other) {
    if (createdAt != other.createdAt) {
      return createdAt.compareTo(other.createdAt);
    }
    return id.compareTo(other.id);
  }

  bool operator >(_OpKey other) => compareTo(other) > 0;
}

class _Field<T> {
  _OpKey key;
  T value;
  _Field(this.key, this.value);

  void set(_OpKey candidate, T next) {
    if (candidate > key) {
      key = candidate;
      value = next;
    }
  }
}

class _ListState {
  final _Field<String> title;
  final TodoVisibility visibility;
  final _Field<bool> archived;
  final _Field<bool> pinned;
  final int createdAt;
  final String createdBy;
  int updatedAt;

  _ListState({
    required this.title,
    required this.visibility,
    required this.archived,
    required this.pinned,
    required this.createdAt,
    required this.createdBy,
    required this.updatedAt,
  });

  void touch(int at) {
    if (at > updatedAt) updatedAt = at;
  }
}

class _ItemState {
  final _Field<String> text;
  final _Field<String> rank;
  _OpKey doneKey;
  bool done;
  String? completedBy;
  final _Field<String?> assignee;
  final _Field<String?> due;
  final int createdAt;
  final String createdBy;
  int updatedAt;

  _ItemState({
    required this.text,
    required this.rank,
    required this.doneKey,
    required this.assignee,
    required this.due,
    required this.createdAt,
    required this.createdBy,
    required this.updatedAt,
  }) : done = false,
       completedBy = null;

  void touch(int at) {
    if (at > updatedAt) updatedAt = at;
  }
}

class _Decoded {
  final _OpKey key;
  final String pubkey;
  final ProjectTodoOp op;
  const _Decoded(this.key, this.pubkey, this.op);
}

String _itemKey(String listId, String itemId) => '$listId/$itemId';

/// Fold [events] for [project] into a digest. [project] must already be a
/// canonical coordinate; events naming any other coordinate are ignored.
ProjectTodoDigest foldProjectTodos(
  String project,
  Iterable<NostrEvent> events,
) {
  var ignored = 0;
  final seen = <String>{};
  final ops = <_Decoded>[];
  for (final event in events) {
    if (!seen.add(event.id)) continue;
    final op = decodeProjectTodoEvent(event, project);
    if (op == null) {
      ignored++;
      continue;
    }
    ops.add(_Decoded(_OpKey(event.createdAt, event.id), event.pubkey, op));
  }
  ops.sort((a, b) => a.key.compareTo(b.key));

  // Creates, earliest first.
  final lists = <String, _ListState>{};
  for (final d in ops) {
    if (d.op.kind != ProjectTodoOpKind.listCreate) continue;
    if (lists.containsKey(d.op.listId)) {
      ignored++;
      continue;
    }
    lists[d.op.listId] = _ListState(
      title: _Field(d.key, d.op.title!),
      visibility: d.op.visibility,
      archived: _Field(d.key, false),
      pinned: _Field(d.key, false),
      createdAt: d.key.createdAt,
      createdBy: d.pubkey,
      updatedAt: d.key.createdAt,
    );
  }
  // Every other op must agree with its list's visibility, and a personal
  // list takes ops from its creator only. Ops on a list that does not exist
  // are counted where they are handled below; these are the ops on existing
  // lists that are refused on visibility grounds.
  ops.removeWhere((d) {
    if (d.op.kind == ProjectTodoOpKind.listCreate) return false;
    final list = lists[d.op.listId];
    if (list == null) return false;
    final admitted =
        d.op.visibility == list.visibility &&
        (list.visibility == TodoVisibility.project ||
            d.pubkey == list.createdBy);
    if (!admitted) ignored++;
    return !admitted;
  });
  final items = <String, _ItemState>{};
  final removed = <String>{};
  for (final d in ops) {
    if (d.op.kind != ProjectTodoOpKind.itemAdd) continue;
    final list = lists[d.op.listId];
    if (list == null) {
      ignored++;
      continue;
    }
    final target = _itemKey(d.op.listId, d.op.itemId!);
    if (items.containsKey(target)) {
      ignored++;
      continue;
    }
    list.touch(d.key.createdAt);
    items[target] = _ItemState(
      text: _Field(d.key, d.op.text!),
      rank: _Field(d.key, d.op.rank!),
      doneKey: d.key,
      assignee: _Field(d.key, null),
      due: _Field(d.key, null),
      createdAt: d.key.createdAt,
      createdBy: d.pubkey,
      updatedAt: d.key.createdAt,
    );
  }
  // Removes are terminal, whenever they were stamped.
  for (final d in ops) {
    if (d.op.kind != ProjectTodoOpKind.itemRemove) continue;
    final target = _itemKey(d.op.listId, d.op.itemId!);
    if (items.containsKey(target)) {
      removed.add(target);
      lists[d.op.listId]?.touch(d.key.createdAt);
    } else {
      ignored++;
    }
  }

  // Field writes, latest key per field.
  for (final d in ops) {
    final list = lists[d.op.listId];
    if (list == null) {
      // Creates and removes on a missing list were already counted in their
      // own passes; every other op on a missing list is counted here.
      if (d.op.kind != ProjectTodoOpKind.listCreate &&
          d.op.kind != ProjectTodoOpKind.itemAdd &&
          d.op.kind != ProjectTodoOpKind.itemRemove) {
        ignored++;
      }
      continue;
    }
    switch (d.op.kind) {
      case ProjectTodoOpKind.listCreate:
      case ProjectTodoOpKind.itemAdd:
      case ProjectTodoOpKind.itemRemove:
        continue;
      case ProjectTodoOpKind.listTitle:
        list.title.set(d.key, d.op.title!);
        list.touch(d.key.createdAt);
        continue;
      case ProjectTodoOpKind.listArchived:
        list.archived.set(d.key, d.op.archived!);
        list.touch(d.key.createdAt);
        continue;
      case ProjectTodoOpKind.listPinned:
        list.pinned.set(d.key, d.op.pinned!);
        list.touch(d.key.createdAt);
        continue;
      case ProjectTodoOpKind.itemText:
      case ProjectTodoOpKind.itemDone:
      case ProjectTodoOpKind.itemAssignee:
      case ProjectTodoOpKind.itemDue:
      case ProjectTodoOpKind.itemRank:
        break;
    }
    final target = _itemKey(d.op.listId, d.op.itemId!);
    if (removed.contains(target)) continue;
    final item = items[target];
    if (item == null) {
      ignored++;
      continue;
    }
    switch (d.op.kind) {
      case ProjectTodoOpKind.itemText:
        item.text.set(d.key, d.op.text!);
      case ProjectTodoOpKind.itemDone:
        if (d.key > item.doneKey) {
          item.doneKey = d.key;
          item.done = d.op.done!;
          item.completedBy = d.op.done! ? d.pubkey : null;
        }
      case ProjectTodoOpKind.itemAssignee:
        item.assignee.set(d.key, d.op.assignee);
      case ProjectTodoOpKind.itemDue:
        item.due.set(d.key, d.op.due);
      case ProjectTodoOpKind.itemRank:
        item.rank.set(d.key, d.op.rank!);
      default:
        break;
    }
    item.touch(d.key.createdAt);
    list.touch(d.key.createdAt);
  }

  // Assemble.
  final out = <TodoList>[];
  for (final entry in lists.entries) {
    final listId = entry.key;
    final state = entry.value;
    final open = <(String, TodoItem)>[];
    final completed = <(_OpKey, TodoItem)>[];
    for (final itemEntry in items.entries) {
      final item = itemEntry.value;
      final slash = itemEntry.key.indexOf('/');
      final owner = itemEntry.key.substring(0, slash);
      final itemId = itemEntry.key.substring(slash + 1);
      if (owner != listId || removed.contains(itemEntry.key)) continue;
      final done = item.done;
      final row = TodoItem(
        id: itemId,
        listId: listId,
        text: item.text.value,
        done: done,
        rank: item.rank.value,
        assignee: item.assignee.value,
        due: item.due.value,
        createdAt: item.createdAt,
        createdBy: item.createdBy,
        updatedAt: item.updatedAt,
        completedAt: done ? item.doneKey.createdAt : null,
        completedBy: item.completedBy,
      );
      if (done) {
        completed.add((item.doneKey, row));
      } else {
        open.add((item.rank.value, row));
      }
    }
    open.sort((a, b) {
      final byRank = a.$1.compareTo(b.$1);
      return byRank != 0 ? byRank : a.$2.id.compareTo(b.$2.id);
    });
    completed.sort((a, b) => b.$1.compareTo(a.$1));
    out.add(
      TodoList(
        id: listId,
        title: state.title.value,
        visibility: state.visibility,
        archived: state.archived.value,
        pinned: state.pinned.value,
        createdAt: state.createdAt,
        createdBy: state.createdBy,
        updatedAt: state.updatedAt,
        open: [for (final e in open) e.$2],
        completed: [for (final e in completed) e.$2],
      ),
    );
  }
  out.sort((a, b) {
    if (a.createdAt != b.createdAt) return a.createdAt.compareTo(b.createdAt);
    return a.id.compareTo(b.id);
  });
  return ProjectTodoDigest(project: project, ignored: ignored, lists: out);
}
