import 'package:flutter/cupertino.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:intl/intl.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../../shared/mentions/agent_identity_provider.dart';
import '../../../shared/relay/relay_provider.dart';
import '../../../shared/theme/theme.dart';
import '../../../shared/utils/string_utils.dart';
import '../../../shared/widgets/avatar_image.dart';
import '../../../shared/widgets/bee_refresh_indicator.dart';
import '../../../shared/widgets/frosted_app_bar.dart';
import '../../../shared/widgets/frosted_scaffold.dart';
import '../../../shared/widgets/modal_presentation.dart';
import '../../profile/user_cache_provider.dart';
import '../../profile/user_profile.dart';
import '../../projects/state/projects_provider.dart';
import '../domain/project_todo_fold.dart';
import '../domain/project_todo_op.dart';
import '../state/project_todo_actions.dart';
import '../state/project_todos_provider.dart';

part 'project_todos_page/assignee_sheet.dart';
part 'project_todos_page/item_sheet.dart';
part 'project_todos_page/list_sheet.dart';
part 'project_todos_page/rows.dart';

/// A project's shared to-do lists (NIP-TD): one list at a time, its open
/// items in rank order and drag-reorderable, its completed items beneath,
/// most recently completed first, and a field to add the next item. Every
/// edit is one signed op; the page shows what the relay folded, never a
/// guess. Mobile cannot resolve roster roles, so the controls are always
/// shown and a refusal arrives as the relay's message, verbatim.
class ProjectTodosPage extends HookConsumerWidget {
  /// The canonical `30621:<owner>:<dtag>` coordinate.
  final String address;

  /// The list to show first (a pinned row in the project tree opens its
  /// list directly), or `null` for the first visible list.
  final String? initialListId;

