import 'package:flutter/material.dart';

import '../../../shared/theme/theme.dart';
import '../domain/coding_sessions_domain.dart';

/// The four states a session row's dot can paint.
///
/// Mirrors the desktop's project sidebar (`projectSessionIndicator.ts`):
/// green only while the provider last reported running, orange once the
/// session is closed, a hollow ring for a session founded but never started,
/// and the calm blue (the scheme's tertiary, Catppuccin Blue in both modes —
/// primary is near-black here) for everything else — a fresh session, an idle
/// one, a waiting one, or a status nobody has read.
enum CodingSessionDotState { running, idle, founded, closed }

/// Which dot a folded status and a closure verdict add up to.
///
/// Closure facts (kind 44230) outrank provider metadata, exactly as on the
/// desktop: a closed session is orange whatever its provider last said — and
/// whether or not it was ever started. A founded session with no execution
/// is the ring, never the idle blue: idle claims something ran.
CodingSessionDotState codingSessionDotState({
  required CodingSessionFoldedStatus status,
  required bool closed,
}) {
  if (closed) return CodingSessionDotState.closed;
  return switch (status.kind) {
    CodingSessionFoldedStatusKind.working => CodingSessionDotState.running,
    CodingSessionFoldedStatusKind.founded => CodingSessionDotState.founded,
    _ => CodingSessionDotState.idle,
  };
}

/// The short state name a screen reader gets, and the hover repeats.
String codingSessionDotLabel(CodingSessionDotState state) => switch (state) {
  CodingSessionDotState.running => 'Running',
  CodingSessionDotState.idle => 'Idle',
  CodingSessionDotState.founded => 'Not started',
  CodingSessionDotState.closed => 'Closed',
};

/// The long form for the tooltip.
///
/// Green is the one colour that claims activity, so it alone carries the
/// caveat the text chip used to: last reported by the provider, not a live
/// lease. The ring says why there is nothing to report.
String codingSessionDotTitle(CodingSessionDotState state) => switch (state) {
  CodingSessionDotState.running =>
    'Running — last reported by the provider, not a live lease',
  CodingSessionDotState.idle => 'Idle',
  CodingSessionDotState.founded =>
    'Not started — founded, no provider has been asked to run it',
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
    final decoration = switch (state) {
      CodingSessionDotState.running => BoxDecoration(
        color: context.appColors.success,
        shape: BoxShape.circle,
      ),
      CodingSessionDotState.idle => BoxDecoration(
        color: context.colors.tertiary,
        shape: BoxShape.circle,
      ),
      // A hollow ring: the outline of a session, with nothing in it yet.
      CodingSessionDotState.founded => BoxDecoration(
        color: Colors.transparent,
        shape: BoxShape.circle,
        border: Border.all(color: context.colors.outline, width: 1.5),
      ),
      CodingSessionDotState.closed => BoxDecoration(
        color: context.appColors.warning,
        shape: BoxShape.circle,
      ),
    };
    return Tooltip(
      message: codingSessionDotTitle(state),
      child: Semantics(
        label: codingSessionDotLabel(state),
        child: Container(
          key: const ValueKey('coding-session-status-dot'),
          width: Grid.xxs + Grid.quarter,
          height: Grid.xxs + Grid.quarter,
          decoration: decoration,
        ),
      ),
    );
  }
}
