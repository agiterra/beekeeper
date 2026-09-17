part of '../project_todos_page.dart';

/// One item: a checkbox that flips `done`, the text (struck through when
/// done), and beneath it the assignee chip and the due chip. Tapping the
/// row opens the edit sheet; an open row also carries a drag handle.
class _TodoItemRow extends StatelessWidget {
  final TodoItem item;

  /// The row's index among the open items, or `null` for a completed one.
  final int? index;
  final _Assignee? assignee;
  final ValueChanged<bool> onToggle;
  final VoidCallback onTap;

  const _TodoItemRow({
    super.key,
    required this.item,
    required this.index,
    required this.assignee,
    required this.onToggle,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final due = parseDueDate(item.due);
    final today = todayDate();
    final overdue = due != null && !item.done && due.isBefore(today);
    final chips = <Widget>[
      if (assignee != null)
        _AssigneeChip(
          key: ValueKey('todo-assignee-${item.id}'),
          assignee: assignee!,
        ),
      if (due != null)
        _DueChip(
          key: ValueKey('todo-due-${item.id}'),
          label: formatDueDate(due, today),
          overdue: overdue,
        ),
    ];
    return Opacity(
      opacity: item.done ? 0.7 : 1,
      child: ListTile(
        dense: true,
        visualDensity: VisualDensity.compact,
        contentPadding: const EdgeInsets.only(left: 0, right: Grid.half),
        leading: Checkbox(
          key: ValueKey('todo-checkbox-${item.id}'),
          value: item.done,
          onChanged: (value) => onToggle(value ?? false),
        ),
        title: Text(
          item.text,
          style: context.textTheme.bodyMedium?.copyWith(
            decoration: item.done ? TextDecoration.lineThrough : null,
            color: item.done ? colors.onSurfaceVariant : null,
          ),
        ),
        subtitle: chips.isEmpty
            ? null
            : Padding(
                padding: const EdgeInsets.only(top: Grid.half),
                child: Wrap(
                  spacing: Grid.xxs,
                  runSpacing: Grid.half,
                  children: chips,
                ),
              ),
        trailing: index == null
            ? null
            : ReorderableDragStartListener(
                key: ValueKey('todo-drag-${item.id}'),
                index: index!,
                child: Icon(
                  LucideIcons.gripVertical,
                  size: 18,
                  color: colors.onSurfaceVariant,
                ),
              ),
        onTap: onTap,
      ),
    );
  }
}

/// Who an item is assigned to: an avatar (a bot glyph for an agent) and a
/// name.
class _AssigneeChip extends StatelessWidget {
  final _Assignee assignee;

  const _AssigneeChip({super.key, required this.assignee});

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (assignee.isAgent)
          Icon(
            LucideIcons.bot,
            key: const ValueKey('todo-assignee-bot'),
            size: 14,
            color: colors.primary,
          )
        else
          AvatarImage(
            imageUrl: assignee.avatarUrl,
            radius: 7,
            backgroundColor: colors.primaryContainer,
            fallback: Text(
              assignee.label.isEmpty ? '?' : assignee.label[0].toUpperCase(),
              style: context.textTheme.labelSmall?.copyWith(
                color: colors.onPrimaryContainer,
              ),
            ),
          ),
        const SizedBox(width: Grid.half),
        Text(
          assignee.label,
          style: context.textTheme.bodySmall?.copyWith(
            color: colors.onSurfaceVariant,
          ),
        ),
      ],
    );
  }
}

/// When an item is due; red once the day has passed and the item is open.
class _DueChip extends StatelessWidget {
  final String label;
  final bool overdue;

  const _DueChip({super.key, required this.label, required this.overdue});

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final color = overdue ? colors.error : colors.onSurfaceVariant;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(
          overdue ? LucideIcons.calendarClock : LucideIcons.calendar,
          key: ValueKey(overdue ? 'todo-due-overdue' : 'todo-due-upcoming'),
          size: 14,
          color: color,
        ),
        const SizedBox(width: Grid.half),
        Text(
          overdue ? '$label · overdue' : label,
          style: context.textTheme.bodySmall?.copyWith(color: color),
        ),
      ],
    );
  }
}

/// "Open · 3" / "Completed · 2".
class _SectionHeader extends StatelessWidget {
  final String text;
  final int count;

  const _SectionHeader({super.key, required this.text, required this.count});

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.fromLTRB(
      Grid.xs,
      Grid.twelve,
      Grid.xs,
      Grid.half,
    ),
    child: Text(
      '$text · $count',
      style: context.textTheme.labelLarge?.copyWith(
        color: context.colors.onSurfaceVariant,
      ),
    ),
  );
}

/// A one-line disclosure above the list.
class _Notice extends StatelessWidget {
  final IconData icon;
  final String text;
  final bool emphasise;

  const _Notice({
    super.key,
    required this.icon,
    required this.text,
    this.emphasise = false,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final color = emphasise ? colors.error : colors.onSurfaceVariant;
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: Grid.xs,
        vertical: Grid.half,
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, size: 16, color: color),
          const SizedBox(width: Grid.xxs),
          Expanded(
            child: Text(
              text,
              style: context.textTheme.bodySmall?.copyWith(color: color),
            ),
          ),
        ],
      ),
    );
  }
}

/// No list to show: either none exists or every one is archived.
class _EmptyLists extends StatelessWidget {
  final int hiddenArchived;
  final VoidCallback onNewList;
  final VoidCallback? onShowArchived;

  const _EmptyLists({
    required this.hiddenArchived,
    required this.onNewList,
    required this.onShowArchived,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    return Padding(
      key: const ValueKey('todo-empty'),
      padding: const EdgeInsets.all(Grid.sm),
      child: Column(
        children: [
          Icon(
            LucideIcons.listChecks,
            size: 32,
            color: colors.onSurfaceVariant,
          ),
          const SizedBox(height: Grid.xxs),
          Text(
            hiddenArchived > 0
                ? '$hiddenArchived archived '
                      '${hiddenArchived == 1 ? 'list' : 'lists'}, none open'
                : 'No to-do lists yet',
            style: context.textTheme.bodyMedium,
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: Grid.xs),
          FilledButton.icon(
            key: const ValueKey('todo-empty-new-list'),
            onPressed: onNewList,
            icon: const Icon(LucideIcons.plus, size: 16),
            label: const Text('New list'),
          ),
          if (onShowArchived != null)
            TextButton(
              key: const ValueKey('todo-empty-show-archived'),
              onPressed: onShowArchived,
              child: const Text('Show archived'),
            ),
        ],
      ),
    );
  }
}
