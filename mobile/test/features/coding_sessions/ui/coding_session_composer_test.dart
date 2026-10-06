import 'dart:convert';

import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/features/coding_sessions/ui/coding_session_labels.dart';
import 'package:beekeeper/features/coding_sessions/ui/coding_session_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../../helpers/widget_helpers.dart';
import 'fake_observer.dart';

const _genesis =
    'aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd';

const _founder = CodingSessionFounder(
  pubkey: testSignerPubkey,
  resolution: CodingSessionFounderResolution.genesis,
  genesisRef: _genesis,
);

Future<void> _pump(WidgetTester tester, FakeObserverBinding binding) async {
  await tester.pumpWidget(
    WidgetHelpers.testable(
      overrides: [fakeObserverOverride(binding)],
      child: CodingSessionPage(
        channelId: testChannelId,
        sessionKey: 'umbrella-1',
      ),
    ),
  );
  await tester.pump();
}

CodingSessionUmbrella _session({
  CodingSessionStatus status = CodingSessionStatus.idle,
  int generation = 1,
  Map<String, bool> capabilities = const {},
}) {
  final execution = testExecution(
    target: testTarget(generation: generation),
    status: status,
  );
  return testUmbrella(
    sessionRef: '6f1c9a52-0f2f-4f7e-8a5b-2c1d0e9f8a7b',
    founder: _founder,
    executions: [
      CodingSessionExecution(
        channelId: execution.channelId,
        target: execution.target,
        authority: execution.authority,
        status: execution.status,
        statusAt: execution.statusAt,
        metadata: CodingSessionMetadata(
          ref: execution.metadata!.ref,
          target: execution.target,
          status: status,
          capabilities: capabilities,
          canonicalPayload: '{}',
          runtime: 'claude-code',
          model: 'sonnet',
          sessionRef: '6f1c9a52-0f2f-4f7e-8a5b-2c1d0e9f8a7b',
        ),
        sessionRef: '6f1c9a52-0f2f-4f7e-8a5b-2c1d0e9f8a7b',
        isCurrentGeneration: true,
        statusConflict: false,
        lastActivityAt: 1000,
        commandId: 'command-1',
      ),
    ],
    status: CodingSessionFoldedStatus(
      kind: status == CodingSessionStatus.running
          ? CodingSessionFoldedStatusKind.working
          : CodingSessionFoldedStatusKind.reported,
      status: status,
    ),
  );
}

String _commandId(FakeObserverBinding binding) =>
    (jsonDecode(binding.relay.published.last.content)
            as Map<String, dynamic>)['commandId']
        as String;

