part of '../project_todos_page.dart';

/// The list picker: a dropdown over the lists the page is showing, with an
/// "archived" mark on the ones the show-archived toggle let in.
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
              child: Text(
                list.archived ? '${list.title} (archived)' : list.title,
                overflow: TextOverflow.ellipsis,
                style: list.archived
                    ? TextStyle(color: colors.onSurfaceVariant)
                    : null,
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
