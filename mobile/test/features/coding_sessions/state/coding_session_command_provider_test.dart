import 'dart:convert';

import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/features/coding_sessions/state/coding_sessions_state.dart';
import 'package:beekeeper/shared/relay/nostr_models.dart';
import 'package:beekeeper/shared/relay/signed_event_relay.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:nostr/nostr.dart' as nostr;

import '../../../helpers/recording_relay_session.dart';
import '../ui/fake_observer.dart';

const _channel = '0b3a7d9c-3d0f-4c2b-9d0e-9f6a1c2b3d4e';

class _Harness {
  final RecordingRelaySessionNotifier relay;
  final ProviderContainer container;
  final CodingSessionCommands commands;
  bool deliveryValid = true;

  _Harness._(this.relay, this.container, this.commands);

  factory _Harness({List<Object> publishResults = const []}) {
    final relay = RecordingRelaySessionNotifier(publishResults: publishResults);
    final container = ProviderContainer();
    addTearDown(container.dispose);
    late final _Harness harness;
    final commands = CodingSessionCommands(
      channelId: _channel,
      relay: SignedEventRelay(session: relay, nsec: nostr.Keys.generate().nsec),
      isDeliveryValid: () => harness.deliveryValid,
      pending: container.read(pendingTurnsProvider.notifier),
      pendingCreates: container.read(pendingCreatesProvider.notifier),
      now: () => DateTime.fromMillisecondsSinceEpoch(5000),
    );
    harness = _Harness._(relay, container, commands);
    return harness;
  }

  CodingSessionPendingTurns get pending => container.read(pendingTurnsProvider);
  CodingSessionPendingCreates get pendingCreates =>
      container.read(pendingCreatesProvider);
}

/// A relay OK echo for a publish the recording relay should accept.
final _accepted = NostrEvent(
  id: 'ab' * 32,
  pubkey: 'cd' * 32,
  createdAt: 1,
  kind: 44226,
  tags: const [],
  content: '',
  sig: '',
);

Map<String, dynamic> _content(RecordingRelaySessionNotifier relay) =>
    jsonDecode(relay.published.single.content) as Map<String, dynamic>;