  const ProjectTodosPage({
    super.key,
    required this.address,
    this.initialListId,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final read = ref.watch(projectTodosProvider(address));
    final project = ref.watch(projectsProvider).byAddress(address);
    final showArchived = useState(false);
    final selectedId = useState<String?>(initialListId);

    final lists = read.digest.lists;
    final visible = [
      for (final list in lists)
        if (showArchived.value || !list.archived) list,
    ];
    final selected = visible.isEmpty
        ? null
        : visible.firstWhere(
            (list) => list.id == selectedId.value,
            orElse: () => visible.first,
          );
    final hiddenArchived =
        lists.length -
        [
          for (final list in lists)
            if (!list.archived) list,
        ].length;

    ProjectTodoActions actions() =>
        ref.read(projectTodoActionsProvider(address));

    Future<void> newList() async {
      final draft = await _showNewListSheet(context);
      if (draft == null || !context.mounted) return;
      await runTodoAction(context, () async {
        final id = await actions().createList(
          draft.title,
          visibility: draft.visibility,
          pinned: draft.pinned,
        );
        selectedId.value = id;
      });
    }

    Future<void> renameList(TodoList list) async {
      final title = await _showListTitleSheet(
        context,
        title: 'Rename list',
        initial: list.title,
      );
      if (title == null || title == list.title || !context.mounted) return;
      await runTodoAction(context, () => actions().retitleList(list.id, title));
    }

    Future<void> setArchived(TodoList list, bool archived) => runTodoAction(
      context,
      () => actions().setListArchived(list.id, archived),
    );

    Future<void> setPinned(TodoList list, bool pinned) =>
        runTodoAction(context, () => actions().setListPinned(list.id, pinned));

    final colors = context.colors;
    return FrostedScaffold(
      backgroundColor: colors.surface,
      appBar: FrostedAppBar(
        title: Text(
          '${project?.name ?? 'Project'} · To-do',
          overflow: TextOverflow.ellipsis,
        ),
        actions: [
          PopupMenuButton<_ListMenuAction>(
            key: const ValueKey('todo-list-menu'),
            icon: const Icon(LucideIcons.ellipsisVertical, size: 20),
            onSelected: (action) {
              switch (action) {
                case _ListMenuAction.newList:
                  newList();
                case _ListMenuAction.rename:
                  if (selected != null) renameList(selected);
                case _ListMenuAction.archive:
                  if (selected != null) setArchived(selected, true);
                case _ListMenuAction.unarchive:
                  if (selected != null) setArchived(selected, false);
                case _ListMenuAction.pin:
                  if (selected != null) setPinned(selected, true);
                case _ListMenuAction.unpin:
                  if (selected != null) setPinned(selected, false);
                case _ListMenuAction.toggleArchived:
                  showArchived.value = !showArchived.value;
              }
            },
            itemBuilder: (_) => [
              const PopupMenuItem(
                key: ValueKey('todo-menu-new-list'),
                value: _ListMenuAction.newList,
                child: Text('New list'),
              ),
              if (selected != null) ...[
                const PopupMenuItem(
                  key: ValueKey('todo-menu-rename'),
                  value: _ListMenuAction.rename,
                  child: Text('Rename list'),
                ),
                PopupMenuItem(
                  key: const ValueKey('todo-menu-pin'),
                  value: selected.pinned
                      ? _ListMenuAction.unpin
                      : _ListMenuAction.pin,
                  child: Text(
                    selected.pinned
                        ? 'Unpin from project tree'
                        : 'Pin to project tree',
                  ),
                ),
                PopupMenuItem(
                  key: const ValueKey('todo-menu-archive'),
                  value: selected.archived
                      ? _ListMenuAction.unarchive
                      : _ListMenuAction.archive,
                  child: Text(
                    selected.archived ? 'Unarchive list' : 'Archive list',
                  ),
                ),
              ],
              CheckedPopupMenuItem(
                key: const ValueKey('todo-menu-show-archived'),
                value: _ListMenuAction.toggleArchived,
                checked: showArchived.value,
                child: const Text('Show archived'),
              ),
            ],
          ),
        ],
      ),
      body: Column(
        children: [
          SizedBox(height: frostedAppBarHeight(context)),
          Expanded(
            child: BeeRefreshIndicator(
              onRefresh: () =>
                  ref.read(projectTodosProvider(address).notifier).refresh(),
              child: ListView(
                padding: const EdgeInsets.fromLTRB(
                  Grid.xs,
                  Grid.xxs,
                  Grid.xs,
                  Grid.xl,
                ),
                children: [
                  if (read.error != null)
                    _Notice(
                      key: const ValueKey('todo-notice-error'),
                      icon: LucideIcons.triangleAlert,
                      text: read.error!,
                      emphasise: true,
                    ),
                  if (read.truncated)
                    const _Notice(
                      key: ValueKey('todo-notice-truncated'),
                      icon: LucideIcons.triangleAlert,
                      text:
                          'The read stopped before the oldest change: this '
                          'list may be missing history.',
                      emphasise: true,
                    ),
                  if (read.digest.ignored > 0)
                    _Notice(
                      key: const ValueKey('todo-notice-ignored'),
                      icon: LucideIcons.info,
                      text:
                          '${read.digest.ignored} '
                          '${read.digest.ignored == 1 ? 'change' : 'changes'} '
                          'could not be applied and '
                          '${read.digest.ignored == 1 ? 'is' : 'are'} not '
                          'shown.',
                    ),
                  if (read.loading && !read.hasRead)
                    const Padding(
                      padding: EdgeInsets.all(Grid.sm),
                      child: Center(child: CircularProgressIndicator()),
                    )
                  else if (visible.isEmpty)
                    _EmptyLists(
                      hiddenArchived: hiddenArchived,
                      onNewList: newList,
                      onShowArchived: hiddenArchived == 0
                          ? null
                          : () => showArchived.value = true,
                    )
                  else ...[
                    _ListPickerRow(
                      lists: visible,
                      selected: selected!,
                      onSelect: (id) => selectedId.value = id,
                    ),
                    _TodoListBody(
                      key: ValueKey('todo-list-${selected.id}'),
                      address: address,
                      list: selected,
                    ),
                  ],
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

enum _ListMenuAction {
  newList,
  rename,
  archive,
  unarchive,
  pin,
  unpin,
  toggleArchived,
}

/// Run one write and, when the relay or the validator refuses it, show the
/// refusal as it came — a viewer of a private project sees the relay's
/// `restricted: …` line, not a guess about their role.
Future<void> runTodoAction(
  BuildContext context,
  Future<void> Function() action,
) async {
  try {
    await action();
  } catch (error) {
    if (!context.mounted) return;
    ScaffoldMessenger.maybeOf(
      context,
    )?.showSnackBar(SnackBar(content: Text(todoErrorMessage(error))));
  }
}

/// The text of a refusal, without the `Exception:` wrapper Dart adds.
String todoErrorMessage(Object error) {
  final text = error.toString();
  const prefix = 'Exception: ';
  return text.startsWith(prefix) ? text.substring(prefix.length) : text;
}

/// One list's body: the add field, the open items as a reorderable list,
/// and the completed section.
class _TodoListBody extends HookConsumerWidget {
  final String address;
  final TodoList list;

  const _TodoListBody({super.key, required this.address, required this.list});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final profiles = ref.watch(userCacheProvider);
    final agents = ref.watch(knownAgentPubkeysProvider);
    final me = ref.watch(myPubkeyProvider);
    final addController = useTextEditingController();
    final adding = useState(false);
    // A drag's result, shown until the fold catches up so the row does not
    // snap back while the op is in flight.
    final localOpen = useState<List<TodoItem>?>(null);
    useEffect(() {
      localOpen.value = null;
      return null;
    }, [list]);

    final assignees = <String>{
      for (final item in list.open)
        if (item.assignee != null) item.assignee!,
      for (final item in list.completed)
        if (item.assignee != null) item.assignee!,
    };
    useEffect(() {
      if (assignees.isNotEmpty) {
        ref.read(userCacheProvider.notifier).preload(assignees.toList());
      }
      return null;
    }, [assignees.join(',')]);

    ProjectTodoActions actions() =>
        ref.read(projectTodoActionsProvider(address));

    _Assignee? assigneeOf(TodoItem item) {
      final pubkey = item.assignee;
      if (pubkey == null) return null;
      final profile = profiles[pubkey];
      return _Assignee(
        pubkey: pubkey,
        label: pubkey == me?.toLowerCase()
            ? 'you'
            : profile?.label ?? shortPubkey(pubkey),
        avatarUrl: profile?.avatarUrl,
        isAgent: profile?.ownerPubkey != null || agents.contains(pubkey),
      );
    }

    Future<void> add() async {
      final text = addController.text.trim();
      if (text.isEmpty || adding.value) return;
      adding.value = true;
      try {
        await runTodoAction(context, () async {
          await actions().addItem(list, text);
          addController.clear();
        });
      } finally {
        adding.value = false;
      }
    }

    Future<void> setDone(TodoItem item, bool done) =>
        runTodoAction(context, () => actions().setDone(list.id, item.id, done));

    void openItem(TodoItem item) => showBeekeeperModalBottomSheet<void>(
      context: context,
      title: 'Edit item',
      isScrollControlled: true,
      builder: (_) => _ItemSheet(address: address, listId: list.id, item: item),
    );

    final open = localOpen.value ?? list.open;
    final colors = context.colors;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(vertical: Grid.xxs),
          child: TextField(
            key: const ValueKey('todo-add-field'),
            controller: addController,
            enabled: !adding.value,
            textInputAction: TextInputAction.done,
            onSubmitted: (_) => add(),
            decoration: InputDecoration(
              hintText: 'Add an item',
              isDense: true,
              prefixIcon: const Icon(LucideIcons.plus, size: 18),
              suffixIcon: IconButton(
                key: const ValueKey('todo-add-submit'),
                tooltip: 'Add',
                onPressed: adding.value ? null : add,
                icon: const Icon(LucideIcons.send, size: 18),
              ),
            ),
          ),
        ),
        _SectionHeader(
          key: const ValueKey('todo-section-open'),
          text: 'Open',
          count: open.length,
        ),
        if (open.isEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(
              horizontal: Grid.xs,
              vertical: Grid.xxs,
            ),
            child: Text(
              list.completed.isEmpty
                  ? 'Nothing here yet. Add the first item above.'
                  : 'Everything is done.',
              style: context.textTheme.bodySmall?.copyWith(
                color: colors.onSurfaceVariant,
              ),
            ),
          )
        else
          ReorderableListView(
            key: const ValueKey('todo-open-list'),
            shrinkWrap: true,
            physics: const NeverScrollableScrollPhysics(),
            buildDefaultDragHandles: false,
            onReorder: (oldIndex, newIndex) {
              final reordered = List.of(open);
              final moved = reordered.removeAt(oldIndex);
              reordered.insert(
                newIndex > oldIndex ? newIndex - 1 : newIndex,
                moved,
              );
              localOpen.value = reordered;
              runTodoAction(
                context,
                () => actions().moveItem(list, oldIndex, newIndex),
              );
            },
            children: [
              for (var i = 0; i < open.length; i++)
                _TodoItemRow(
                  key: ValueKey('todo-open-${open[i].id}'),
                  item: open[i],
                  index: i,
                  assignee: assigneeOf(open[i]),
                  onToggle: (done) => setDone(open[i], done),
                  onTap: () => openItem(open[i]),
                ),
            ],
          ),
        if (list.completed.isNotEmpty) ...[
          _SectionHeader(
            key: const ValueKey('todo-section-completed'),
            text: 'Completed',
            count: list.completed.length,
          ),
          for (final item in list.completed)
            _TodoItemRow(
              key: ValueKey('todo-done-${item.id}'),
              item: item,
              index: null,
              assignee: assigneeOf(item),
              onToggle: (done) => setDone(item, done),
              onTap: () => openItem(item),
            ),
        ],
      ],
    );
  }
}

/// What the rows know about an assignee: enough to draw the chip.
@immutable
class _Assignee {
  final String pubkey;
  final String label;
  final String? avatarUrl;
  final bool isAgent;

  const _Assignee({
    required this.pubkey,
    required this.label,
    required this.avatarUrl,
    required this.isAgent,
  });
}

/// `YYYY-MM-DD` as a local calendar date, or `null` when it is not one.
DateTime? parseDueDate(String? due) {
  if (due == null) return null;
  try {
    validateDueDate(due);
  } on FormatException {
    return null;
  }
  return DateTime(
    int.parse(due.substring(0, 4)),
    int.parse(due.substring(5, 7)),
    int.parse(due.substring(8, 10)),
  );
}

/// A due date as the row shows it: the day, with the year when it is not
/// this one.
String formatDueDate(DateTime due, DateTime today) => due.year == today.year
    ? DateFormat('EEE, MMM d').format(due)
    : DateFormat('MMM d, yyyy').format(due);

/// A date as the wire wants it.
String dueDateString(DateTime date) =>
    '${date.year.toString().padLeft(4, '0')}-'
    '${date.month.toString().padLeft(2, '0')}-'
    '${date.day.toString().padLeft(2, '0')}';

/// Midnight today, so "overdue" means strictly before today.
DateTime todayDate() {
  final now = DateTime.now();
  return DateTime(now.year, now.month, now.day);
}
