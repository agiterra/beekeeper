part of '../project_todos_page.dart';

/// Edit one item: its text, its assignee, its due date, or remove it. Each
/// change is one op sent as it is made; the sheet keeps reading the live
/// fold so a change from another device shows up while it is open, and it
/// closes itself if the item is removed under it.
class _ItemSheet extends HookConsumerWidget {
  final String address;
  final String listId;
  final TodoItem item;

  const _ItemSheet({
    required this.address,
    required this.listId,
    required this.item,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final read = ref.watch(projectTodosProvider(address));
    final list = read.digest.listById(listId);
    TodoItem? find(List<TodoItem> items) {
      for (final candidate in items) {
        if (candidate.id == item.id) return candidate;
      }
      return null;
    }

    final live = list == null ? item : find(list.open) ?? find(list.completed);
    final removed = read.hasRead && list != null && live == null;
    final current = live ?? item;

    final profiles = ref.watch(userCacheProvider);
    final agents = ref.watch(knownAgentPubkeysProvider);
    final me = ref.watch(myPubkeyProvider);
    final controller = useTextEditingController(text: current.text);
    final draft = useState(current.text);
    final busy = useState(false);
    useEffect(() {
      void listen() => draft.value = controller.text;
      controller.addListener(listen);
      return () => controller.removeListener(listen);
    }, [controller]);
    // Another device's text write replaces an untouched draft, never one
    // being typed.
    final lastSeenText = useRef(current.text);
    useEffect(() {
      if (controller.text.trim() == lastSeenText.value.trim()) {
        controller.text = current.text;
      }
      lastSeenText.value = current.text;
      return null;
    }, [current.text]);
    useEffect(() {
      if (removed) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (context.mounted) Navigator.of(context).maybePop();
        });
      }
      return null;
    }, [removed]);

    ProjectTodoActions actions() =>
        ref.read(projectTodoActionsProvider(address));

    Future<void> run(Future<void> Function() action) async {
      busy.value = true;
      try {
        await runTodoAction(context, action);
      } finally {
        if (context.mounted) busy.value = false;
      }
    }

    final textChanged =
        draft.value.trim().isNotEmpty && draft.value.trim() != current.text;

    Future<void> saveText() => run(
      () => actions().setText(listId, current.id, controller.text.trim()),
    );

    Future<void> pickAssignee() async {
      final pick = await showBuzzModalBottomSheet<_AssigneePick>(
        context: context,
        title: 'Assign to',
        isScrollControlled: true,
        builder: (_) =>
            _AssigneeSheet(address: address, current: current.assignee),
      );
      if (pick == null || !context.mounted) return;
      await run(() => actions().setAssignee(listId, current.id, pick.pubkey));
    }

    Future<void> pickDue() async {
      final picked = await _pickDueDate(
        context,
        initial: parseDueDate(current.due) ?? todayDate(),
      );
      if (picked == null || !context.mounted) return;
      await run(
        () => actions().setDue(listId, current.id, dueDateString(picked)),
      );
    }

    Future<void> clearDue() =>
        run(() => actions().setDue(listId, current.id, null));

    Future<void> remove() async {
      final confirmed = await showBuzzDialog<bool>(
        context: context,
        builder: (dialogContext) => AlertDialog(
          title: const Text('Remove this item?'),
          content: const Text(
            'It is removed for everyone in the project and cannot be '
            'brought back.',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(dialogContext).pop(false),
              child: const Text('Cancel'),
            ),
            TextButton(
              key: const ValueKey('todo-item-remove-confirm'),
              onPressed: () => Navigator.of(dialogContext).pop(true),
              child: const Text('Remove'),
            ),
          ],
        ),
      );
      if (confirmed != true || !context.mounted) return;
      await run(() async {
        await actions().removeItem(listId, current.id);
        if (context.mounted) Navigator.of(context).maybePop();
      });
    }

    final colors = context.colors;
    final assignee = current.assignee;
    final assigneeProfile = assignee == null ? null : profiles[assignee];
    final assigneeIsAgent =
        assignee != null &&
        (assigneeProfile?.ownerPubkey != null || agents.contains(assignee));
    final assigneeLabel = assignee == null
        ? 'Unassigned'
        : assignee == me?.toLowerCase()
        ? 'You'
        : assigneeProfile?.label ?? shortPubkey(assignee);
    final due = parseDueDate(current.due);

    return SingleChildScrollView(
      padding: EdgeInsets.fromLTRB(
        Grid.xs,
        0,
        Grid.xs,
        MediaQuery.viewInsetsOf(context).bottom + Grid.xs,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          TextField(
            key: const ValueKey('todo-item-text-field'),
            controller: controller,
            enabled: !busy.value,
            minLines: 1,
            maxLines: 5,
            textInputAction: TextInputAction.done,
            onSubmitted: (_) => textChanged ? saveText() : null,
            decoration: InputDecoration(
              labelText: 'Text',
              isDense: true,
              suffixIcon: IconButton(
                key: const ValueKey('todo-item-text-save'),
                tooltip: 'Save',
                onPressed: textChanged && !busy.value ? saveText : null,
                icon: const Icon(LucideIcons.check, size: 18),
              ),
            ),
          ),
          const SizedBox(height: Grid.xxs),
          ListTile(
            key: const ValueKey('todo-item-assignee'),
            dense: true,
            contentPadding: EdgeInsets.zero,
            leading: Icon(
              assigneeIsAgent ? LucideIcons.bot : LucideIcons.userRound,
              size: 18,
              color: colors.primary,
            ),
            title: const Text('Assignee'),
            subtitle: Text(assigneeLabel),
            trailing: const Icon(LucideIcons.chevronRight, size: 16),
            enabled: !busy.value,
            onTap: pickAssignee,
          ),
          ListTile(
            key: const ValueKey('todo-item-due'),
            dense: true,
            contentPadding: EdgeInsets.zero,
            leading: Icon(
              LucideIcons.calendar,
              size: 18,
              color: colors.primary,
            ),
            title: const Text('Due date'),
            subtitle: Text(
              due == null ? 'None' : formatDueDate(due, todayDate()),
            ),
            trailing: due == null
                ? const Icon(LucideIcons.chevronRight, size: 16)
                : IconButton(
                    key: const ValueKey('todo-item-due-clear'),
                    tooltip: 'Clear due date',
                    onPressed: busy.value ? null : clearDue,
                    icon: const Icon(LucideIcons.x, size: 16),
                  ),
            enabled: !busy.value,
            onTap: pickDue,
          ),
          ListTile(
            key: const ValueKey('todo-item-done'),
            dense: true,
            contentPadding: EdgeInsets.zero,
            leading: Icon(
              current.done ? LucideIcons.circleCheck : LucideIcons.circle,
              size: 18,
              color: colors.primary,
            ),
            title: Text(current.done ? 'Done' : 'Open'),
            subtitle: current.done && current.completedAt != null
                ? Text('Completed ${_completedLabel(current, profiles, me)}')
                : null,
            enabled: !busy.value,
            onTap: () =>
                run(() => actions().setDone(listId, current.id, !current.done)),
          ),
          const SizedBox(height: Grid.xxs),
          TextButton.icon(
            key: const ValueKey('todo-item-remove'),
            style: TextButton.styleFrom(foregroundColor: colors.error),
            onPressed: busy.value ? null : remove,
            icon: const Icon(LucideIcons.trash2, size: 16),
            label: const Text('Remove item'),
          ),
        ],
      ),
    );
  }
}

