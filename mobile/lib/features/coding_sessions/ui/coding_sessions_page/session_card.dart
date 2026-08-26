part of '../coding_sessions_page.dart';

/// One umbrella session in the channel list.
class _SessionCard extends StatelessWidget {
  final CodingSessionUmbrella session;
  final CodingSessionReachability reachability;
  final VoidCallback onOpen;

  const _SessionCard({
    required this.session,
    required this.reachability,
    required this.onOpen,
  });

  /// True only when the leases were read and none answered for a status that
  /// claims the provider is busy right now.
  bool get _noProviderAnswering =>
      reachability.kind == CodingSessionReachabilityKind.noProviderAnswering &&
      (session.status.status?.isLiveSounding ?? false);

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final now = DateTime.now();
    return Material(
      color: colors.surfaceContainerLow,
      borderRadius: BorderRadius.circular(Radii.card),
      child: InkWell(
        key: ValueKey('coding-session-card-${session.key}'),
        onTap: onOpen,
        borderRadius: BorderRadius.circular(Radii.card),
        child: Padding(
          padding: const EdgeInsets.all(Grid.twelve),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Expanded(
                    child: Text(
                      session.displayName,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: context.textTheme.titleSmall,
                    ),
                  ),
                  const SizedBox(width: Grid.xxs),
                  CodingSessionStatusChip(status: session.status),
                  if (session.closed) ...[
                    const SizedBox(width: Grid.half),
                    const _ClosedBadge(),
                  ],
                ],
              ),
              const SizedBox(height: Grid.half),
              _CardLine(
                icon: LucideIcons.userRound,
                text: codingSessionFounderLabel(session.founder),
              ),
              _CardLine(
                icon: LucideIcons.cpu,
                text: codingSessionExecutionsLabel(session.executions),
              ),
              _CardLine(
                icon: LucideIcons.clock,
                text:
                    'Last activity '
                    '${codingSessionAgoSince(session.lastActivityAt, now)}',
              ),
              if (_noProviderAnswering)
                const _CardLine(
                  key: ValueKey('coding-session-card-unreachable'),
                  icon: LucideIcons.plugZap,
                  text: 'No provider answering',
                  emphasise: true,
                ),
              if (session.hasUnverifiedAuthority)
                const _CardLine(
                  key: ValueKey('coding-session-card-authority-unverified'),
                  icon: LucideIcons.shieldAlert,
                  text: 'Authority unverified',
                  emphasise: true,
                ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ClosedBadge extends StatelessWidget {
  const _ClosedBadge();

  @override
  Widget build(BuildContext context) => Container(
    key: const ValueKey('coding-session-closed-badge'),
    padding: const EdgeInsets.symmetric(
      horizontal: Grid.xxs,
      vertical: Grid.quarter,
    ),
    decoration: BoxDecoration(
      color: context.colors.surfaceContainerHighest,
      borderRadius: BorderRadius.circular(Radii.full),
    ),
    child: Text(
      'Closed',
      style: context.textTheme.labelSmall?.copyWith(
        color: context.colors.onSurfaceVariant,
      ),
    ),
  );
}

class _CardLine extends StatelessWidget {
  final IconData icon;
  final String text;
  final bool emphasise;

  const _CardLine({
    super.key,
    required this.icon,
    required this.text,
    this.emphasise = false,
  });

  @override
  Widget build(BuildContext context) {
    final color = emphasise
        ? context.colors.error
        : context.colors.onSurfaceVariant;
    return Padding(
      padding: const EdgeInsets.only(top: Grid.quarter),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: Grid.quarter),
            child: Icon(icon, size: 14, color: color),
          ),
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
