import 'dart:math';

import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:uuid/uuid.dart';

import '../../../shared/relay/relay.dart';
import 'package:buzz/shared/utils/fractional_rank.dart';
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

/// A write on a list the current read does not know. Every op repeats its
/// list's visibility as the `td-vis` tag, and that is the list's fact, not
/// the writer's guess — so a list the fold has not shown cannot be written
/// to honestly.
class ProjectTodoUnknownListError implements Exception {
  final String listId;
  const ProjectTodoUnknownListError(this.listId);

  @override
  String toString() =>
      'This list is not in the current read; refresh and try again.';
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

  /// The `created_at` this publisher last stamped per target, so a second
  /// op on the same target (a `list.pinned` right after its `list.create`)
  /// sorts after the first even before the relay echoes it back.
  final Map<String, int> _sent = {};

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
    final target = itemId == null ? listId : '$listId/$itemId';
    final seen = _read.latestSeenFor(listId, itemId);
    final sent = _sent[target];
    final latest = seen == null
        ? sent
        : sent == null
        ? seen
        : max(seen, sent);
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
    final createdAt = _stamp(op.listId, op.itemId);
    await _relay.submit(
      kind: EventKind.projectTodoOp,
      content: op.toContent(),
      tags: op.tags(address),
      createdAt: createdAt,
    );
    _sent[op.itemId == null ? op.listId : '${op.listId}/${op.itemId}'] =
        createdAt;
  }

  /// The visibility of [listId] as the current read folded it — the fact
  /// every op on the list must repeat as its `td-vis` tag. Throws
  /// [ProjectTodoUnknownListError] when the read does not know the list.
  TodoVisibility visibilityOf(String listId) {
    final list = _read.listById(listId);
    if (list == null) throw ProjectTodoUnknownListError(listId);
    return list.visibility;
  }

  /// Create a list with a fixed [visibility], then pin it when [pinned];
  /// returns its id. Two ops, in that order: the pin is a field write on the
  /// list the create brought into existence, stamped after it.
  Future<String> createList(
    String title, {
    required TodoVisibility visibility,
    bool pinned = false,
  }) async {
    final listId = newProjectTodoId();
    await _publish(
      ProjectTodoOp.listCreate(
        listId: listId,
        visibility: visibility,
        title: title,
      ),
    );
    if (pinned) {
      await _publish(
        ProjectTodoOp.listPinned(
          listId: listId,
          visibility: visibility,
          pinned: true,
        ),
      );
    }
    return listId;
  }

  Future<void> retitleList(String listId, String title) async {
    await _publish(
      ProjectTodoOp.listTitle(
        listId: listId,
        visibility: visibilityOf(listId),
        title: title,
      ),
    );
  }

  Future<void> setListArchived(String listId, bool archived) async {
    await _publish(
      ProjectTodoOp.listArchived(
        listId: listId,
        visibility: visibilityOf(listId),
        archived: archived,
      ),
    );
  }

  /// Pin or unpin a list in the project tree (`list.pinned`).
  Future<void> setListPinned(String listId, bool pinned) async {
    await _publish(
      ProjectTodoOp.listPinned(
        listId: listId,
        visibility: visibilityOf(listId),
        pinned: pinned,
      ),
    );
  }

  /// Add an item after the last open item of [list]; returns its id.
  Future<String> addItem(TodoList list, String text) async {
    final itemId = newProjectTodoId();
    final last = list.open.isEmpty ? null : list.open.last.rank;
    await _publish(
      ProjectTodoOp.itemAdd(
        listId: list.id,
        visibility: visibilityOf(list.id),
        itemId: itemId,
        text: text,
        rank: rankBetween(last, null),
      ),
    );
    return itemId;
  }

  Future<void> setText(String listId, String itemId, String text) async {
    await _publish(
      ProjectTodoOp.itemText(
        listId: listId,
        visibility: visibilityOf(listId),
        itemId: itemId,
        text: text,
      ),
    );
  }

  Future<void> setDone(String listId, String itemId, bool done) async {
    await _publish(
      ProjectTodoOp.itemDone(
        listId: listId,
        visibility: visibilityOf(listId),
        itemId: itemId,
        done: done,
      ),
    );
  }

  Future<void> setAssignee(
    String listId,
    String itemId,
    String? assignee,
  ) async {
    await _publish(
      ProjectTodoOp.itemAssignee(
        listId: listId,
        visibility: visibilityOf(listId),
        itemId: itemId,
        assignee: assignee?.toLowerCase(),
      ),
    );
  }

  Future<void> setDue(String listId, String itemId, String? due) async {
    await _publish(
      ProjectTodoOp.itemDue(
        listId: listId,
        visibility: visibilityOf(listId),
        itemId: itemId,
        due: due,
      ),
    );
  }

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
        visibility: visibilityOf(list.id),
        itemId: moved.id,
        rank: rankBetween(after, before),
      ),
    );
  }

  Future<void> removeItem(String listId, String itemId) async {
    await _publish(
      ProjectTodoOp.itemRemove(
        listId: listId,
        visibility: visibilityOf(listId),
        itemId: itemId,
      ),
    );
  }
}

/// Run the wire validator over an op about to be sent, so a blank title or
/// a malformed date is refused here, in the validator's words, before it
/// costs a relay round trip.
void validateProjectTodoOpForSend(ProjectTodoOp op) {
  decodeProjectTodoOp(op.toContent(), op.visibility);
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
    now: ref.read(projectTodoClockProvider),
  );
});
