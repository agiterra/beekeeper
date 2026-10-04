import 'dart:io';

import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/features/coding_sessions/ui/coding_session_page.dart';
import 'package:buzz/features/coding_sessions/ui/observer_contract.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../../helpers/widget_helpers.dart';
import 'fake_observer.dart';

/// SV-36 on mobile: paragraph pieces read as one answer, and a muted
/// "Writing…" line sits under it only while the wire says it is arriving —
/// its turn has no `result`, and the target holds a live, unexpired lease
/// (CONTRACT rule 7) read through the same lease fold the header uses.
void main() {
  final now = DateTime.utc(2026, 10, 4, 12);
  final nowSeconds = now.millisecondsSinceEpoch ~/ 1000;

  setUpAll(_loadFonts);

  const pieces = [
    'The observer reads every 44225 piece and joins them.\n\n',
    'Adjacent paragraphs of one turn become one answer.\n\n',
    'A subagent’s words keep their own title.',
  ];
  final joined = pieces.join();

  List<CodingSessionTranscriptEnvelope> conversation({bool settled = false}) =>
      [
        testEnvelope(
          eventSeq: 1,
          item: const {'kind': 'user_prompt', 'content': 'Explain the join'},
        ),
        for (var index = 0; index < pieces.length; index++)
          testEnvelope(
            eventSeq: index + 2,
            item: {'kind': 'assistant_text', 'text': pieces[index]},
          ),
        if (settled)
          testEnvelope(
            eventSeq: 5,
            item: const {
              'kind': 'result',
              'subtype': 'success',
              'durationMs': 4200,
              'isError': false,
            },
          ),
      ];

  CodingSessionLease lease({
    CodingSessionLeaseState state = CodingSessionLeaseState.live,
    int ageSeconds = 10,
  }) => CodingSessionLease(
    ref: testRef(eventId: 'lease-1', createdAt: nowSeconds - ageSeconds),
    target: testTarget(),
    commandId: 'command-1',
    state: state,
    leaseSequence: 1,
  );

  CodingSessionObserverSnapshot snapshot({
    required List<CodingSessionTranscriptEnvelope> envelopes,
    List<CodingSessionLease> leases = const [],
    CodingSessionStatus status = CodingSessionStatus.running,
  }) {
    final session = testUmbrella(executions: [testExecution(status: status)]);
    final blocks = projectCodingSessionTranscript(
      envelopes,
      labelsByTargetKey: {
        for (final execution in session.executions)
          execution.targetKey: execution.label,
      },
      livenessByStream: codingSessionProseLivenessFor(
        executions: session.executions,
        leases: leases,
        now: now,
      ),
    );
    return CodingSessionObserverSnapshot(
      channelId: testChannelId,
      sessions: [session],
      executions: session.executions,
      transcriptBlocksByExecution: {
        for (final block in blocks) block.target.key: [block],
      },
      connection: CodingSessionObserverConnection.open,
      signaturesVerified: true,
      // The header reads the same lease fold, so it and "Writing…" agree.
      reachabilityBySession: {
        session.key: deriveCodingSessionReachability(
          leases: leases,
          currentTarget: testTarget(),
          acceptedCommandId: 'command-1',
          authorityPubkey: testSignerPubkey,
          now: now,
        ),
      },
    );
  }

  Future<void> pump(
    WidgetTester tester,
    CodingSessionObserverSnapshot read,
  ) async {
    tester.view.physicalSize = const Size(390 * 3, 520 * 3);
    tester.view.devicePixelRatio = 3;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      RepaintBoundary(
        key: const ValueKey('sv36-capture'),
        child: WidgetHelpers.testable(
          overrides: [fakeObserverOverride(FakeObserverBinding(read))],
          child: const CodingSessionPage(
            channelId: testChannelId,
            sessionKey: 'umbrella-1',
          ),
        ),
      ),
    );
    await tester.pump();
  }

  final writing = find.text(codingSessionWritingLabel);

  testWidgets('three pieces read as one Response with Writing… under it '
      'while the turn is open and a live lease is held', (tester) async {
    await pump(tester, snapshot(envelopes: conversation(), leases: [lease()]));

    expect(find.text('Response'), findsOneWidget);
    expect(find.text(joined), findsOneWidget);
    expect(writing, findsOneWidget);
    expect(
      find.byKey(const ValueKey('coding-session-writing-event-2')),
      findsOneWidget,
      reason: 'keyed on the first piece, so the row stays mounted',
    );
    await _golden(tester, 'sv36-mobile-arriving.png');
  });

  testWidgets('the result settles it: one Response, no Writing…', (
    tester,
  ) async {
    await pump(
      tester,
      snapshot(envelopes: conversation(settled: true), leases: [lease()]),
    );

    expect(find.text('Response'), findsOneWidget);
    expect(find.text(joined), findsOneWidget);
    expect(writing, findsNothing);
    expect(find.text('Turn result'), findsOneWidget);
    await _golden(tester, 'sv36-mobile-settled.png');
  });

  testWidgets('no lease held: nothing is arriving', (tester) async {
    await pump(tester, snapshot(envelopes: conversation()));
    expect(find.text(joined), findsOneWidget);
    expect(writing, findsNothing);
  });

  testWidgets('a released lease: nothing is arriving', (tester) async {
    await pump(
      tester,
      snapshot(
        envelopes: conversation(),
        leases: [lease(state: CodingSessionLeaseState.released)],
      ),
    );
    expect(writing, findsNothing);
  });

  testWidgets('a lapsed lease (provider slept): nothing is arriving', (
    tester,
  ) async {
    await pump(
      tester,
      snapshot(
        envelopes: conversation(),
        leases: [lease(ageSeconds: codingSessionLeaseTtl.inSeconds + 30)],
      ),
    );
    expect(writing, findsNothing);
  });

  testWidgets('a session-ending status: nothing is arriving', (tester) async {
    await pump(
      tester,
      snapshot(
        envelopes: conversation(),
        leases: [lease()],
        status: CodingSessionStatus.stopped,
      ),
    );
    expect(writing, findsNothing);
  });

  testWidgets('a turn-only status (interrupted) leaves the next answer '
      'arriving', (tester) async {
    await pump(
      tester,
      snapshot(
        envelopes: conversation(),
        leases: [lease()],
        status: CodingSessionStatus.interrupted,
      ),
    );
    expect(writing, findsOneWidget);
  });

  // Reasoning streaming as the last item of an open turn: web's folded
  // details row says "Writing…", and so does mobile's folded row.
  List<CodingSessionTranscriptEnvelope> thinking({bool settled = false}) => [
    ...conversation(),
    testEnvelope(
      eventSeq: 5,
      item: const {'kind': 'reasoning', 'text': 'Weighing the join rule.\n\n'},
    ),
    testEnvelope(
      eventSeq: 6,
      item: const {'kind': 'reasoning', 'text': 'Checking the lease.'},
    ),
    if (settled)
      testEnvelope(
        eventSeq: 7,
        item: const {
          'kind': 'result',
          'subtype': 'success',
          'durationMs': 4200,
          'isError': false,
        },
      ),
  ];

  testWidgets('trailing reasoning still arriving says Writing… while folded', (
    tester,
  ) async {
    await pump(tester, snapshot(envelopes: thinking(), leases: [lease()]));

    expect(find.text('Reasoning'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('coding-session-reasoning-body-event-5')),
      findsNothing,
      reason: 'reasoning stays folded; only the fact it is arriving shows',
    );
    expect(writing, findsOneWidget);
    expect(
      find.byKey(const ValueKey('coding-session-writing-event-5')),
      findsOneWidget,
      reason: 'the reasoning row is the tail, not the response above it',
    );
    expect(
      find.byKey(const ValueKey('coding-session-writing-event-2')),
      findsNothing,
    );
    await _golden(tester, 'sv36-mobile-reasoning-arriving.png');

    await tester.tap(
      find.byKey(const ValueKey('coding-session-reasoning-toggle-event-5')),
    );
    await tester.pump();
    expect(
      find.byKey(const ValueKey('coding-session-reasoning-body-event-5')),
      findsOneWidget,
    );
    expect(writing, findsOneWidget, reason: 'opening it keeps the line');
  });

  testWidgets('trailing reasoning settled by the result: no Writing…', (
    tester,
  ) async {
    await pump(
      tester,
      snapshot(envelopes: thinking(settled: true), leases: [lease()]),
    );
    expect(find.text('Reasoning'), findsOneWidget);
    expect(writing, findsNothing);
  });

  testWidgets('trailing reasoning with no lease: nothing is arriving', (
    tester,
  ) async {
    await pump(tester, snapshot(envelopes: thinking()));
    expect(writing, findsNothing);
  });

  testWidgets('subagent prose is titled as the subagent’s', (tester) async {
    await pump(
      tester,
      snapshot(
        envelopes: [
          ...conversation(settled: false).take(2),
          testEnvelope(
            eventSeq: 3,
            item: const {
              'kind': 'assistant_text',
              'text': 'Looked around.',
              'parentToolId': 'task-1',
            },
          ),
        ],
        leases: [lease()],
      ),
    );
    expect(find.text('Response'), findsOneWidget);
    expect(find.text(codingSessionSubagentResponseTitle), findsOneWidget);
  });
}

/// Golden captures are compared on macOS only: font rasterisation differs
/// across hosts, and these exist as screenshots, not as the assertion — the
/// finders above are the assertion and run everywhere.
Future<void> _golden(WidgetTester tester, String name) async {
  if (!Platform.isMacOS) return;
  await expectLater(
    find.byKey(const ValueKey('sv36-capture')),
    matchesGoldenFile('goldens/$name'),
  );
}

/// Real glyphs in the goldens: the app's Inter and the Lucide icon font,
/// instead of the test harness's box font.
Future<void> _loadFonts() async {
  final inter = FontLoader('Inter')
    ..addFont(rootBundle.load('assets/fonts/InterVariable.ttf'));
  final lucide = FontLoader('packages/lucide_icons_flutter/Lucide')
    ..addFont(
      rootBundle.load('packages/lucide_icons_flutter/assets/lucide.ttf'),
    );
  await Future.wait([inter.load(), lucide.load()]);
}
