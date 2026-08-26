part of '../coding_sessions_page.dart';

/// The first read is still in flight; nothing is claimed about the channel.
class _CodingSessionsLoading extends StatelessWidget {
  const _CodingSessionsLoading();

  @override
  Widget build(BuildContext context) => ListView(
    key: const ValueKey('coding-sessions-loading'),
    padding: const EdgeInsets.only(top: Grid.xxl),
    children: const [
      Center(
        child: BuzzLoadingIndicator(
          size: 40,
          semanticLabel: 'Reading coding sessions',
        ),
      ),
    ],
  );
}

/// A full-height message, kept scrollable so pull-to-refresh still works.
class _CodingSessionsMessage extends StatelessWidget {
  final IconData icon;
  final String title;
  final String detail;
  final Future<void> Function()? onRetry;

  const _CodingSessionsMessage({
    super.key,
    required this.icon,
    required this.title,
    required this.detail,
    this.onRetry,
  });

  @override
  Widget build(BuildContext context) => ListView(
    padding: const EdgeInsets.fromLTRB(Grid.gutter, Grid.xxl, Grid.gutter, 0),
    children: [
      Icon(icon, size: 32, color: context.colors.onSurfaceVariant),
      const SizedBox(height: Grid.twelve),
      Text(
        title,
        textAlign: TextAlign.center,
        style: context.textTheme.titleSmall,
      ),
      const SizedBox(height: Grid.half),
      Text(
        detail,
        textAlign: TextAlign.center,
        style: context.textTheme.bodySmall?.copyWith(
          color: context.colors.onSurfaceVariant,
        ),
      ),
      if (onRetry case final retry?) ...[
        const SizedBox(height: Grid.xs),
        Center(
          child: TextButton.icon(
            key: const ValueKey('coding-sessions-retry'),
            onPressed: retry,
            icon: const Icon(LucideIcons.refreshCw, size: 16),
            label: const Text('Retry'),
          ),
        ),
      ],
    ],
  );
}

/// A one-line disclosure the list appends, such as the trust-gate losses.
class _CodingSessionsNotice extends StatelessWidget {
  final String text;
  final bool emphasise;

  const _CodingSessionsNotice({
    super.key,
    required this.text,
    this.emphasise = false,
  });

  @override
  Widget build(BuildContext context) => Row(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      Icon(LucideIcons.info, size: 14, color: context.colors.onSurfaceVariant),
      const SizedBox(width: Grid.xxs),
      Expanded(
        child: Text(
          text,
          style: context.textTheme.bodySmall?.copyWith(
            color: emphasise
                ? context.colors.error
                : context.colors.onSurfaceVariant,
          ),
        ),
      ),
    ],
  );
}
