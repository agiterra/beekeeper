import 'package:beekeeper/features/project_todos/domain/project_todo_fold.dart';
import 'package:beekeeper/features/project_todos/domain/project_todo_op.dart';
import 'package:beekeeper/features/project_todos/state/project_todo_actions.dart';
import 'package:beekeeper/features/project_todos/state/project_todos_provider.dart';
import 'package:beekeeper/shared/relay/relay.dart';

import '../../helpers/recording_relay_session.dart';

const todoOwner =
    'aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd';
const todoViewer =
    '11111111222222223333333344444444555555556666666677777777dddddddd';
const todoAgent =
    'a9e47a9e47a9e47a9e47a9e47a9e47a9e47a9e47a9e47a9e47a9e47a9e47a9e4';
const todoAddress = '30621:$todoOwner:beekeeper';

const listA = '0123456789abcdef0123456789abcdef';
const listB = 'fedcba9876543210fedcba9876543210';

/// A 32-hex item id from a short tag.
String itemId(String tag) => tag.padRight(32, '0');

TodoItem testTodoItem(
  String tag, {
  String listId = listA,
  String text = 'Item',
  bool done = false,
  String rank = 'a0',
  String? assignee,
  String? due,
  int createdAt = 100,
  String createdBy = todoOwner,
  int? completedAt,
  String? completedBy,
}) => TodoItem(
  id: itemId(tag),
  listId: listId,
  text: text,
  done: done,
  rank: rank,
  assignee: assignee,
  due: due,
  createdAt: createdAt,
  createdBy: createdBy,
  updatedAt: completedAt ?? createdAt,
  completedAt: completedAt,
  completedBy: completedBy,
);

TodoList testTodoList({
  String id = listA,
  String title = 'Launch',
  TodoVisibility visibility = TodoVisibility.project,
  bool archived = false,
  bool pinned = false,
  int createdAt = 100,
  List<TodoItem> open = const [],
  List<TodoItem> completed = const [],
}) => TodoList(
  id: id,
  title: title,
  visibility: visibility,
  archived: archived,
  pinned: pinned,
  createdAt: createdAt,
  createdBy: todoOwner,
  updatedAt: createdAt,
  open: open,
  completed: completed,
);

ProjectTodosRead testTodosRead({
  List<TodoList> lists = const [],
  int ignored = 0,
  bool truncated = false,
  bool loading = false,
  String? error,
  bool hasRead = true,
}) => ProjectTodosRead(
  digest: ProjectTodoDigest(
    project: todoAddress,
    ignored: ignored,
    lists: lists,
  ),
  truncated: truncated,
  loading: loading,
  error: error,
  hasRead: hasRead,
);

class FakeProjectTodosNotifier extends ProjectTodosNotifier {
  final ProjectTodosRead read;
  int refreshCount = 0;

  FakeProjectTodosNotifier(super.address, this.read);

  @override
  ProjectTodosRead build() => read;

  @override
  Future<void> refresh() async {
    refreshCount++;
  }
}

/// Records every write the page asks for, in call order, as one line each
/// (`setDone <list> <item> true`), and throws [failure] when set.
class FakeProjectTodoActions extends ProjectTodoActions {
  final List<String> calls = [];
  Exception? failure;

  FakeProjectTodoActions()
    : super(
        address: todoAddress,
        relay: SignedEventRelay(
          session: RecordingRelaySessionNotifier(),
          nsec: null,
        ),
        read: FakeProjectTodosNotifier(todoAddress, testTodosRead()),
      );

  Future<T> _record<T>(String line, T value) async {
    calls.add(line);
    final error = failure;
    if (error != null) throw error;
    return value;
  }

  @override
  Future<String> createList(
    String title, {
    required TodoVisibility visibility,
    bool pinned = false,
  }) => _record('createList $title ${visibility.wire} pinned=$pinned', listB);

  @override
  Future<void> retitleList(String listId, String title) =>
      _record('retitleList $listId $title', null);

  @override
  Future<void> setListArchived(String listId, bool archived) =>
      _record('setListArchived $listId $archived', null);

  @override
  Future<void> setListPinned(String listId, bool pinned) =>
      _record('setListPinned $listId $pinned', null);

  @override
  Future<String> addItem(TodoList list, String text) =>
      _record('addItem ${list.id} $text', itemId('new'));

  @override
  Future<void> setText(String listId, String itemId, String text) =>
      _record('setText $listId $itemId $text', null);

  @override
  Future<void> setDone(String listId, String itemId, bool done) =>
      _record('setDone $listId $itemId $done', null);

  @override
  Future<void> setAssignee(String listId, String itemId, String? assignee) =>
      _record('setAssignee $listId $itemId $assignee', null);

  @override
  Future<void> setDue(String listId, String itemId, String? due) =>
      _record('setDue $listId $itemId $due', null);

  @override
  Future<void> moveItem(TodoList list, int oldIndex, int newIndex) =>
      _record('moveItem ${list.id} $oldIndex $newIndex', null);

  @override
  Future<void> removeItem(String listId, String itemId) =>
      _record('removeItem $listId $itemId', null);
}
