import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/features/coding_sessions/ui/coding_session_labels.dart';
import 'package:beekeeper/features/coding_sessions/ui/coding_session_page.dart';
import 'package:beekeeper/features/coding_sessions/ui/observer_contract.dart';
import 'package:beekeeper/shared/relay/nostr_models.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../../helpers/widget_helpers.dart';
import 'fake_observer.dart';

Future<void> _pump(
  WidgetTester tester,
  FakeObserverBinding binding, {
  String sessionKey = 'umbrella-1',
}) async {
  await tester.pumpWidget(
    WidgetHelpers.testable(
      overrides: [fakeObserverOverride(binding)],
      child: CodingSessionPage(
        channelId: testChannelId,
        sessionKey: sessionKey,
      ),
    ),
  );
  await tester.pump();
}

List<CodingSessionTranscriptEnvelope> _conversation() => [
  testEnvelope(
    eventSeq: 1,
    item: const {
      'kind': 'user_prompt',
      'content': 'Add the observer',
      'operatorPubkey': testOperatorPubkey,
      'commandId': 'command-abcdef01',
    },
  ),
  testEnvelope(
    eventSeq: 2,
    item: const {'kind': 'assistant_text', 'text': 'On it.'},
  ),
];

void main() {
  testWidgets('renders a prompt and an assistant reply in order', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(envelopes: _conversation()),
    );

    await _pump(tester, binding);

    expect(find.text('Prompt'), findsOneWidget);
    expect(find.text('Add the observer'), findsOneWidget);
    expect(find.text('Response'), findsOneWidget);
    expect(find.text('On it.'), findsOneWidget);
    expect(find.text('operator 11111111… · command command-…'), findsOneWidget);
  });

  testWidgets('names the founder as the one who closed the session', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(sessions: [testUmbrella(closed: true)]),
    );

    await _pump(tester, binding);

    // The fold only keeps a closure the session's founder signed, so the
    // header states that rather than hedging about "a member".
    expect(find.byKey(const ValueKey('coding-session-closed')), findsOneWidget);
    expect(find.text('Closed by its founder'), findsOneWidget);
    expect(find.text('Closed by a member of this channel'), findsNothing);
  });

  testWidgets('folds a tool row and expands it to args and result', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        envelopes: [
          testEnvelope(
            eventSeq: 1,
            item: const {
              'kind': 'tool_call',
              'tool': {
                'toolName': 'read_file',
                'toolId': 'tool-1',
                'input': {'path': 'lib/main.dart'},
              },
            },
          ),
          testEnvelope(
            eventSeq: 2,
            item: const {
              'kind': 'tool_result',
              'toolId': 'tool-1',
              'content': 'void main() {}',
            },
          ),
        ],
      ),
    );

    await _pump(tester, binding);

    expect(find.text('read_file'), findsOneWidget);
    expect(find.text('path=lib/main.dart'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('coding-session-tool-result-event-1')),
      findsNothing,
    );

    await tester.tap(
      find.byKey(const ValueKey('coding-session-tool-toggle-event-1')),
    );
    await tester.pump();

    expect(
      find.byKey(const ValueKey('coding-session-tool-args-event-1')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey('coding-session-tool-result-event-1')),
      findsOneWidget,
    );
  });

  testWidgets('an elided row names the reason and never the payload', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        envelopes: [
          testEnvelope(
            eventSeq: 1,
            item: const {
              'kind': 'elided',
              'reason': 'too_large',
              'byteCount': 4096,
              'content': 'the-secret-payload',
            },
          ),
        ],
      ),
    );

    await _pump(tester, binding);

    expect(find.text('Content elided'), findsOneWidget);
    expect(find.text('reason: too_large · byteCount: 4096'), findsOneWidget);
    expect(find.textContaining('the-secret-payload'), findsNothing);
  });

  testWidgets('an unknown item kind is named and carries no payload', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        envelopes: [
          testEnvelope(
            eventSeq: 1,
            item: const {'kind': 'quantum_thing', 'text': 'unreviewed'},
          ),
        ],
      ),
    );

    await _pump(tester, binding);

    expect(find.text('Unrecognized item kind: quantum_thing'), findsOneWidget);
    expect(find.textContaining('unreviewed'), findsNothing);
  });

  testWidgets('separates turns', (tester) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        envelopes: [
          testEnvelope(
            eventSeq: 1,
            turnId: 'turn-1',
            item: const {'kind': 'user_prompt', 'content': 'first'},
          ),
          testEnvelope(
            eventSeq: 2,
            turnId: 'turn-2',
            item: const {'kind': 'user_prompt', 'content': 'second'},
          ),
        ],
      ),
    );

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-turn-separator')),
      findsOneWidget,
    );
  });

  testWidgets('reasoning is present but folded until opened', (tester) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        envelopes: [
          testEnvelope(
            eventSeq: 1,
            item: const {'kind': 'reasoning', 'text': 'weighing options'},
          ),
        ],
      ),
    );

    await _pump(tester, binding);

    expect(find.text('Reasoning'), findsOneWidget);
    expect(find.text('weighing options'), findsNothing);

    await tester.tap(
      find.byKey(const ValueKey('coding-session-reasoning-toggle-event-1')),
    );
    await tester.pump();

    expect(find.text('weighing options'), findsOneWidget);
  });

  testWidgets('a turn result carries duration and cost as fields', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        envelopes: [
          testEnvelope(
            eventSeq: 1,
            item: const {
              'kind': 'result',
              'subtype': 'success',
              'durationMs': 1200,
              'costUsd': 0.0125,
              'isError': false,
            },
          ),
        ],
      ),
    );

    await _pump(tester, binding);

    expect(find.text('Turn result'), findsOneWidget);
    expect(find.text('success · 1200ms · \$0.0125'), findsOneWidget);
  });

  testWidgets('ends with the steer disclosure when this device holds no key', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(envelopes: _conversation()),
    );

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-steer-disclosure')),
      findsOneWidget,
    );
    expect(
      find.text(codingSessionSteerDisclosure(CodingSessionSteerStanding.noKey)),
      findsOneWidget,
    );
  });

  testWidgets('discloses that signatures were not verified here', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(signaturesVerified: false),
    );

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-signatures-unverified')),
      findsOneWidget,
    );
    expect(find.text('Signatures not verified on this device'), findsOneWidget);
  });

  testWidgets('stays silent about signatures it did verify', (tester) async {
    final binding = FakeObserverBinding(testSnapshot());

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-signatures-unverified')),
      findsNothing,
    );
  });

  testWidgets('reports a stale provider against the last reported status', (
    tester,
  ) async {
    final nowSeconds = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final binding = FakeObserverBinding(
      testSnapshot(
        sessions: [
          testUmbrella(
            lastActivityAt: nowSeconds - 180,
            executions: [
              testExecution(
                lastActivityAt: nowSeconds - 180,
                statusAt: nowSeconds - 180,
              ),
            ],
          ),
        ],
        reachabilityBySession: const {
          'umbrella-1': CodingSessionReachability(
            kind: CodingSessionReachabilityKind.noProviderAnswering,
            leaseSequence: 4,
          ),
        },
      ),
    );

    await _pump(tester, binding);

    expect(
      find.text('No provider answering · last reported running 3m ago'),
      findsOneWidget,
    );
  });

  testWidgets('an unread lease query reads as unknown, not as a denial', (
    tester,
  ) async {
    final binding = FakeObserverBinding(testSnapshot());

    await _pump(tester, binding);

    expect(find.text('Provider reachability unknown'), findsOneWidget);
    expect(find.textContaining('No provider answering'), findsNothing);
  });

  testWidgets('marks an execution no create vouched for', (tester) async {
    // D5: with no readable create the facts come from the first-seen metadata
    // signer. The reader is shown facts nobody signed a mandate for, and this
    // line is the only place the page says so.
    final binding = FakeObserverBinding(
      testSnapshot(
        sessions: [
          testUmbrella(executions: [testExecution(authorityVerified: false)]),
        ],
        envelopes: _conversation(),
      ),
    );

    await _pump(tester, binding);

    expect(
      find.textContaining('authority unverified'),
      findsOneWidget,
      reason: 'an unvouched signer must be named as one',
    );
  });

  testWidgets('says nothing of the sort when a create vouched for it', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        sessions: [
          testUmbrella(executions: [testExecution()]),
        ],
        envelopes: _conversation(),
      ),
    );

    await _pump(tester, binding);

    expect(find.textContaining('authority unverified'), findsNothing);
    expect(find.textContaining('signer aaaaaaaa…'), findsOneWidget);
  });

  testWidgets('reports history truncated at the page limit', (tester) async {
    final binding = FakeObserverBinding(
      testSnapshot(envelopes: _conversation(), truncatedAt1000: true),
    );

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-truncated')),
      findsOneWidget,
    );
    expect(
      find.textContaining('History truncated at 1000 events'),
      findsOneWidget,
    );
  });

  // D10 again, per generation: the notice belongs to the session whose own
  // history this device shortened.
  testWidgets('discloses events this device dropped for this session', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        envelopes: _conversation(),
        evictedByGeneration: {testTarget().key: 500},
      ),
    );

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-evicted')),
      findsOneWidget,
    );
    expect(
      find.textContaining(
        'Older events were dropped on this device (kept the newest 2000)',
      ),
      findsOneWidget,
    );
  });

  testWidgets('another generation\'s losses are not claimed as this one\'s', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        envelopes: _conversation(),
        evictedByGeneration: {
          testTarget(sessionId: 'someone-elses-session').key: 500,
        },
      ),
    );

    await _pump(tester, binding);

    expect(find.byKey(const ValueKey('coding-session-evicted')), findsNothing);
  });

  testWidgets('names the kinds this read refused, and why', (tester) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        envelopes: _conversation(),
        counts: const CodingSessionReadCounts(
          malformed: 2,
          rejectedAuthor: 1,
          malformedByKind: {EventKind.codingSessionTranscript: 2},
          rejectedAuthorByKind: {EventKind.codingSessionMetadata: 1},
        ),
      ),
    );

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-counts-by-kind')),
      findsOneWidget,
    );
    expect(
      find.textContaining(
        'By kind: status 1 wrong signer; transcript 2 malformed',
      ),
      findsOneWidget,
    );
  });

  testWidgets('discloses a failed read behind a transcript it still shows', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        envelopes: _conversation(),
        connection: CodingSessionObserverConnection.error,
        lastError: 'subscription dropped',
      ),
    );

    await _pump(tester, binding);

    expect(find.byKey(const ValueKey('coding-session-stale')), findsOneWidget);
    expect(find.text('On it.'), findsOneWidget);
  });

  testWidgets('a session outside the read is said to be missing', (
    tester,
  ) async {
    final binding = FakeObserverBinding(testSnapshot());

    await _pump(tester, binding, sessionKey: 'umbrella-elsewhere');

    expect(
      find.byKey(const ValueKey('coding-session-missing')),
      findsOneWidget,
    );
    expect(binding.refreshCount, 0);
    await tester.tap(find.byKey(const ValueKey('coding-session-retry')));
    await tester.pump();
    expect(binding.refreshCount, 1);
  });

  testWidgets('a disconnected community is stated, not spun on', (
    tester,
  ) async {
    final binding = FakeObserverBinding(
      testSnapshot(
        sessions: const [],
        connection: CodingSessionObserverConnection.idle,
      ),
    );

    await _pump(tester, binding);

    expect(
      find.byKey(const ValueKey('coding-session-disconnected')),
      findsOneWidget,
    );
    expect(find.text('Not connected to this community'), findsOneWidget);
    expect(find.byKey(const ValueKey('coding-session-loading')), findsNothing);

    await tester.tap(find.byKey(const ValueKey('coding-session-retry')));
    await tester.pump();
    expect(binding.refreshCount, 1);
  });

  testWidgets('a founded session says where to start it, offers no composer '
      'and no Stop, and still lets its founder rename it', (tester) async {
    // A wire-shaped ref: the rename is signed against it, and the command
    // builder refuses anything that is not a UUID.
    const sessionRef = '6f1c9a52-0f2f-4f7e-8a5b-2c1d0e9f8a7b';
    final founded = testUmbrella(
      key: sessionRef,
      sessionRef: sessionRef,
      name: 'Keystone lead',
      goal: 'Ship the founded row',
      executions: const [],
      founder: const CodingSessionFounder(
        pubkey: testSignerPubkey,
        resolution: CodingSessionFounderResolution.genesis,
        genesisRef:
            'aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd',
      ),
      status: const CodingSessionFoldedStatus(
        kind: CodingSessionFoldedStatusKind.founded,
      ),
    );
    final binding = FakeObserverBinding(
      testSnapshot(sessions: [founded]),
      signerPubkey: testSignerPubkey,
    );
    await _pump(tester, binding, sessionKey: sessionRef);

    // The app bar and the header both carry the name.
    expect(find.text('Keystone lead'), findsNWidgets(2));
    expect(find.text('Not started'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('coding-session-founded')),
      findsOneWidget,
    );
    expect(find.text(codingSessionFoundedHeaderLabel), findsOneWidget);
    expect(find.text('Ship the founded row'), findsOneWidget);
    // Nothing was asked of a provider, so nothing is said about reaching one.
    expect(
      find.byKey(const ValueKey('coding-session-reachability')),
      findsNothing,
    );
    expect(find.textContaining('unknown'), findsNothing);
    expect(find.textContaining('Idle'), findsNothing);
    // No composer of either kind: not the editor, and not the line that
    // claims every generation is stopped.
    expect(find.byKey(const ValueKey('coding-session-composer')), findsNothing);
    expect(
      find.byKey(const ValueKey('coding-session-composer-unavailable')),
      findsNothing,
    );
    expect(find.text(codingSessionNoLiveExecutionLabel), findsNothing);
    expect(
      find.byKey(const ValueKey('coding-session-steer-disclosure')),
      findsNothing,
    );

    // The founder's menu: Rename, Set goal and Close, and no Stop.
    await tester.tap(find.byKey(const ValueKey('coding-session-actions')));
    await tester.pumpAndSettle();
    expect(find.text('Rename'), findsOneWidget);
    expect(find.text('Set goal'), findsOneWidget);
    expect(find.text('Close'), findsOneWidget);
    expect(find.text('Stop execution'), findsNothing);
    expect(
      find.byKey(const ValueKey('coding-session-action-stop')),
      findsNothing,
    );
    await tester.tap(find.text('Rename'));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const ValueKey('coding-session-sheet-field')),
      'Renamed before it ran',
    );
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('coding-session-sheet-save')));
    await tester.pumpAndSettle();
    expect(binding.relay.published.last.kind, EventKind.codingSessionName);
    expect(binding.relay.published.last.content, 'Renamed before it ran');
  });

  testWidgets('a founded session shows a non-founder no composer and no '
      'menu, and no steer disclosure either', (tester) async {
    final founded = testUmbrella(
      executions: const [],
      status: const CodingSessionFoldedStatus(
        kind: CodingSessionFoldedStatusKind.founded,
      ),
    );
    final binding = FakeObserverBinding(
      testSnapshot(sessions: [founded]),
      signerPubkey: testOperatorPubkey,
    );
    await _pump(tester, binding);
    expect(
      find.byKey(const ValueKey('coding-session-founded')),
      findsOneWidget,
    );
    expect(find.byKey(const ValueKey('coding-session-actions')), findsNothing);
    expect(find.byKey(const ValueKey('coding-session-composer')), findsNothing);
    expect(
      find.byKey(const ValueKey('coding-session-composer-unavailable')),
      findsNothing,
    );
    expect(
      find.byKey(const ValueKey('coding-session-steer-disclosure')),
      findsNothing,
    );
  });

  testWidgets('resolves a session by its execution key', (tester) async {
    final binding = FakeObserverBinding(testSnapshot());
    final executionKey = testTarget().executionKey;

    await _pump(tester, binding, sessionKey: executionKey);

    expect(find.byKey(const ValueKey('coding-session-header')), findsOneWidget);
  });

  testWidgets('labels each execution block in a multi-execution session', (
    tester,
  ) async {
    final second = testTarget(sessionId: 'session-2');
    final binding = FakeObserverBinding(
      testSnapshot(
        sessions: [
          testUmbrella(
            executions: [
              testExecution(),
              testExecution(target: second, agentRef: 'agent-7'),
            ],
          ),
        ],
        envelopes: [
          testEnvelope(
            eventSeq: 1,
            item: const {'kind': 'assistant_text', 'text': 'from one'},
          ),
          testEnvelope(
            eventSeq: 1,
            target: second,
            item: const {'kind': 'assistant_text', 'text': 'from two'},
          ),
        ],
      ),
    );

    await _pump(tester, binding);

    expect(
      find.textContaining('claude-code · sonnet · aaaaaaaa…'),
      findsOneWidget,
    );
    expect(find.textContaining('agent-7 · aaaaaaaa…'), findsOneWidget);
    expect(find.text('from one'), findsOneWidget);
    expect(find.text('from two'), findsOneWidget);
  });
}
