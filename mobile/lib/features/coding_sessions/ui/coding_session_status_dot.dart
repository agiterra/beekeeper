import 'package:flutter/material.dart';

import '../../../shared/theme/theme.dart';
import '../domain/coding_sessions_domain.dart';

/// The three states a session row's dot can paint.
///
/// Mirrors the desktop's project sidebar (`projectSessionIndicator.ts`):
/// green only while the provider last reported running, orange once the
/// session is closed, and the calm blue for everything else — a fresh
/// session, an idle one, a waiting one, or a status nobody has read.
enum CodingSessionDotState { running, idle, closed }

/// Which dot a folded status and a closure verdict add up to.
///
/// Closure facts (kind 44230) outrank provider metadata, exactly as on the
/// desktop: a closed session is orange whatever its provider last said.
CodingSessionDotState codingSessionDotState({
  required CodingSessionFoldedStatus status,
  required bool closed,
}) {
  if (closed) return CodingSessionDotState.closed;
  return status.kind == CodingSessionFoldedStatusKind.working
      ? CodingSessionDotState.running
      : CodingSessionDotState.idle;
}

/// The short state name a screen reader gets, and the hover repeats.
String codingSessionDotLabel(CodingSessionDotState state) => switch (state) {
  CodingSessionDotState.running => 'Running',
  CodingSessionDotState.idle => 'Idle',
  CodingSessionDotState.closed => 'Closed',
};

/// The long form for the tooltip.
///
/// Green is the one colour that claims activity, so it alone carries the
/// caveat the text chip used to: last reported by the provider, not a live
/// lease.
String codingSessionDotTitle(CodingSessionDotState state) => switch (state) {
  CodingSessionDotState.running =>
    'Running — last reported by the provider, not a live lease',
  CodingSessionDotState.idle => 'Idle',
  CodingSessionDotState.closed => 'Closed',
};

/// A session row's one-glance state, painted as a coloured dot.
///
/// Replaces the text chip in the project tree so the phone reads like the
/// desktop's sidebar. The dot never softens a verdict: it repeats the fold's
/// own state and names it for assistive tech.
class CodingSessionStatusDot extends StatelessWidget {
  /// The folded status the dot reads.
  final CodingSessionFoldedStatus status;

  /// Whether a closure fact has settled the session.
  final bool closed;

  /// Creates a dot for [status], overridden by [closed].
  const CodingSessionStatusDot({
    super.key,
    required this.status,
    required this.closed,
  });

  @override
  Widget build(BuildContext context) {
    final state = codingSessionDotState(status: status, closed: closed);
    final color = switch (state) {
      CodingSessionDotState.running => context.appColors.success,
      CodingSessionDotState.idle => context.colors.primary,
      CodingSessionDotState.closed => context.appColors.warning,
    };
    return Tooltip(
      message: codingSessionDotTitle(state),
      child: Semantics(
        label: codingSessionDotLabel(state),
        child: Container(
          key: const ValueKey('coding-session-status-dot'),
          width: Grid.xxs + Grid.quarter,
          height: Grid.xxs + Grid.quarter,
          decoration: BoxDecoration(color: color, shape: BoxShape.circle),
        ),
      ),
    );
  }
}