void main() {
  test('sendTurn records the row before the relay answers, then marks it '
      'published', () async {
    final harness = _Harness();
    final execution = testExecution();

    final sent = await harness.commands.sendTurn(
      execution: execution,
      text: 'go',
      draft: 'go ',
    );

    expect(harness.relay.operations, ['publish']);
    final event = harness.relay.published.single;
    expect(event.kind, 44220);
    expect(event.tags[0], ['h', _channel]);
    expect(event.tags[2], ['cs-target', execution.targetKey]);
    expect(event.sig, isNotEmpty);
    final content = _content(harness.relay);
    expect(content['commandId'], sent.commandId);
    expect(content['action'], {'type': 'thread.turn.start', 'text': 'go'});

    final row = harness.pending.byKey[sent.pendingKey]!;
    expect(row.published, isTrue);
    expect(row.draft, 'go ');
    expect(row.generation, execution.target.generation);
    expect(row.executionKey, execution.executionKey);
    expect(row.recordedAt, 5000);
  });

  test(
    'a refused publish forgets the row and surfaces the relay\'s words',
    () async {
      final harness = _Harness(
        publishResults: [
          Exception(
            'restricted: only a session founder or a granted operator may steer',
          ),
        ],
      );

      await expectLater(
        harness.commands.sendTurn(
          execution: testExecution(),
          text: 'go',
          draft: 'go',
        ),
        throwsA(
          isA<CodingSessionPublishException>().having(
            (e) => e.message,
            'message',
            'restricted: only a session founder or a granted operator may steer',
          ),
        ),
      );
      expect(harness.pending.byKey, isEmpty);
    },
  );

  test('a community switch mid-send publishes nothing', () async {
    final harness = _Harness()..deliveryValid = false;
    await expectLater(
      harness.commands.interrupt(testExecution()),
      throwsA(isA<CodingSessionPublishException>()),
    );
    expect(harness.relay.published, isEmpty);
  });

  test('interrupt and stop address the exact generation', () async {
    final harness = _Harness();
    final execution = testExecution(target: testTarget(generation: 3));

    await harness.commands.interrupt(execution);
    expect(_content(harness.relay)['action'], {
      'type': 'thread.turn.interrupt',
    });
    expect((_content(harness.relay)['target'] as Map)['generation'], 3);
    harness.relay.published.clear();

    await harness.commands.stop(execution);
    final stop = harness.relay.published.single;
    expect(stop.kind, 44221);
    final action = _content(harness.relay)['action'] as Map;
    expect(action['type'], 'session.stop');
    expect(action['providerAuthorityPubkey'], execution.signerPubkey);
    expect((action['session'] as Map)['generation'], 3);
    expect(stop.tags[1], ['csl-v', 'csl1-1']);
    expect(stop.tags[2][1], _content(harness.relay)['commandId']);
  });

  test(
    'rename, goal and closure publish under the umbrella\'s sessionRef',
    () async {
      final harness = _Harness();
      final session = testUmbrella(
        sessionRef: '6f1c9a52-0f2f-4f7e-8a5b-2c1d0e9f8a7b',
        founder: const CodingSessionFounder(
          pubkey: testSignerPubkey,
          resolution: CodingSessionFounderResolution.genesis,
          genesisRef:
              'aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd',
        ),
      );

      await harness.commands.rename(session, ' Keystone ');
      await harness.commands.setGoal(session, 'Ship it');
      await harness.commands.close(session);
      await harness.commands.reopen(session);

      final kinds = [for (final event in harness.relay.published) event.kind];
      expect(kinds, [44229, 44227, 44230, 44230]);
      expect(harness.relay.published[0].content, 'Keystone');
      expect(harness.relay.published[0].tags[1], [
        'd',
        '6f1c9a52-0f2f-4f7e-8a5b-2c1d0e9f8a7b',
      ]);
      expect(
        jsonDecode(harness.relay.published[2].content)['action'],
        'closed',
      );
      expect(jsonDecode(harness.relay.published[3].content)['action'], 'open');
    },
  );

  test(
    'a closure without a readable genesis is refused before signing',
    () async {
      final harness = _Harness();
      final session = testUmbrella(
        founder: const CodingSessionFounder(
          pubkey: testSignerPubkey,
          resolution: CodingSessionFounderResolution.legacy,
        ),
      );
      await expectLater(
        harness.commands.close(session),
        throwsA(
          isA<CodingSessionPublishException>().having(
            (e) => e.message,
            'message',
            contains('no readable genesis'),
          ),
        ),
      );
      expect(harness.relay.published, isEmpty);
    },
  );

  test('a builder refusal names the field and publishes nothing', () async {
    final harness = _Harness();
    await expectLater(
      harness.commands.rename(
        testUmbrella(sessionRef: '6f1c9a52-0f2f-4f7e-8a5b-2c1d0e9f8a7b'),
        'two\nlines',
      ),
      throwsA(
        isA<CodingSessionPublishException>().having(
          (e) => e.message,
          'message',
          startsWith('name '),
        ),
      ),
    );
    expect(harness.relay.published, isEmpty);
  });

  test('the pending store keys rows per execution and forgets on demand', () {
    final container = ProviderContainer();
    addTearDown(container.dispose);
    final notifier = container.read(pendingTurnsProvider.notifier);
    final a = CodingSessionPendingTurn(
      channelId: 'c',
      executionKey: 'x',
      generation: 1,
      commandId: 'csc-a',
      text: 'a',
      draft: 'a',
      recordedAt: 2,
      published: false,
    );
    final b = CodingSessionPendingTurn(
      channelId: 'c',
      executionKey: 'y',
      generation: 1,
      commandId: 'csc-b',
      text: 'b',
      draft: 'b',
      recordedAt: 1,
      published: false,
    );
    notifier
      ..record(a)
      ..record(b);
    expect(
      container.read(pendingTurnsProvider).forChannel('c').map((t) => t.text),
      ['b', 'a'],
    );
    expect(
      container.read(pendingTurnsProvider).forExecution('c', 'x').single.text,
      'a',
    );
    notifier.markPublished(a.key);
    expect(container.read(pendingTurnsProvider).byKey[a.key]!.published, true);
    notifier.forget(a.key);
    expect(container.read(pendingTurnsProvider).byKey.keys, [b.key]);
  });

  group('createSession', () {
    const authority = testSignerPubkey;

    test('publishes genesis, name and create in the desktop\'s order, the '
        'create naming the genesis it just signed', () async {
      final harness = _Harness();

      final requested = await harness.commands.createSession(
        projectRef: '30621:$testOperatorPubkey:beekeeper',
        repoRef: null,
        providerInstanceRef: 'claude-primary',
        providerAuthorityPubkey: authority,
        model: 'sonnet',
        title: '  Fix the gate ',
        initialTurn: 'Read the ledger first.',
      );

      expect(harness.relay.operations, ['publish', 'publish', 'publish']);
      final [genesis, name, create] = harness.relay.published;
      expect(genesis.kind, 44226);
      expect(genesis.tags, [
        ['h', _channel],
        ['csg-v', 'csg1-1'],
        ['csg-session', requested.sessionRef],
      ]);
      expect(name.kind, 44229);
      expect(name.content, 'Fix the gate');
      expect(name.tags[1], ['d', requested.sessionRef]);
      expect(create.kind, 44221);
      expect(create.tags, [
        ['h', _channel],
        ['csl-v', 'csl1-1'],
        ['csl-command', requested.commandId],
      ]);
      final action =
          (jsonDecode(create.content) as Map<String, dynamic>)['action']
              as Map<String, dynamic>;
      expect(action['sessionRef'], requested.sessionRef);
      expect(action['genesisRef'], genesis.id);
      expect(action['projectRef'], '30621:$testOperatorPubkey:beekeeper');
      expect(action['providerInstanceRef'], 'claude-primary');
      expect(action['providerAuthorityPubkey'], authority);
      expect(action['model'], 'sonnet');
      expect(action['title'], 'Fix the gate');
      expect(action['initialTurn'], 'Read the ledger first.');
      // Every event is signed by this device's key.
      expect({genesis.pubkey, name.pubkey, create.pubkey}, hasLength(1));

      final row = harness.pendingCreates.byKey[requested.pendingKey]!;
      expect(row.published, isTrue);
      expect(row.genesisRef, genesis.id);
      expect(row.sessionRef, requested.sessionRef);
      expect(row.title, 'Fix the gate');
      expect(row.providerAuthorityPubkey, authority);
      expect(row.recordedAt, 5000);
    });

    test(
      'no title means no 44229, and blank text means no first turn',
      () async {
        final harness = _Harness();
        await harness.commands.createSession(
          projectRef: null,
          repoRef: null,
          providerInstanceRef: 'claude-primary',
          providerAuthorityPubkey: authority,
          model: null,
          title: '   ',
          initialTurn: '\n',
        );
        expect(harness.relay.published.map((e) => e.kind), [44226, 44221]);
        final action =
            (jsonDecode(harness.relay.published.last.content)
                    as Map<String, dynamic>)['action']
                as Map<String, dynamic>;
        expect(action['title'], isNull);
        expect(action['initialTurn'], isNull);
        expect(action['model'], isNull);
        expect(harness.pendingCreates.byKey.values.single.title, isNull);
      },
    );

    test(
      'a refused genesis forgets the row and publishes nothing else',
      () async {
        final harness = _Harness(
          publishResults: [Exception('restricted: not a member')],
        );
        await expectLater(
          harness.commands.createSession(
            projectRef: null,
            repoRef: null,
            providerInstanceRef: 'claude-primary',
            providerAuthorityPubkey: authority,
            model: null,
            title: null,
            initialTurn: null,
          ),
          throwsA(
            isA<CodingSessionPublishException>().having(
              (e) => e.message,
              'message',
              'restricted: not a member',
            ),
          ),
        );
        expect(harness.relay.published.map((e) => e.kind), [44226]);
        expect(harness.pendingCreates.byKey, isEmpty);
      },
    );

    test('a refused create after an accepted genesis says the umbrella '
        'exists and nothing will start', () async {
      // The genesis is accepted (the relay's OK echo); the create is refused.
      final failing = _Harness(
        publishResults: [_accepted, Exception('rate-limited: slow down')],
      );
      await expectLater(
        failing.commands.createSession(
          projectRef: null,
          repoRef: null,
          providerInstanceRef: 'claude-primary',
          providerAuthorityPubkey: authority,
          model: null,
          title: null,
          initialTurn: null,
        ),
        throwsA(
          isA<CodingSessionPublishException>().having(
            (e) => e.message,
            'message',
            allOf(
              startsWith('rate-limited: slow down'),
              contains('founded on the relay'),
              contains('nothing will start'),
            ),
          ),
        ),
      );
      expect(failing.relay.published.map((e) => e.kind), [44226, 44221]);
      expect(failing.pendingCreates.byKey, isEmpty);
    });

    test('a first prompt over the bound is refused before anything is '
        'signed', () async {
      final harness = _Harness();
      await expectLater(
        harness.commands.createSession(
          projectRef: null,
          repoRef: null,
          providerInstanceRef: 'claude-primary',
          providerAuthorityPubkey: authority,
          model: null,
          title: null,
          initialTurn: 'x' * (12 * 1024 + 1),
        ),
        throwsA(
          isA<CodingSessionPublishException>().having(
            (e) => e.message,
            'message',
            startsWith('action.initialTurn'),
          ),
        ),
      );
      expect(harness.relay.published, isEmpty);
      expect(harness.pendingCreates.byKey, isEmpty);
    });
  });
}
