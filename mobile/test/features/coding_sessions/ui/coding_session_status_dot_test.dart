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
  const founded = CodingSessionFoldedStatus(
    kind: CodingSessionFoldedStatusKind.founded,
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

    test('founded is its own state, and a closure still outranks it', () {
      expect(
        codingSessionDotState(status: founded, closed: false),
        CodingSessionDotState.founded,
      );
      expect(
        codingSessionDotState(status: founded, closed: true),
        CodingSessionDotState.closed,
      );
      expect(
        codingSessionDotLabel(CodingSessionDotState.founded),
        'Not started',
      );
      expect(
        codingSessionDotTitle(CodingSessionDotState.founded),
        'Not started — founded, no provider has been asked to run it',
      );
    });
  });

  testWidgets('a founded session is a hollow ring named Not started', (
    tester,
  ) async {
    await tester.pumpWidget(
      WidgetHelpers.testable(
        child: const CodingSessionStatusDot(status: founded, closed: false),
      ),
    );
    expect(find.bySemanticsLabel('Not started'), findsOneWidget);
    expect(find.bySemanticsLabel('Idle'), findsNothing);
    final dot = tester.widget<Container>(
      find.byKey(const ValueKey('coding-session-status-dot')),
    );
    final decoration = dot.decoration! as BoxDecoration;
    expect(decoration.color, Colors.transparent);
    expect(decoration.border, isNotNull);
    expect(decoration.shape, BoxShape.circle);
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
