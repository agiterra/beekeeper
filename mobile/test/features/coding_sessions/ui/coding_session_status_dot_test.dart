import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/features/coding_sessions/ui/coding_session_status_dot.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../../helpers/widget_helpers.dart';

void main() {
  const working = CodingSessionFoldedStatus(
    kind: CodingSessionFoldedStatusKind.working,
    status: CodingSessionStatus.running,
  );
  const unread = CodingSessionFoldedStatus(
    kind: CodingSessionFoldedStatusKind.reported,
    status: CodingSessionStatus.unknown,
  );
  const waiting = CodingSessionFoldedStatus(
    kind: CodingSessionFoldedStatusKind.waiting,
    status: CodingSessionStatus.waitingForInput,
  );

  group('codingSessionDotState', () {
    test('green only for working; everything else unclosed is idle', () {
      expect(
        codingSessionDotState(status: working, closed: false),
        CodingSessionDotState.running,
      );
      expect(
        codingSessionDotState(status: unread, closed: false),
        CodingSessionDotState.idle,
      );
      expect(
        codingSessionDotState(status: waiting, closed: false),
        CodingSessionDotState.idle,
      );
    });

    test('a closure outranks whatever the provider last said', () {
      expect(
        codingSessionDotState(status: working, closed: true),
        CodingSessionDotState.closed,
      );
    });
  });

  testWidgets('the dot names its state for assistive tech', (tester) async {
    await tester.pumpWidget(
      WidgetHelpers.testable(
        child: const Row(
          children: [
            CodingSessionStatusDot(status: working, closed: false),
            CodingSessionStatusDot(status: unread, closed: false),
            CodingSessionStatusDot(status: working, closed: true),
          ],
        ),
      ),
    );
    expect(find.bySemanticsLabel('Running'), findsOneWidget);
    expect(find.bySemanticsLabel('Idle'), findsOneWidget);
    expect(find.bySemanticsLabel('Closed'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('coding-session-status-dot')),
      findsNWidgets(3),
    );
    expect(find.text('unknown'), findsNothing);
    expect(find.text('Unknown'), findsNothing);
  });
}
