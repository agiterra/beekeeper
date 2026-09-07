import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

const _channel = '0b3a7d9c-3d0f-4c2b-9d0e-9f6a1c2b3d4e';
const _sessionRef = '6f1c9a52-0f2f-4f7e-8a5b-2c1d0e9f8a7b';
const _hex = 'aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd';

const _target = CodingSessionTarget(
  driver: 'provider-a',
  instanceId: 'instance-1',
  sessionId: 'session-1',
  generation: 1,
);

NostrEvent _signed(CodingSessionCommandEvent event) => NostrEvent(
  id: _hex,
  pubkey: _hex,
  createdAt: 1700000000,
  kind: event.kind,
  tags: event.tags,
  content: event.content,
  sig: '$_hex$_hex',
);

void main() {
  group('44220 thread.turn.start', () {
    test('a boundary turn is byte-identical to the buzz-core fixture', () {
      // crates/buzz-core/src/coding_session_command.rs, test
      // `an_absent_deliver_class_means_boundary`.
      final event = buildCodingSessionTurnStartEvent(
        channelId: _channel,
        commandId: 'cmd-3',
        target: _target,
        text: 'go',
      );
      expect(event.kind, 44220);
      expect(
        event.content,
        '{"schema":"buzz-coding-session-command/v1","commandId":"cmd-3",'
        '"target":{"driver":"provider-a","instanceId":"instance-1",'
        '"sessionId":"session-1","generation":1},'
        '"action":{"type":"thread.turn.start","text":"go"}}',
      );
      expect(event.tags, [
        ['h', _channel],
        ['cs-v', 'csc1-1'],
        ['cs-target', _target.key],
      ]);
    });

    test('steer and interrupt are written; boundary is omitted', () {
      // coding_session_command.rs, the `deliver` round-trip test.
      for (final (delivery, wire) in [
        (CodingSessionTurnDelivery.steer, 'steer'),
        (CodingSessionTurnDelivery.interrupt, 'interrupt'),
      ]) {
        final event = buildCodingSessionTurnStartEvent(
          channelId: _channel,
          commandId: 'cmd-4',
          target: _target,
          text: 'go',
          deliver: delivery,
        );
        expect(
          event.content,
          endsWith(
            '"action":{"type":"thread.turn.start","text":"go",'
            '"deliver":"$wire"}}',
          ),
        );
      }
      final boundary = buildCodingSessionTurnStartEvent(
        channelId: _channel,
        commandId: 'cmd-4',
        target: _target,
        text: 'go',
        deliver: CodingSessionTurnDelivery.boundary,
      );
      expect(boundary.content, isNot(contains('deliver')));
    });

    test('the cs-target tag is the parseable signed key', () {
      final event = buildCodingSessionTurnStartEvent(
        channelId: _channel,
        commandId: 'cmd-3',
        target: _target,
        text: 'go',
      );
      expect(CodingSessionTarget.fromKey(event.tags[2][1]), _target);
    });

    test('bounds are refused before signing, naming the field', () {
      expect(
        () => buildCodingSessionTurnStartEvent(
          channelId: _channel,
          commandId: 'cmd-3',
          target: _target,
          text: '   ',
        ),
        throwsA(
          isA<CodingSessionCommandError>().having(
            (e) => e.field,
            'field',
            'action.text',
          ),
        ),
      );
      expect(
        () => buildCodingSessionTurnStartEvent(
          channelId: _channel,
          commandId: 'cmd-3',
          target: _target,
          text: 'x' * (12 * 1024 + 1),
        ),
        throwsA(isA<CodingSessionCommandError>()),
      );
      expect(
        () => buildCodingSessionTurnStartEvent(
          channelId: _channel,
          commandId: 'cmd-3',
          target: const CodingSessionTarget(
            driver: 'provider-a',
            instanceId: 'instance-1',
            sessionId: 'session-1',
            generation: 0,
          ),
          text: 'go',
        ),
        throwsA(
          isA<CodingSessionCommandError>().having(
            (e) => e.field,
            'field',
            'target.generation',
          ),
        ),
      );
      expect(
        () => buildCodingSessionTurnStartEvent(
          channelId: '',
          commandId: 'cmd-3',
          target: _target,
          text: 'go',
        ),
        throwsA(
          isA<CodingSessionCommandError>().having(
            (e) => e.field,
            'field',
            'channelId',
          ),
        ),
      );
    });

    test('a UTF-8 text at exactly the bound is accepted', () {
      // Bytes, not characters: `é` is two bytes.
      final text = 'é' * (12 * 1024 ~/ 2);
      final event = buildCodingSessionTurnStartEvent(
        channelId: _channel,
        commandId: 'cmd-3',
        target: _target,
        text: text,
      );
      expect(event.content, contains(text));
      expect(
        () => buildCodingSessionTurnStartEvent(
          channelId: _channel,
          commandId: 'cmd-3',
          target: _target,
          text: '${text}a',
        ),
        throwsA(isA<CodingSessionCommandError>()),
      );
    });
  });

  group('44220 thread.turn.interrupt', () {
    test('matches the buzz-core fixture', () {
      // coding_session_command.rs: `{"type":"thread.turn.interrupt"}` under
      // commandId cmd-2.
      final event = buildCodingSessionInterruptEvent(
        channelId: _channel,
        commandId: 'cmd-2',
        target: _target,
      );
      expect(
        event.content,
        '{"schema":"buzz-coding-session-command/v1","commandId":"cmd-2",'
        '"target":{"driver":"provider-a","instanceId":"instance-1",'
        '"sessionId":"session-1","generation":1},'
        '"action":{"type":"thread.turn.interrupt"}}',
      );
      expect(event.tags[1], ['cs-v', 'csc1-1']);
    });
  });

  group('44221 session.stop', () {
    test('matches the desktop lifecycle builder byte for byte', () {
      // desktop/src/features/coding-sessions/lib/codingSessionLifecycleCommand.ts
      // `buildCodingSessionTargetLifecycleEvent(input, "session.stop")`.
      final event = buildCodingSessionStopEvent(
        channelId: _channel,
        commandId: 'csl-1',
        target: _target,
        providerAuthorityPubkey: _hex,
      );
      expect(event.kind, 44221);
      expect(
        event.content,
        '{"schema":"buzz-coding-session-lifecycle-command/v1",'
        '"commandId":"csl-1","action":{"type":"session.stop",'
        '"session":{"driver":"provider-a","instanceId":"instance-1",'
        '"sessionId":"session-1","generation":1},'
        '"providerAuthorityPubkey":"$_hex"}}',
      );
      expect(event.tags, [
        ['h', _channel],
        ['csl-v', 'csl1-1'],
        ['csl-command', 'csl-1'],
      ]);
    });

    test('refuses a provider authority that is not a pubkey', () {
      expect(
        () => buildCodingSessionStopEvent(
          channelId: _channel,
          commandId: 'csl-1',
          target: _target,
          providerAuthorityPubkey: 'ABC',
        ),
        throwsA(
          isA<CodingSessionCommandError>().having(
            (e) => e.field,
            'field',
            'action.providerAuthorityPubkey',
          ),
        ),
      );
    });
  });

  group('umbrella facts', () {
    test('a name is trimmed, single-line, and decodes with our own reader', () {
      final event = buildCodingSessionNameEvent(
        channelId: _channel,
        sessionRef: _sessionRef,
        name: '  Keystone lead  ',
      );
      expect(event.kind, 44229);
      expect(event.content, 'Keystone lead');
      expect(event.tags, [
        ['h', _channel],
        ['d', _sessionRef],
        ['csnm-v', 'csnm1-1'],
      ]);
      final decoded = decodeCodingSessionName(_signed(event));
      expect(decoded.isValid, isTrue, reason: '${decoded.reason}');
      expect(decoded.value!.content, 'Keystone lead');
      expect(decoded.value!.sessionRef, _sessionRef);
      expect(
        () => buildCodingSessionNameEvent(
          channelId: _channel,
          sessionRef: _sessionRef,
          name: 'two\nlines',
        ),
        throwsA(
          isA<CodingSessionCommandError>().having(
            (e) => e.field,
            'field',
            'name',
          ),
        ),
      );
    });

    test('a goal decodes with our own reader', () {
      final event = buildCodingSessionGoalEvent(
        channelId: _channel,
        sessionRef: _sessionRef,
        goal: 'Ship the terminal viewer.\n\nThen the composer.',
      );
      expect(event.kind, 44227);
      expect(event.tags[2], ['csgl-v', 'csgl1-1']);
      final decoded = decodeCodingSessionGoal(_signed(event));
      expect(decoded.isValid, isTrue, reason: '${decoded.reason}');
      expect(decoded.value!.content, event.content);
    });

    test('a closure matches the desktop payload and decodes', () {
      // codingSessionClosure.ts: {action, genesisRef, sessionRef, v}.
      final event = buildCodingSessionClosureEvent(
        channelId: _channel,
        sessionRef: _sessionRef,
        genesisRef: _hex,
        action: CodingSessionClosureAction.closed,
      );
      expect(event.kind, 44230);
      expect(
        event.content,
        '{"action":"closed","genesisRef":"$_hex","sessionRef":"$_sessionRef",'
        '"v":1}',
      );
      expect(event.tags, [
        ['h', _channel],
        ['d', _sessionRef],
        ['cscl-v', 'cscl1-1'],
        ['cscl-genesis', _hex],
      ]);
      final decoded = decodeCodingSessionClosure(_signed(event));
      expect(decoded.isValid, isTrue, reason: '${decoded.reason}');
      expect(decoded.value!.closed, isTrue);
      expect(decoded.value!.genesisRef, _hex);

      final reopen = buildCodingSessionClosureEvent(
        channelId: _channel,
        sessionRef: _sessionRef,
        genesisRef: _hex,
        action: CodingSessionClosureAction.open,
      );
      expect(
        decodeCodingSessionClosure(_signed(reopen)).value!.closed,
        isFalse,
      );
    });

    test('a malformed session ref or genesis id is refused', () {
      expect(
        () => buildCodingSessionGoalEvent(
          channelId: _channel,
          sessionRef: 'not-a-uuid',
          goal: 'x',
        ),
        throwsA(
          isA<CodingSessionCommandError>().having(
            (e) => e.field,
            'field',
            'sessionRef',
          ),
        ),
      );
      expect(
        () => buildCodingSessionClosureEvent(
          channelId: _channel,
          sessionRef: _sessionRef,
          genesisRef: 'abc',
          action: CodingSessionClosureAction.closed,
        ),
        throwsA(
          isA<CodingSessionCommandError>().having(
            (e) => e.field,
            'field',
            'genesisRef',
          ),
        ),
      );
    });
  });

  group('command ids', () {
    test('carry the desktop prefixes', () {
      expect(createCodingSessionCommandId(), startsWith('csc-'));
      expect(createCodingSessionLifecycleCommandId(), startsWith('csl-'));
      expect(
        createCodingSessionCommandId(),
        isNot(createCodingSessionCommandId()),
      );
    });
  });
}