void main() {
  testWidgets('a non-founder sees the disclosure and no composer', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(sessions: [_session()]),
      signerPubkey: testOperatorPubkey,
    );
    await _pump(tester, binding);

    expect(
      find.text(
        codingSessionSteerDisclosure(CodingSessionSteerStanding.notFounder),
      ),
      findsOneWidget,
    );
    expect(find.byKey(const ValueKey('coding-session-composer')), findsNothing);
    expect(find.byKey(const ValueKey('coding-session-actions')), findsNothing);
  });

  testWidgets('the founder sends a turn: the row is pending until a receipt '
      'names its command', (tester) async {
    final binding = FakeObserverBinding(
      testSnapshot(sessions: [_session()]),
      signerPubkey: testSignerPubkey,
    );
    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-composer')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey('coding-session-steer-disclosure')),
      findsNothing,
    );
    expect(
      find.byKey(const ValueKey('coding-session-interrupt')),
      findsNothing,
    );
    expect(find.text('Send · claude-code · sonnet · gen 1'), findsOneWidget);

    await tester.enterText(
      find.byKey(const ValueKey('coding-session-composer-field')),
      'Add the observer',
    );
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('coding-session-send')));
    await tester.pumpAndSettle();

    final event = binding.relay.published.single;
    expect(event.kind, 44220);
    expect((jsonDecode(event.content) as Map<String, dynamic>)['action'], {
      'type': 'thread.turn.start',
      'text': 'Add the observer',
    });
    final commandId = _commandId(binding);
    expect(
      find.byKey(ValueKey('coding-session-pending-$commandId')),
      findsOneWidget,
    );
    expect(
      find.text('Sent · waiting for the provider\'s receipt'),
      findsOneWidget,
    );
    expect(find.text('Add the observer'), findsOneWidget);
    // The editor cleared before the relay answered.
    final field = tester.widget<TextField>(
      find.byKey(const ValueKey('coding-session-composer-field')),
    );
    expect(field.controller!.text, isEmpty);
    // Acceptance marks the session as steerable from this key.
    expect(binding.steerAccepted, contains('umbrella-1'));

    // A queued receipt describes the row; a started one settles it.
    binding.snapshot = testSnapshot(
      sessions: [_session()],
      turnReceiptsByCommandId: {
        commandId: [
          testTurnReceipt(
            commandId: commandId,
            status: CodingSessionReceiptStatus.turnQueued,
          ),
        ],
      },
    );
    await tester.pumpWidget(
      WidgetHelpers.testable(
        overrides: [fakeObserverOverride(binding)],
        child: CodingSessionPage(
          channelId: testChannelId,
          sessionKey: 'umbrella-1',
        ),
      ),
    );
    await tester.pump();
    expect(find.text('Queued for the next turn'), findsOneWidget);

    binding.snapshot = testSnapshot(
      sessions: [_session()],
      turnReceiptsByCommandId: {
        commandId: [
          testTurnReceipt(
            commandId: commandId,
            status: CodingSessionReceiptStatus.turnStarted,
            createdAt: 2001,
          ),
        ],
      },
    );
    await tester.pumpWidget(
      WidgetHelpers.testable(
        overrides: [fakeObserverOverride(binding)],
        child: CodingSessionPage(
          channelId: testChannelId,
          sessionKey: 'umbrella-1',
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(
      find.byKey(ValueKey('coding-session-pending-$commandId')),
      findsNothing,
    );
  });

  testWidgets('a refused turn hands the words back and offers the successor '
      'generation', (tester) async {
    final binding = FakeObserverBinding(
      testSnapshot(sessions: [_session()]),
      signerPubkey: testSignerPubkey,
    );
    await _pump(tester, binding);
    await tester.enterText(
      find.byKey(const ValueKey('coding-session-composer-field')),
      'try again',
    );
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('coding-session-send')));
    await tester.pumpAndSettle();
    final commandId = _commandId(binding);

    // The session resumed as generation 2 and the provider refused the turn
    // aimed at generation 1.
    binding.snapshot = testSnapshot(
      sessions: [_session(generation: 2)],
      turnReceiptsByCommandId: {
        commandId: [
          testTurnReceipt(
            commandId: commandId,
            status: CodingSessionReceiptStatus.turnRefused,
            code: 'STALE_GENERATION',
            message: 'generation 1 is not live',
          ),
        ],
      },
    );
    await tester.pumpWidget(
      WidgetHelpers.testable(
        overrides: [fakeObserverOverride(binding)],
        child: CodingSessionPage(
          channelId: testChannelId,
          sessionKey: 'umbrella-1',
        ),
      ),
    );
    await tester.pump();

    expect(
      find.text('Refused — STALE_GENERATION: generation 1 is not live'),
      findsOneWidget,
    );
    await tester.tap(
      find.byKey(ValueKey('coding-session-pending-readdress-$commandId')),
    );
    await tester.pumpAndSettle();

    expect(
      find.byKey(ValueKey('coding-session-pending-$commandId')),
      findsNothing,
    );
    final field = tester.widget<TextField>(
      find.byKey(const ValueKey('coding-session-composer-field')),
    );
    expect(field.controller!.text, 'try again');
    expect(find.text('Send · claude-code · sonnet · gen 2'), findsOneWidget);
  });

  testWidgets('a working execution offers Interrupt and Send next, and '
      'Interrupt publishes the interrupt action', (tester) async {
    final binding = FakeObserverBinding(
      testSnapshot(sessions: [_session(status: CodingSessionStatus.running)]),
      signerPubkey: testSignerPubkey,
    );
    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-interrupt')),
      findsOneWidget,
    );
    expect(
      find.text('Send next · claude-code · sonnet · gen 1'),
      findsOneWidget,
    );

    await tester.tap(find.byKey(const ValueKey('coding-session-interrupt')));
    await tester.pumpAndSettle();

    final event = binding.relay.published.single;
    expect(event.kind, 44220);
    expect((jsonDecode(event.content) as Map<String, dynamic>)['action'], {
      'type': 'thread.turn.interrupt',
    });
  });

  testWidgets('Steer is offered only when the runtime advertised it, and '
      'then the turn carries deliver=steer', (tester) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        sessions: [
          _session(
            status: CodingSessionStatus.running,
            capabilities: const {'threadSteer': true},
          ),
        ],
      ),
      signerPubkey: testSignerPubkey,
    );
    await _pump(tester, binding);
    expect(find.text('Steer · claude-code · sonnet · gen 1'), findsOneWidget);

    await tester.enterText(
      find.byKey(const ValueKey('coding-session-composer-field')),
      'stop, use the other file',
    );
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('coding-session-send')));
    await tester.pumpAndSettle();

    final action =
        (jsonDecode(binding.relay.published.single.content)
                as Map<String, dynamic>)['action']
            as Map<String, dynamic>;
    expect(action['deliver'], 'steer');
  });

  testWidgets(
    'a relay refusal restores the draft and shows the words verbatim',
    (tester) async {
      final binding = FakeObserverBinding(
        testSnapshot(sessions: [_session()]),
        signerPubkey: testSignerPubkey,
        publishResults: [Exception('restricted: not yours')],
      );
      await _pump(tester, binding);
      await tester.enterText(
        find.byKey(const ValueKey('coding-session-composer-field')),
        'hello',
      );
      await tester.pump();
      await tester.tap(find.byKey(const ValueKey('coding-session-send')));
      await tester.pumpAndSettle();

      expect(find.text('restricted: not yours'), findsOneWidget);
      final field = tester.widget<TextField>(
        find.byKey(const ValueKey('coding-session-composer-field')),
      );
      expect(field.controller!.text, 'hello');
      expect(
        find.byKey(const ValueKey('coding-session-pending-edit-')),
        findsNothing,
      );
      expect(find.textContaining('Sending'), findsNothing);
      expect(binding.steerAccepted, isNot(contains('umbrella-1')));
    },
  );

  testWidgets('with every generation stopped the composer says so', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(sessions: [_session(status: CodingSessionStatus.stopped)]),
      signerPubkey: testSignerPubkey,
    );
    await _pump(tester, binding);
    expect(
      find.byKey(const ValueKey('coding-session-composer-unavailable')),
      findsOneWidget,
    );
    expect(find.text(codingSessionNoLiveExecutionLabel), findsOneWidget);
  });

  testWidgets('the menu renames, sets a goal, closes, and stops with a '
      'confirmation', (tester) async {
    final binding = FakeObserverBinding(
      testSnapshot(sessions: [_session(status: CodingSessionStatus.running)]),
      signerPubkey: testSignerPubkey,
    );
    await _pump(tester, binding);

    // Rename.
    await tester.tap(find.byKey(const ValueKey('coding-session-actions')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Rename'));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const ValueKey('coding-session-sheet-field')),
      'Keystone lead',
    );
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('coding-session-sheet-save')));
    await tester.pumpAndSettle();
    expect(binding.relay.published.last.kind, 44229);
    expect(binding.relay.published.last.content, 'Keystone lead');
    expect(
      find.byKey(const ValueKey('coding-session-sheet-field')),
      findsNothing,
    );

    // Goal.
    await tester.tap(find.byKey(const ValueKey('coding-session-actions')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Set goal'));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const ValueKey('coding-session-sheet-field')),
      'Ship the terminal viewer',
    );
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('coding-session-sheet-save')));
    await tester.pumpAndSettle();
    expect(binding.relay.published.last.kind, 44227);

    // Close needs a confirmation and names the genesis.
    await tester.tap(find.byKey(const ValueKey('coding-session-actions')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Close'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('coding-session-confirm')));
    await tester.pumpAndSettle();
    expect(binding.relay.published.last.kind, 44230);
    expect(binding.relay.published.last.tags[3], ['cscl-genesis', _genesis]);

    // Stop needs a confirmation and names the provider authority.
    await tester.tap(find.byKey(const ValueKey('coding-session-actions')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Stop execution'));
    await tester.pumpAndSettle();
    expect(find.text('Stop execution'), findsWidgets);
    await tester.tap(find.byKey(const ValueKey('coding-session-confirm')));
    await tester.pumpAndSettle();
    final stop = binding.relay.published.last;
    expect(stop.kind, 44221);
    final action =
        (jsonDecode(stop.content) as Map<String, dynamic>)['action']
            as Map<String, dynamic>;
    expect(action['type'], 'session.stop');
    expect(action['providerAuthorityPubkey'], testSignerPubkey);
  });

  testWidgets('closing is disabled when the founder was not read from a '
      'genesis', (tester) async {
    final legacy = testUmbrella(
      sessionRef: '6f1c9a52-0f2f-4f7e-8a5b-2c1d0e9f8a7b',
      founder: const CodingSessionFounder(
        pubkey: testSignerPubkey,
        resolution: CodingSessionFounderResolution.legacy,
      ),
    );
    final binding = FakeObserverBinding(
      testSnapshot(sessions: [legacy]),
      signerPubkey: testSignerPubkey,
    );
    await _pump(tester, binding);
    await tester.tap(find.byKey(const ValueKey('coding-session-actions')));
    await tester.pumpAndSettle();

    final item = tester.widget<PopupMenuItem<Object?>>(
      find.byKey(const ValueKey('coding-session-action-closure')),
    );
    expect(item.enabled, isFalse);
    expect(
      find.text('No readable genesis — closing needs its id'),
      findsOneWidget,
    );
  });
}
