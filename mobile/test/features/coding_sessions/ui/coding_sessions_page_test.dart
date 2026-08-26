import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/features/coding_sessions/ui/coding_session_page.dart';
import 'package:buzz/features/coding_sessions/ui/coding_session_status_chip.dart';
import 'package:buzz/features/coding_sessions/ui/coding_sessions_page.dart';
import 'package:buzz/features/coding_sessions/ui/observer_contract.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../../helpers/widget_helpers.dart';
import 'fake_observer.dart';

Future<void> _pump(WidgetTester tester, FakeObserverBinding binding) async {
  await tester.pumpWidget(
    WidgetHelpers.testable(
      overrides: [fakeObserverOverride(binding)],
      child: const CodingSessionsPage(
        channelId: testChannelId,
        channelName: 'general',
      ),
    ),
  );
  await tester.pump();
}

void main() {
  testWidgets('lists a session with name, status, founder and executions', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        sessions: [
          testUmbrella(
            executions: [
              testExecution(),
              testExecution(
                target: testTarget(sessionId: 'session-2'),
                status: CodingSessionStatus.idle,
              ),
            ],
          ),
        ],
      ),
    );

    await _pump(tester, binding);

    expect(find.text('Ship the observer'), findsOneWidget);
    expect(find.byType(CodingSessionStatusChip), findsOneWidget);
    expect(find.text('Working'), findsOneWidget);
    expect(find.text('Founder aaaaaaaa…'), findsOneWidget);
    expect(find.text('2 executions · claude-code · sonnet'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('coding-session-closed-badge')),
      findsNothing,
    );
  });

  testWidgets('marks a closed session as closed by its founder', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(sessions: [testUmbrella(closed: true)]),
    );

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-closed-badge')),
      findsOneWidget,
    );
    // `closed` now only survives the fold when the founder of the session's
    // own genesis signed it, so the badge may say so plainly.
    expect(find.text('Closed by founder'), findsOneWidget);
    expect(find.text('Closed by a member'), findsNothing);
  });

  testWidgets('marks a session whose authority no create vouched for', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        sessions: [
          testUmbrella(
            executions: [
              testExecution(),
              testExecution(
                target: testTarget(sessionId: 'session-2'),
                authorityVerified: false,
              ),
            ],
          ),
        ],
      ),
    );

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-card-authority-unverified')),
      findsOneWidget,
    );
    expect(find.text('Authority unverified'), findsOneWidget);
  });

  testWidgets('says nothing of the sort when every create vouched', (
    tester,
  ) async {
    final binding = FakeObserverBinding(testSnapshot());

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-card-authority-unverified')),
      findsNothing,
    );
    expect(find.text('Authority unverified'), findsNothing);
  });

  testWidgets('shows the empty state when the read returned no sessions', (
    tester,
  ) async {
    final binding = FakeObserverBinding(testSnapshot(sessions: const []));

    await _pump(tester, binding);

    expect(find.byKey(const ValueKey('coding-sessions-empty')), findsOneWidget);
    expect(find.text('No coding sessions in this channel'), findsOneWidget);
  });

  testWidgets('shows an error state with a retry that refreshes', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        sessions: const [],
        connection: CodingSessionObserverConnection.error,
        lastError: 'relay unreachable',
      ),
    );

    await _pump(tester, binding);

    expect(find.byKey(const ValueKey('coding-sessions-error')), findsOneWidget);
    expect(find.text('relay unreachable'), findsOneWidget);
    expect(find.byKey(const ValueKey('coding-sessions-empty')), findsNothing);

    await tester.tap(find.byKey(const ValueKey('coding-sessions-retry')));
    await tester.pump();
    expect(binding.refreshCount, 1);
  });

  testWidgets('a read that has not returned is not an empty channel', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        sessions: const [],
        connection: CodingSessionObserverConnection.connecting,
      ),
    );

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-sessions-loading')),
      findsOneWidget,
    );
    expect(find.text('No coding sessions in this channel'), findsNothing);
  });

  testWidgets('discloses what the trust gate refused', (tester) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        counts: const CodingSessionReadCounts(malformed: 2, conflicts: 1),
      ),
    );

    await _pump(tester, binding);

    expect(
      find.text('Dropped from this read: 2 malformed, 1 conflicting'),
      findsOneWidget,
    );
  });

  testWidgets('says so when a live-sounding session has no provider', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        reachabilityBySession: const {
          'umbrella-1': CodingSessionReachability(
            kind: CodingSessionReachabilityKind.noProviderAnswering,
          ),
        },
      ),
    );

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-card-unreachable')),
      findsOneWidget,
    );
  });

  testWidgets('an unknown reachability read never reads as nobody answering', (
    tester,
  ) async {
    final binding = FakeObserverBinding(testSnapshot());

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-card-unreachable')),
      findsNothing,
    );
  });

  testWidgets('discloses a failed read behind a list that still has rows', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        connection: CodingSessionObserverConnection.error,
        lastError: 'subscription dropped',
      ),
    );

    await _pump(tester, binding);

    expect(find.text('Ship the observer'), findsOneWidget);
    expect(
      find.text('This list may be out of date: subscription dropped'),
      findsOneWidget,
    );
  });

  testWidgets('opens the session page on tap', (tester) async {
    final binding = FakeObserverBinding(testSnapshot());

    await _pump(tester, binding);
    await tester.tap(
      find.byKey(const ValueKey('coding-session-card-umbrella-1')),
    );
    await tester.pumpAndSettle();

    expect(find.byType(CodingSessionPage), findsOneWidget);
    expect(find.text('Read-only on mobile'), findsOneWidget);
  });
}
