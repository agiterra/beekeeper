import 'package:flutter/material.dart';

import '../../../shared/theme/theme.dart';
import '../domain/coding_sessions_domain.dart';
import 'coding_session_labels.dart';

/// A compact chip stating a session's folded status (D8).
///
/// The chip repeats the fold's own verdict and nothing more: it never softens
/// a `failed` into a neutral colour or dresses an unknown read as activity.
class CodingSessionStatusChip extends StatelessWidget {
  /// The folded status to state.
  final CodingSessionFoldedStatus status;

  /// Creates a status chip for [status].
  const CodingSessionStatusChip({super.key, required this.status});

  @override
  Widget build(BuildContext context) {
    final palette = _palette(context);
    return Container(
      key: const ValueKey('coding-session-status-chip'),
      padding: const EdgeInsets.symmetric(
        horizontal: Grid.xxs,
        vertical: Grid.quarter,
      ),
      decoration: BoxDecoration(
        color: palette.$2,
        borderRadius: BorderRadius.circular(Radii.full),
      ),
      child: Text(
        codingSessionStatusLabel(status),
        style: context.textTheme.labelSmall?.copyWith(
          color: palette.$1,
          fontWeight: FontWeight.w600,
        ),
      ),
    );
  }

  (Color, Color) _palette(BuildContext context) {
    final colors = context.colors;
    final app = context.appColors;
    switch (status.kind) {
      case CodingSessionFoldedStatusKind.working:
        return (app.success, app.success.withValues(alpha: 0.14));
      case CodingSessionFoldedStatusKind.waiting:
        return (app.warning, app.warning.withValues(alpha: 0.16));
      case CodingSessionFoldedStatusKind.ended:
        return (colors.onSurfaceVariant, colors.surfaceContainerHighest);
      case CodingSessionFoldedStatusKind.unknown:
        return (colors.onSurfaceVariant, colors.surfaceContainerHighest);
      case CodingSessionFoldedStatusKind.reported:
        final reported = status.status;
        if (reported == CodingSessionStatus.failed ||
            reported == CodingSessionStatus.disconnected) {
          return (colors.error, colors.errorContainer);
        }
        return (colors.onSurfaceVariant, colors.surfaceContainerHighest);
    }
  }
}
