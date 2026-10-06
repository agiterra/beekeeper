import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/features/coding_sessions/ui/coding_session_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../../helpers/widget_helpers.dart';
import 'fake_observer.dart';

const _incomplete = <String, Object?>{
  'contentSource': 'streamed_deltas',
  'outputComplete': false,
  'outputGap': {'streamedBytes': 1193, 'aggregatedBytes': 1793},
};

List<CodingSessionTranscriptEnvelope> _pair(Map<String, Object?> extra) => [
  testEnvelope(
    eventSeq: 1,
    item: const {
      'kind': 'tool_call',
      'tool': {
        'toolName': 'exec_command',
        'toolId': 'tool-1',
        'input': {'cmd': 'cargo test'},
      },
    },
  ),
  testEnvelope(
    eventSeq: 2,
    item: {
      'kind': 'tool_result',
      'toolId': 'tool-1',
      'content': 'tail of the output',
      ...extra,
    },
  ),
];

CodingSessionToolRow _toolRow(
  List<CodingSessionTranscriptEnvelope> envelopes,
) => projectCodingSessionTranscript(envelopes).single.items.single.tool!;

Future<void> _pump(WidgetTester tester, FakeObserverBinding binding) async {
  await tester.pumpWidget(
    WidgetHelpers.testable(
      overrides: [fakeObserverOverride(binding)],
      child: const CodingSessionPage(
        channelId: testChannelId,
        sessionKey: 'umbrella-1',
      ),
    ),
  );
  await tester.pump();
}

void main() {
  group('parse', () {
    test('an incomplete result carries both byte counts', () {
      expect(
        _toolRow(_pair(_incomplete)).outputGap,
        const CodingSessionToolOutputGap(
          streamedBytes: 1193,
          aggregatedBytes: 1793,
        ),
      );
    });

    test('an unpaired incomplete result carries the gap too', () {
      final row = projectCodingSessionTranscript([
        testEnvelope(
          eventSeq: 1,
          item: {'kind': 'tool_result', 'toolId': 'orphan', ..._incomplete},
        ),
      ]).single.items.single.tool!;
      expect(row.outputGap?.streamedBytes, 1193);
    });

    test('recovered, verified and unlabelled results carry no gap', () {
      for (final extra in <Map<String, Object?>>[
        const {},
        const {'contentSource': 'native_rollout', 'outputComplete': true},
        const {'contentSource': 'streamed_deltas', 'outputComplete': true},
      ]) {
        expect(_toolRow(_pair(extra)).outputGap, isNull);
      }
    });

    test('a non-boolean outputComplete is no claim at all', () {
      for (final value in <Object?>['false', 0, null]) {
        expect(
          _toolRow(_pair({..._incomplete, 'outputComplete': value})).outputGap,
          isNull,
        );
      }
    });

    test('a malformed outputGap keeps the notice and drops the counts', () {
      for (final gap in <Object?>[
        '1193',
        null,
        const <Object?>[],
        const {'streamedBytes': -1, 'aggregatedBytes': '1793'},
        const {'streamedBytes': 1.5},
      ]) {
        final row = _toolRow(
          _pair({'outputComplete': false, 'outputGap': gap}),
        );
        expect(row.outputGap, const CodingSessionToolOutputGap());
        expect(row.outputGap!.detail, isNull);
      }
    });

    test('an aggregate below what was captured is dropped', () {
      final gap = CodingSessionToolOutputGap.fromResult(const {
        'outputComplete': false,
        'outputGap': {'streamedBytes': 900, 'aggregatedBytes': 600},
      });
      expect(gap, const CodingSessionToolOutputGap(streamedBytes: 900));
    });

    test('detail formats the counts it has', () {
      expect(
        const CodingSessionToolOutputGap(
          streamedBytes: 1193,
          aggregatedBytes: 1793,
        ).detail,
        'captured 1,193 of 1,793 bytes',
      );
      expect(
        const CodingSessionToolOutputGap(streamedBytes: 600).detail,
        'captured 600 bytes',
      );
    });
  });

  group('notice', () {
    testWidgets('an incomplete result says so on the folded row', (
      tester,
    ) async {
      await _pump(
        tester,
        FakeObserverBinding(testSnapshot(envelopes: _pair(_incomplete))),
      );

      expect(
        find.byKey(const ValueKey('coding-session-tool-output-gap-event-1')),
        findsOneWidget,
      );
      expect(
        find.text(
          'Output may be missing its beginning · captured 1,193 of 1,793 bytes',
        ),
        findsOneWidget,
      );
    });

    testWidgets('a recovered result renders as before, with no notice', (
      tester,
    ) async {
      await _pump(
        tester,
        FakeObserverBinding(
          testSnapshot(
            envelopes: _pair(const {
              'contentSource': 'native_rollout',
              'outputComplete': true,
            }),
          ),
        ),
      );

      expect(
        find.byKey(const ValueKey('coding-session-tool-output-gap-event-1')),
        findsNothing,
      );
      expect(find.textContaining('Output may be missing'), findsNothing);
    });
  });
}