String _completedLabel(
  TodoItem item,
  Map<String, UserProfile> profiles,
  String? me,
) {
  final at = DateTime.fromMillisecondsSinceEpoch(
    (item.completedAt ?? 0) * 1000,
  );
  final by = item.completedBy;
  final who = by == null
      ? ''
      : by == me?.toLowerCase()
      ? ' by you'
      : ' by ${profiles[by]?.label ?? shortPubkey(by)}';
  return '${DateFormat('MMM d, h:mm a').format(at)}$who';
}

/// A calendar day: the Cupertino wheel on iOS, the Material picker
/// elsewhere. Resolves to `null` when dismissed.
Future<DateTime?> _pickDueDate(
  BuildContext context, {
  required DateTime initial,
}) async {
  final first = DateTime(1970);
  final last = DateTime(9999, 12, 31);
  final safeInitial = initial.isBefore(first)
      ? first
      : initial.isAfter(last)
      ? last
      : initial;
  if (defaultTargetPlatform == TargetPlatform.iOS) {
    var selected = safeInitial;
    return showCupertinoModalPopup<DateTime>(
      context: context,
      builder: (pickerContext) => Material(
        color: context.colors.surface,
        child: SafeArea(
          top: false,
          child: SizedBox(
            height: 300,
            child: Column(
              children: [
                Align(
                  alignment: Alignment.centerRight,
                  child: TextButton(
                    onPressed: () => Navigator.of(pickerContext).pop(selected),
                    child: const Text('Done'),
                  ),
                ),
                Expanded(
                  child: CupertinoDatePicker(
                    mode: CupertinoDatePickerMode.date,
                    minimumDate: first,
                    maximumDate: last,
                    initialDateTime: safeInitial,
                    onDateTimeChanged: (value) => selected = value,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
  return showDatePicker(
    context: context,
    initialDate: safeInitial,
    firstDate: first,
    lastDate: last,
  );
}
