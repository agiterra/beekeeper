part of '../terminal_observe_page.dart';

/// Whose terminal, what this viewer may do, the grid, and the stream status.
class _ObserveHeader extends StatelessWidget {
  /// The owner's name, or `null` for the viewer's own terminal.
  final String? ownerName;
  final String? role;
  final bool closed;
  final bool canType;
  final ShellObserverStatus status;
  final ShellDims dims;
  final String? watchError;

  const _ObserveHeader({
    required this.ownerName,
    required this.role,
    required this.closed,
    required this.canType,
    required this.status,
    required this.dims,
    required this.watchError,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    return Container(
      key: const ValueKey('terminal-observe-header'),
      padding: const EdgeInsets.fromLTRB(Grid.xs, Grid.xxs, Grid.xs, Grid.xxs),
      color: colors.surfaceContainerLow,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  ownerName == null
                      ? 'Your terminal · $dims'
                      : '$ownerName’s terminal · $dims',
                  style: context.textTheme.titleSmall,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              const SizedBox(width: Grid.xxs),
              _StatusChip(status: status),
            ],
          ),
          if (closed || role != null)
            Padding(
              padding: const EdgeInsets.only(top: Grid.quarter),
              child: Row(
                children: [
                  Icon(
                    canType ? LucideIcons.keyboard : LucideIcons.eye,
                    size: 14,
                    color: colors.onSurfaceVariant,
                  ),
                  const SizedBox(width: Grid.xxs),
                  Expanded(
                    child: Text(
                      closed
                          ? 'The owner closed or unshared this terminal'
                          : canType
                          ? '$role — your keystrokes go to this terminal'
                          : '$role — read-only',
                      key: const ValueKey('terminal-observe-role'),
                      style: context.textTheme.bodySmall?.copyWith(
                        color: colors.onSurfaceVariant,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          if (watchError case final error?)
            Padding(
              padding: const EdgeInsets.only(top: Grid.quarter),
              child: Text(
                'Watch not delivered: $error',
                key: const ValueKey('terminal-observe-watch-error'),
                style: context.textTheme.bodySmall?.copyWith(
                  color: colors.error,
                ),
              ),
            ),
        ],
      ),
    );
  }
}

/// The desktop's four badges, plus one for a relay that is not connected.
String terminalObserveStatusLabel(ShellObserverStatus status) =>
    switch (status) {
      ShellObserverStatus.connecting => 'Connecting…',
      ShellObserverStatus.live => 'LIVE',
      ShellObserverStatus.stalled => 'Not streaming',
      ShellObserverStatus.ended => 'Ended',
      ShellObserverStatus.offline => 'Offline',
    };

class _StatusChip extends StatelessWidget {
  final ShellObserverStatus status;

  const _StatusChip({required this.status});

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final (Color fg, Color bg) = switch (status) {
      ShellObserverStatus.live => (
        context.appColors.success,
        context.appColors.success.withValues(alpha: 0.15),
      ),
      ShellObserverStatus.stalled => (
        context.appColors.warning,
        context.appColors.warning.withValues(alpha: 0.15),
      ),
      _ => (colors.onSurfaceVariant, colors.surfaceContainerHighest),
    };
    return Container(
      key: ValueKey('terminal-observe-status-${status.name}'),
      padding: const EdgeInsets.symmetric(
        horizontal: Grid.xxs,
        vertical: Grid.quarter,
      ),
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(Radii.full),
        border: Border.all(color: fg.withValues(alpha: 0.4)),
      ),
      child: Text(
        terminalObserveStatusLabel(status),
        style: context.textTheme.labelSmall?.copyWith(
          color: fg,
          fontWeight: FontWeight.w600,
        ),
      ),
    );
  }
}
