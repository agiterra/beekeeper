import 'package:buzz/features/terminals/state/terminals_index_provider.dart';
import 'package:buzz/shared/relay/relay.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../helpers/recording_relay_session.dart';

const owner =
    'aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd';
const project = '30621:$owner:beekeeper';

NostrEvent _announce(String sessionId, {String status = 'open'}) => NostrEvent(
  id: 'a-$sessionId-$status',
  pubkey: owner,
  createdAt: status == 'open' ? 100 : 200,
  kind: 30623,
  tags: [
    ['d', sessionId],
    ['a', project],
    ['title', 'shell $sessionId'],
    ['status', status],
    ['dims', '24x80'],
  ],
  content: '',
  sig: '',
);

Future<void> _settle() async {
  for (var i = 0; i < 4; i++) {
    await Future<void>.delayed(Duration.zero);
  }
}

void main() {
  test('subscribes to live announces, reads the history, and indexes by '
      'project', () async {
    final relay = RecordingRelaySessionNotifier(
      queryResults: [
        [_announce('s1'), _announce('s2'), _announce('s2', status: 'closed')],
      ],
    );
    final container = ProviderContainer(
      overrides: [relaySessionProvider.overrideWith(() => relay)],
    );
    addTearDown(container.dispose);

    final first = container.read(terminalsIndexProvider);
    expect(first.connection, TerminalsConnection.connecting);
    expect(first.hasRead, isFalse);
    await _settle();

    // The announce read goes through the coalescer, not the socket.
    expect(relay.operations, ['subscribe', 'query1']);
    expect(relay.liveFilters.single.kinds, [30623]);
    expect(relay.liveFilters.single.since, isNotNull);
    expect(relay.coalescedQueryFilters.single.kinds, [30623]);
    expect(relay.coalescedQueryFilters.single.limit, 500);

    final index = container.read(terminalsIndexProvider);
    expect(index.connection, TerminalsConnection.open);
    expect(index.hasRead, isTrue);
    expect(index.terminals.map((t) => t.sessionId), ['s1']);
    expect(index.forProject(project).single.title, 'shell s1');
    expect(index.find(owner, 's1'), isNotNull);
    expect(index.find(owner, 's2'), isNull);
  });

  test('a live head re-reads the list', () async {
    final relay = RecordingRelaySessionNotifier(
      queryResults: [
        [_announce('s1')],
        [_announce('s1'), _announce('s3')],
      ],
    );
    final container = ProviderContainer(
      overrides: [relaySessionProvider.overrideWith(() => relay)],
    );
    addTearDown(container.dispose);
    container.read(terminalsIndexProvider);
    await _settle();
    expect(container.read(terminalsIndexProvider).terminals.length, 1);

    relay.emit(_announce('s3'));
    await _settle();
    expect(relay.operations, ['subscribe', 'query1', 'query1']);
    expect(container.read(terminalsIndexProvider).terminals.length, 2);
  });

  test('a failed read is reported, and a later success clears it', () async {
    final relay = RecordingRelaySessionNotifier();
    final container = ProviderContainer(
      overrides: [relaySessionProvider.overrideWith(() => relay)],
    );
    addTearDown(container.dispose);
    container.read(terminalsIndexProvider);
    await _settle();
    relay.failHistory(Exception('relay down'));
    await _settle();

    final failed = container.read(terminalsIndexProvider);
    expect(failed.connection, TerminalsConnection.error);
    expect(failed.hasRead, isFalse);
    expect(failed.lastError, contains('relay down'));
  });
}
