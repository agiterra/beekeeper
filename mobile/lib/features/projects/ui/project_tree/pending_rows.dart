part of '../project_tree.dart';

/// What a person can do about a refusal, when its code says so.
///
/// `PROJECT_CWD_UNRESOLVED` is the one a phone-issued create hits first: the
/// provider's own words name the project and channel that have no directory,
/// but not that the directory is the desktop's to record. Live finding
/// 2026-09-08 — the refusal was read on the relay while the phone still said
/// "waiting", and even once shown it would not have said where to fix it.
/// The desktop records the directory the first time it starts a session for
/// the project, so that is the other way to make the next phone create
/// resolve.
String? pendingCreateRefusalHint(CodingSessionReceiptError? error) =>
    switch (error?.code) {
      'PROJECT_CWD_UNRESOLVED' =>
        "Set the project's working directory in the desktop app's project "
            'settings, or start one session for it from the desktop, then '
            'try again.',
      _ => null,
    };

/// A create this device sent, with where it stands.
class _PendingCreateRow {
  final CodingSessionPendingCreate pending;
  final CodingSessionPendingCreatePhase phase;

  const _PendingCreateRow(this.pending, this.phase);
}

/// The row for a session that is being asked for: the title it will have,
/// what is being waited on, and — when the provider refused — its code and
/// words, with a way to dismiss them. Never a status chip: nothing is
/// running.
class _PendingCreateTile extends StatelessWidget {
  final _PendingCreateRow row;
  final String providerLabel;
  final VoidCallback onDismiss;

  const _PendingCreateTile({
    super.key,
    required this.row,
    required this.providerLabel,
    required this.onDismiss,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final failed = row.phase.kind == CodingSessionPendingCreateKind.failed;
    final error = row.phase.error;
    final detail = switch (row.phase.kind) {
      CodingSessionPendingCreateKind.sending => 'Sending…',
      CodingSessionPendingCreateKind.awaitingProvider =>
        'Waiting for $providerLabel to answer',
      CodingSessionPendingCreateKind.failed =>
        '$providerLabel refused'
            '${error == null ? '' : ' (${error.code}): ${error.message}'}'
            '${switch (pendingCreateRefusalHint(error)) {
              final hint? => ' $hint',
              null => '',
            }}',
      CodingSessionPendingCreateKind.created => 'Created',
    };
    return ListTile(
      dense: true,
      visualDensity: VisualDensity.compact,
      leading: failed
          ? Icon(LucideIcons.circleX, size: 18, color: colors.error)
          : const SizedBox(
              width: 18,
              height: 18,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
      title: Text(
        row.pending.title ?? 'New coding session',
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: context.textTheme.bodyMedium,
      ),
      subtitle: Text(
        detail,
        maxLines: failed ? 6 : 1,
        overflow: TextOverflow.ellipsis,
        style: context.textTheme.bodySmall?.copyWith(
          color: failed ? colors.error : colors.onSurfaceVariant,
        ),
      ),
      trailing: failed
          ? IconButton(
              key: ValueKey('project-pending-dismiss-${row.pending.commandId}'),
              tooltip: 'Dismiss',
              visualDensity: VisualDensity.compact,
              icon: const Icon(LucideIcons.x, size: 16),
              onPressed: onDismiss,
            )
          : null,
    );
  }
}
