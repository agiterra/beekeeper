part of '../project_todos_page.dart';

/// The list picker: a dropdown over the lists the page is showing, with an
/// "archived" mark on the ones the show-archived toggle let in, a lock on
/// the personal ones and a pin on the pinned ones.
class _ListPickerRow extends StatelessWidget {
  final List<TodoList> lists;
  final TodoList selected;
  final ValueChanged<String> onSelect;

  const _ListPickerRow({
    required this.lists,
    required this.selected,
    required this.onSelect,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: Grid.half),
      child: DropdownButtonFormField<String>(
        key: const ValueKey('todo-list-picker'),
        initialValue: selected.id,
        isExpanded: true,
        decoration: const InputDecoration(
          labelText: 'List',
          isDense: true,
          prefixIcon: Icon(LucideIcons.listChecks, size: 18),
        ),
        items: [
          for (final list in lists)
            DropdownMenuItem(
              key: ValueKey('todo-list-option-${list.id}'),
              value: list.id,
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      list.archived ? '${list.title} (archived)' : list.title,
                      overflow: TextOverflow.ellipsis,
                      style: list.archived
                          ? TextStyle(color: colors.onSurfaceVariant)
                          : null,
                    ),
                  ),
                  if (list.pinned)
                    Padding(
                      padding: const EdgeInsets.only(left: Grid.xxs),
                      child: Icon(
                        LucideIcons.pin,
                        key: ValueKey('todo-list-pinned-${list.id}'),
                        size: 14,
                        color: colors.onSurfaceVariant,
                      ),
                    ),
                  if (list.personal)
                    Padding(
                      padding: const EdgeInsets.only(left: Grid.xxs),
                      child: Icon(
                        LucideIcons.lock,
                        key: ValueKey('todo-list-personal-${list.id}'),
                        size: 14,
                        color: colors.onSurfaceVariant,
                      ),
                    ),
                ],
              ),
            ),
        ],
        onChanged: (id) {
          if (id != null) onSelect(id);
        },
      ),
    );
  }
}

/// Ask for a list title. Resolves to the trimmed title, or `null` when the
/// sheet is dismissed.
Future<String?> _showListTitleSheet(
  BuildContext context, {
  required String title,
  String? initial,
}) => showBuzzModalBottomSheet<String>(
  context: context,
  title: title,
  isScrollControlled: true,
  builder: (_) => _ListTitleSheet(initial: initial),
);

class _ListTitleSheet extends HookWidget {
  final String? initial;

  const _ListTitleSheet({required this.initial});

  @override
  Widget build(BuildContext context) {
    final controller = useTextEditingController(text: initial ?? '');
    final text = useState(initial ?? '');
    useEffect(() {
      void listen() => text.value = controller.text;
      controller.addListener(listen);
      return () => controller.removeListener(listen);
    }, [controller]);
    final valid = text.value.trim().isNotEmpty;

    void submit() {
      final value = controller.text.trim();
      if (value.isEmpty) return;
      Navigator.of(context).pop(value);
    }

    return Padding(
      padding: EdgeInsets.fromLTRB(
        Grid.xs,
        0,
        Grid.xs,
        MediaQuery.viewInsetsOf(context).bottom + Grid.xs,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          TextField(
            key: const ValueKey('todo-list-title-field'),
            controller: controller,
            autofocus: true,
            textInputAction: TextInputAction.done,
            onSubmitted: (_) => submit(),
            decoration: const InputDecoration(
              labelText: 'Title',
              isDense: true,
            ),
          ),
          const SizedBox(height: Grid.xxs),
          FilledButton(
            key: const ValueKey('todo-list-title-save'),
            onPressed: valid ? submit : null,
            child: Text(initial == null ? 'Create' : 'Save'),
          ),
        ],
      ),
    );
  }
}

/// What the "New list" sheet asks for: a title, a visibility that cannot be
/// changed later, and whether to pin the list to the project tree.
@immutable
class NewListDraft {
  final String title;
  final TodoVisibility visibility;
  final bool pinned;

  const NewListDraft({
    required this.title,
    required this.visibility,
    required this.pinned,
  });
}

/// Ask for a new list. Resolves to the draft, or `null` when the sheet is
/// dismissed.
Future<NewListDraft?> _showNewListSheet(BuildContext context) =>
    showBuzzModalBottomSheet<NewListDraft>(
      context: context,
      title: 'New list',
      isScrollControlled: true,
      builder: (_) => const _NewListSheet(),
    );

class _NewListSheet extends HookWidget {
  const _NewListSheet();

  @override
  Widget build(BuildContext context) {
    final controller = useTextEditingController();
    final text = useState('');
    final visibility = useState(TodoVisibility.project);
    final pinned = useState(true);
    useEffect(() {
      void listen() => text.value = controller.text;
      controller.addListener(listen);
      return () => controller.removeListener(listen);
    }, [controller]);
    final valid = text.value.trim().isNotEmpty;

    void submit() {
      final value = controller.text.trim();
      if (value.isEmpty) return;
      Navigator.of(context).pop(
        NewListDraft(
          title: value,
          visibility: visibility.value,
          pinned: pinned.value,
        ),
      );
    }

    final colors = context.colors;
    final hint = context.textTheme.bodySmall?.copyWith(
      color: colors.onSurfaceVariant,
    );
    return Padding(
      padding: EdgeInsets.fromLTRB(
        Grid.xs,
        0,
        Grid.xs,
        MediaQuery.viewInsetsOf(context).bottom + Grid.xs,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          TextField(
            key: const ValueKey('todo-list-title-field'),
            controller: controller,
            autofocus: true,
            textInputAction: TextInputAction.done,
            onSubmitted: (_) => submit(),
            decoration: const InputDecoration(
              labelText: 'Title',
              isDense: true,
            ),
          ),
          const SizedBox(height: Grid.xxs),
          RadioGroup<TodoVisibility>(
            groupValue: visibility.value,
            onChanged: (value) {
              if (value != null) visibility.value = value;
            },
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                RadioListTile<TodoVisibility>(
                  key: const ValueKey('todo-new-list-project'),
                  value: TodoVisibility.project,
                  dense: true,
                  contentPadding: EdgeInsets.zero,
                  secondary: const Icon(LucideIcons.users, size: 18),
                  title: const Text('Project'),
                  subtitle: const Text(
                    'Every project member reads and edits it.',
                  ),
                ),
                RadioListTile<TodoVisibility>(
                  key: const ValueKey('todo-new-list-personal'),
                  value: TodoVisibility.personal,
                  dense: true,
                  contentPadding: EdgeInsets.zero,
                  secondary: const Icon(LucideIcons.lock, size: 18),
                  title: const Text('Personal'),
                  subtitle: const Text(
                    'Only you. The relay withholds it from everyone else.',
                  ),
                ),
              ],
            ),
          ),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: Grid.xxs),
            child: Text(
              'This cannot be changed later; make a new list instead.',
              style: hint,
            ),
          ),
          SwitchListTile(
            key: const ValueKey('todo-new-list-pinned'),
            value: pinned.value,
            onChanged: (value) => pinned.value = value,
            dense: true,
            contentPadding: EdgeInsets.zero,
            secondary: const Icon(LucideIcons.pin, size: 18),
            title: const Text('Pin to project tree'),
          ),
          const SizedBox(height: Grid.xxs),
          FilledButton(
            key: const ValueKey('todo-list-title-save'),
            onPressed: valid ? submit : null,
            child: const Text('Create'),
          ),
        ],
      ),
    );
  }
}
