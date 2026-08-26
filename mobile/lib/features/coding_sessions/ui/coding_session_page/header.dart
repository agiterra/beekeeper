part of '../coding_session_page.dart';

/// Name, folded status, reachability, founder and executions.
class _SessionHeader extends StatelessWidget {
  final CodingSessionUmbrella session;
  final CodingSessionObserverSnapshot snapshot;

  const _SessionHeader({required this.session, required this.snapshot});

  @override
  Widget build(BuildContext context) {
    final now = DateTime.now();
    final reachability = snapshot.reachabilityFor(session.key);
    final reachabilityLine = codingSessionReachabilityLabel(
      reachability: reachability,
      status: session.status,
      statusAt: _statusAt,
      now: now,
    );
    return Container(
      key: const ValueKey('coding-session-header'),
      margin: const EdgeInsets.only(bottom: Grid.twelve),
      padding: const EdgeInsets.all(Grid.twelve),
      decoration: BoxDecoration(
        color: context.colors.surfaceContainerLow,
        borderRadius: BorderRadius.circular(Radii.card),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  session.displayName,
                  style: context.textTheme.titleSmall,
                ),
              ),
              const SizedBox(width: Grid.xxs),
              CodingSessionStatusChip(status: session.status),
            ],
          ),
          if (session.closed)
            _HeaderLine(
              key: const ValueKey('coding-session-closed'),
              icon: LucideIcons.circleCheck,
              text: 'Closed by its founder',
            ),
          if (reachabilityLine != null)
            _HeaderLine(
              key: const ValueKey('coding-session-reachability'),
              icon: LucideIcons.plugZap,
              text: reachabilityLine,
              emphasise:
                  reachability.kind ==
                  CodingSessionReachabilityKind.noProviderAnswering,
            ),
          _HeaderLine(
            icon: LucideIcons.userRound,
            text: codingSessionFounderLabel(session.founder),
          ),
          if (session.goal case final goal?)
            _HeaderLine(
              key: const ValueKey('coding-session-goal'),
              icon: LucideIcons.target,
              text: goal,
            ),
          const SizedBox(height: Grid.xxs),
          for (final execution in session.executions)
            _ExecutionLine(
              key: ValueKey('coding-session-execution-${execution.targetKey}'),
              execution: execution,
            ),
        ],
      ),
    );
  }

  /// When the status the fold settled on was reported.
  int? get _statusAt {
    final folded = session.status.status;
    int? best;
    for (final execution in session.executions) {
      if (folded != null && execution.status != folded) continue;
      final at = execution.statusAt ?? execution.lastActivityAt;
      if (best == null || at > best) best = at;
    }
    return best ?? session.lastActivityAt;
  }
}

/// One execution of the session, with the facts its provider signed.
class _ExecutionLine extends StatelessWidget {
  final CodingSessionExecution execution;

  const _ExecutionLine({super.key, required this.execution});

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final parts = <String>[
      execution.label,
      'gen ${execution.target.generation}',
      codingSessionStatusWords(execution.status),
    ];
    return Padding(
      padding: const EdgeInsets.only(top: Grid.half),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: Grid.quarter),
            child: Icon(
              LucideIcons.cpu,
              size: 14,
              color: colors.onSurfaceVariant,
            ),
          ),
          const SizedBox(width: Grid.xxs),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  parts.join(' · '),
                  style: context.textTheme.bodySmall?.copyWith(
                    color: colors.onSurfaceVariant,
                  ),
                ),
                Text(
                  'signer ${shortPubkey(execution.signerPubkey)}'
                  '${execution.authority.verified ? '' : ' · authority unverified'}'
                  '${execution.statusConflict ? ' · status conflict' : ''}',
                  style: context.textTheme.bodySmall?.copyWith(
                    color: execution.authority.verified
                        ? colors.onSurfaceVariant
                        : colors.error,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _HeaderLine extends StatelessWidget {
  final IconData icon;
  final String text;
  final bool emphasise;

  const _HeaderLine({
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
      padding: const EdgeInsets.only(top: Grid.half),
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
