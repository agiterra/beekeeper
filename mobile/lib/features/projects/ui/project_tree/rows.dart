part of '../project_tree.dart';

/// A channel or forum row: one icon for the type, the name, a detail line.
class _ProjectChildTile extends StatelessWidget {
  final IconData icon;
  final String label;
  final String? detail;
  final VoidCallback onTap;

  const _ProjectChildTile({
    super.key,
    required this.icon,
    required this.label,
    required this.detail,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    return ListTile(
      dense: true,
      visualDensity: VisualDensity.compact,
      leading: Icon(icon, size: 18, color: colors.primary),
      title: Text(
        label,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: context.textTheme.bodyMedium,
      ),
      subtitle: detail == null
          ? null
          : Text(
              detail!,
              style: context.textTheme.bodySmall?.copyWith(
                color: colors.onSurfaceVariant,
              ),
            ),
      onTap: onTap,
    );
  }
}

/// A coding session row: the session icon, its name, the channel it lives
/// in and who started it, and its folded status. A settled (closed) session
/// keeps the row but dims it — closure is intent, not activity.
class _SessionTile extends StatelessWidget {
  final ProjectSessionRow row;

  /// The founder's display name, or `null` when the fold has no founder.
  final String? founderLabel;
  final bool isMine;
  final VoidCallback onTap;

  const _SessionTile({
    super.key,
    required this.row,
    required this.founderLabel,
    required this.isMine,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final settled = row.isClosed;
    final startedBy = founderLabel == null
        ? 'initiator unknown'
        : isMine
        ? 'started by you'
        : 'started by $founderLabel';
    final detail = [row.channelName, startedBy].join(' · ');
    return Opacity(
      opacity: settled ? 0.6 : 1,
      child: ListTile(
        dense: true,
        visualDensity: VisualDensity.compact,
        leading: Icon(
          settled ? LucideIcons.circleCheck : LucideIcons.bot,
          size: 18,
          color: settled ? colors.onSurfaceVariant : colors.primary,
        ),
        title: Text(
          row.label,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: context.textTheme.bodyMedium,
        ),
        subtitle: Text(
          detail,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: context.textTheme.bodySmall?.copyWith(
            color: colors.onSurfaceVariant,
          ),
        ),
        trailing: CodingSessionStatusDot(
          status: row.session.status,
          closed: settled,
        ),
        onTap: onTap,
      ),
    );
  }
}

/// The filter under a project's list: its one-line summary, a button that
/// opens the sheet, and the disclosure for what it hides.
class _ProjectFilterBar extends StatelessWidget {
  final Project project;
  final ProjectSessionFilter filter;
  final List<String> founders;
  final int hiddenUnattributed;
  final int hiddenByState;
  final String Function(String pubkey) ownerLabel;
  final String? myPubkey;
  final ValueChanged<ProjectSessionFilter> onChange;

  const _ProjectFilterBar({
    required this.project,
    required this.filter,
    required this.founders,
    required this.hiddenUnattributed,
    required this.hiddenByState,
    required this.ownerLabel,
    required this.myPubkey,
    required this.onChange,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final unattributed = projectSessionUnattributedNote(hiddenUnattributed);
    return Padding(
      padding: const EdgeInsets.fromLTRB(Grid.xs, Grid.half, Grid.xs, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          TextButton.icon(
            key: ValueKey('project-filter-${project.address}'),
            style: TextButton.styleFrom(
              padding: const EdgeInsets.symmetric(horizontal: Grid.xxs),
              visualDensity: VisualDensity.compact,
              foregroundColor: colors.onSurfaceVariant,
            ),
            onPressed: () => showBuzzModalBottomSheet<void>(
              context: context,
              title: 'Sessions in ${project.name}',
              isScrollControlled: true,
              builder: (_) => _ProjectFilterSheet(
                filter: filter,
                founders: founders,
                ownerLabel: ownerLabel,
                myPubkey: myPubkey,
                onChange: onChange,
              ),
            ),
            icon: const Icon(LucideIcons.listFilter, size: 14),
            label: Text(filter.label, style: context.textTheme.labelMedium),
          ),
          if (unattributed != null)
            Padding(
              padding: const EdgeInsets.only(left: Grid.xxs, top: Grid.quarter),
              child: Text(
                unattributed,
                key: ValueKey('project-unattributed-${project.address}'),
                style: context.textTheme.bodySmall?.copyWith(
                  color: colors.onSurfaceVariant,
                ),
              ),
            ),
          if (hiddenByState > 0)
            Padding(
              padding: const EdgeInsets.only(left: Grid.xxs, top: Grid.quarter),
              child: Text(
                '$hiddenByState ${hiddenByState == 1 ? 'session' : 'sessions'} '
                'hidden by the closed box or the date range.',
                key: ValueKey('project-hidden-by-state-${project.address}'),
                style: context.textTheme.bodySmall?.copyWith(
                  color: colors.onSurfaceVariant,
                ),
              ),
            ),
        ],
      ),
    );
  }
}
