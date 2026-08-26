import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('cs-target key', () {
    test('matches a desktop-produced key byte for byte', () {
      // Copied from the desktop test that pins the signed tag value:
      // desktop/src/features/coding-sessions/lib/codingSessionCommand.test.mjs
      // line 55 — ["cs-target", "coding-session/v1|10:provider-a10:instance-19:session-11:2"].
      const target = CodingSessionTarget(
        driver: 'provider-a',
        instanceId: 'instance-1',
        sessionId: 'session-1',
        generation: 2,
      );
      expect(
        target.key,
        'coding-session/v1|10:provider-a10:instance-19:session-11:2',
      );
    });

    test('matches the second desktop-produced key', () {
      // desktop/src/features/coding-sessions/lib/codingSessionHandoff.test.mjs
      // line 18 — targetKey: "coding-session/v1|5:acp-x4:i-129:session-11:7".
      const target = CodingSessionTarget(
        driver: 'acp-x',
        instanceId: 'i-12',
        sessionId: 'session-1',
        generation: 7,
      );
      expect(target.key, 'coding-session/v1|5:acp-x4:i-129:session-11:7');
    });

    test('prefixes by UTF-8 bytes, not UTF-16 code units', () {
      const target = CodingSessionTarget(
        driver: 'é',
        instanceId: 'a',
        sessionId: 'b',
        generation: 1,
      );
      // 'é' is two UTF-8 bytes; a code-unit length would say 1.
      expect(target.key, 'coding-session/v1|2:é1:a1:b1:1');
    });

    test('a delimiter inside a field cannot forge another tuple', () {
      const sneaky = CodingSessionTarget(
        driver: 'a',
        instanceId: 'b|c',
        sessionId: 'd',
        generation: 1,
      );
      const plain = CodingSessionTarget(
        driver: 'a',
        instanceId: 'b',
        sessionId: 'c|d',
        generation: 1,
      );
      expect(sneaky.key, isNot(plain.key));
    });

    test('round-trips through fromKey', () {
      const target = CodingSessionTarget(
        driver: 'claude-agent-acp',
        instanceId: 'i-1',
        sessionId: 'sess-é',
        generation: 12,
      );
      expect(CodingSessionTarget.fromKey(target.key), target);
    });

    test('generation is part of the identity', () {
      final first = CodingSessionTarget.fromKey(
        const CodingSessionTarget(
          driver: 'a',
          instanceId: 'b',
          sessionId: 'c',
          generation: 1,
        ).key,
      );
      final second = CodingSessionTarget.fromKey(
        const CodingSessionTarget(
          driver: 'a',
          instanceId: 'b',
          sessionId: 'c',
          generation: 2,
        ).key,
      );
      expect(first, isNot(second));
      expect(first!.executionKey, second!.executionKey);
    });

    test('rejects a non-key string', () {
      expect(CodingSessionTarget.fromKey('coding-session/v1|nope'), isNull);
      expect(CodingSessionTarget.fromKey('other/v1|1:a1:b1:c1:1'), isNull);
    });
  });

  group('CodingSessionTarget.decode', () {
    test('accepts exactly the four wire keys', () {
      expect(
        CodingSessionTarget.decode({
          'driver': 'a',
          'instanceId': 'b',
          'sessionId': 'c',
          'generation': 3,
        }),
        const CodingSessionTarget(
          driver: 'a',
          instanceId: 'b',
          sessionId: 'c',
          generation: 3,
        ),
      );
    });

    test(
      'rejects an extra key, a blank field, or a non-positive generation',
      () {
        expect(
          CodingSessionTarget.decode({
            'driver': 'a',
            'instanceId': 'b',
            'sessionId': 'c',
            'generation': 1,
            'extra': 1,
          }),
          isNull,
        );
        expect(
          CodingSessionTarget.decode({
            'driver': '  ',
            'instanceId': 'b',
            'sessionId': 'c',
            'generation': 1,
          }),
          isNull,
        );
        expect(
          CodingSessionTarget.decode({
            'driver': 'a',
            'instanceId': 'b',
            'sessionId': 'c',
            'generation': 0,
          }),
          isNull,
        );
      },
    );
  });

  group('semantic keys', () {
    test('metadata key matches the desktop domain and field order', () {
      const target = CodingSessionTarget(
        driver: 'provider-a',
        instanceId: 'instance-1',
        sessionId: 'session-1',
        generation: 2,
      );
      expect(
        target.metadataSemanticKey,
        'coding-session-metadata/v1|10:provider-a10:instance-19:session-11:2',
      );
    });

    test('transcript key carries the sequence', () {
      const target = CodingSessionTarget(
        driver: 'provider-a',
        instanceId: 'instance-1',
        sessionId: 'session-1',
        generation: 2,
      );
      expect(
        target.transcriptSemanticKey(10),
        'coding-session-transcript/v1'
        '|10:provider-a10:instance-19:session-11:22:10',
      );
    });

    test('a turn receipt key names its stage; a lifecycle one does not', () {
      expect(
        codingSessionReceiptSemanticKey(
          'cmd-1',
          CodingSessionReceiptStatus.created,
        ),
        'coding-session-lifecycle-receipt/v1|5:cmd-1',
      );
      expect(
        codingSessionReceiptSemanticKey(
          'cmd-1',
          CodingSessionReceiptStatus.turnStarted,
        ),
        'coding-session-lifecycle-receipt/v1|5:cmd-112:turn_started',
      );
    });
  });
}
