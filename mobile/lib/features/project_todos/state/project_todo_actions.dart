import 'dart:math';

import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:uuid/uuid.dart';

import '../../../shared/relay/relay.dart';
import '../domain/fractional_rank.dart';
import '../domain/project_todo_fold.dart';
import '../domain/project_todo_op.dart';
import 'project_todos_provider.dart';

const _uuid = Uuid();

/// A fresh 32-lowercase-hex list or item id.
String newProjectTodoId() => _uuid.v4().replaceAll('-', '');

/// A write the writer's own clock rule could not make: the bump past the
/// latest seen op would land outside the relay's 900 s window. Surfaced
/// as a retry, never silently dropped (NIP-TD § Timestamps).
class ProjectTodoClockError implements Exception {
  final String message;
  const ProjectTodoClockError(this.message);

  @override
  String toString() => message;
}

/// Publishes kind 44248 ops for one project.
///
/// Every method signs and sends one op and waits for the relay's OK. A
/// refusal (a viewer of a private project, an unknown coordinate) arrives as
/// an [Exception] carrying the relay's message verbatim; the page shows it
/// as it came. Mobile cannot resolve roster roles, so nothing here guesses
/// at write access — the relay is the authority and its answer is disclosed.
class ProjectTodoActions {
  final String address;
  final SignedEventRelay _relay;
  final ProjectTodosNotifier _read;
  final DateTime Function() _now;

  ProjectTodoActions({
    required this.address,
    required SignedEventRelay relay,
    required ProjectTodosNotifier read,
    DateTime Function()? now,
  }) : _relay = relay,
       _read = read,
       _now = now ?? DateTime.now;

  /// `max(now, latest seen op on the same target + 1)`, refused rather than
  /// clamped when that would exceed the relay's window.
  int _stamp(String listId, String? itemId) {
    final now = _now().millisecondsSinceEpoch ~/ 1000;
    final latest = _read.latestSeenFor(listId, itemId);
    final stamped = latest == null ? now : max(now, latest + 1);
    if (stamped - now > projectTodoClockSkew.inSeconds) {
      throw const ProjectTodoClockError(
        'This item was last changed too far in the future for this device '
        'to write after it; try again shortly.',
      );
    }
    return stamped;
  }

  Future<void> _publish(ProjectTodoOp op) async {
    validateProjectTodoOpForSend(op);
    await _relay.submit(
      kind: EventKind.projectTodoOp,
      content: op.toContent(),
      tags: op.tags(address),
      createdAt: _stamp(op.listId, op.itemId),
    );
  }

  /// Create a list; returns its id.
  Future<String> createList(String title) async {
    final listId = newProjectTodoId();
    await _publish(ProjectTodoOp.listCreate(listId: listId, title: title));
    return listId;
  }

  Future<void> retitleList(String listId, String title) =>
      _publish(ProjectTodoOp.listTitle(listId: listId, title: title));

  Future<void> setListArchived(String listId, bool archived) =>
      _publish(ProjectTodoOp.listArchived(listId: listId, archived: archived));

  /// Add an item after the last open item of [list]; returns its id.
  Future<String> addItem(TodoList list, String text) async {
    final itemId = newProjectTodoId();
    final last = list.open.isEmpty ? null : list.open.last.rank;
    await _publish(
      ProjectTodoOp.itemAdd(
        listId: list.id,
        itemId: itemId,
        text: text,
        rank: rankBetween(last, null),
      ),
    );
    return itemId;
  }

  Future<void> setText(String listId, String itemId, String text) => _publish(
    ProjectTodoOp.itemText(listId: listId, itemId: itemId, text: text),
  );

  Future<void> setDone(String listId, String itemId, bool done) => _publish(
    ProjectTodoOp.itemDone(listId: listId, itemId: itemId, done: done),
  );

  Future<void> setAssignee(String listId, String itemId, String? assignee) =>
      _publish(
        ProjectTodoOp.itemAssignee(
          listId: listId,
          itemId: itemId,
          assignee: assignee?.toLowerCase(),
        ),
      );

  Future<void> setDue(String listId, String itemId, String? due) =>
      _publish(ProjectTodoOp.itemDue(listId: listId, itemId: itemId, due: due));

  /// Move the open item at [oldIndex] of [list] to [newIndex], in
  /// [ReorderableListView.onReorder] terms (the new index counts the item's
  /// own old slot). One `item.rank` op, minted between the new neighbours.
  Future<void> moveItem(TodoList list, int oldIndex, int newIndex) async {
    final open = list.open;
    if (oldIndex < 0 || oldIndex >= open.length) return;
    var target = newIndex;
    if (target > oldIndex) target -= 1;
    if (target == oldIndex) return;
    final remaining = [
      for (var i = 0; i < open.length; i++)
        if (i != oldIndex) open[i],
    ];
    final after = target > 0 ? remaining[target - 1].rank : null;
    final before = target < remaining.length ? remaining[target].rank : null;
    final moved = open[oldIndex];
    await _publish(
      ProjectTodoOp.itemRank(
        listId: list.id,
        itemId: moved.id,
        rank: rankBetween(after, before),
      ),
    );
  }

  Future<void> removeItem(String listId, String itemId) =>
      _publish(ProjectTodoOp.itemRemove(listId: listId, itemId: itemId));
}

/// Run the wire validator over an op about to be sent, so a blank title or
/// a malformed date is refused here, in the validator's words, before it
/// costs a relay round trip.
void validateProjectTodoOpForSend(ProjectTodoOp op) {
  decodeProjectTodoOp(op.toContent());
}

/// The publisher for one project.
final projectTodoActionsProvider = Provider.family<ProjectTodoActions, String>((
  ref,
  address,
) {
  final config = ref.watch(relayConfigProvider);
  final session = ref.read(relaySessionProvider.notifier);
  return ProjectTodoActions(
    address: address,
    relay: SignedEventRelay(session: session, nsec: config.nsec),
    read: ref.read(projectTodosProvider(address).notifier),
  );
});
