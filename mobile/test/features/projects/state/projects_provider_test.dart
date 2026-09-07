import 'package:buzz/features/projects/state/projects_provider.dart';
import 'package:buzz/shared/relay/relay.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../helpers/recording_relay_session.dart';

const owner =
    'aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd';

NostrEvent _head(String dtag, {List<String> channels = const []}) => NostrEvent(
  id: 'h-$dtag',
  pubkey: owner,
  createdAt: 100,
  kind: 30621,
  tags: [
    ['d', dtag],
    ['name', dtag.toUpperCase()],
    for (final id in channels) ['channel', id],
  ],
  content: '',
  sig: '',
);

NostrEvent _metadata(String id, {String type = 'transport'}) => NostrEvent(
  id: 'm-$id',
  pubkey: owner,
  createdAt: 100,
  kind: 39000,
  tags: [
    ['d', id],
    ['name', 'meta $id'],
    ['t', type],
    ['project', '30621:$owner:beekeeper'],
  ],
  content: '',
  sig: '',
);

Future<void> _settle() async {
  for (var i = 0; i < 6; i++) {
    await Future<void>.delayed(Duration.zero);
  }
}

void main() {
  test(
    'reads heads, tombstones, then the metadata of referenced channels',
    () async {
      final relay = RecordingRelaySessionNotifier(
        historyResults: [
          [
            _head('beekeeper', channels: ['c-transport']),
            _head('gone'),
          ],
          [
            NostrEvent(
              id: 't',
              pubkey: owner,
              createdAt: 200,
              kind: 5,
              tags: [
                ['a', '30621:$owner:gone'],
              ],
              content: '',
              sig: '',
            ),
          ],
          [_metadata('c-transport')],
        ],
      );
      final container = ProviderContainer(
        overrides: [relaySessionProvider.overrideWith(() => relay)],
      );
      addTearDown(container.dispose);

      expect(
        container.read(projectsProvider).connection,
        ProjectsConnection.connecting,
      );
      await _settle();

      expect(relay.operations, ['subscribe', 'fetch', 'fetch', 'fetch']);
      expect(relay.liveFilters.single.kinds, [30621]);
      expect(relay.historyFilters[0].kinds, [30621]);
      expect(relay.historyFilters[1].kinds, [5]);
      expect(relay.historyFilters[2].kinds, [39000]);
      expect(relay.historyFilters[2].tags['#d'], ['c-transport']);

      final read = container.read(projectsProvider);
      expect(read.connection, ProjectsConnection.open);
      expect(read.projects.map((p) => p.name), ['BEEKEEPER']);
      expect(read.byAddress('30621:$owner:beekeeper')!.channelIds, [
        'c-transport',
      ]);
      final referenced = read.referencedChannels['c-transport']!;
      expect(referenced.channelType, 'transport');
      expect(referenced.projectRef, '30621:$owner:beekeeper');
    },
  );

  test('no referenced channels means no metadata read', () async {
    final relay = RecordingRelaySessionNotifier(
      historyResults: [
        [_head('beekeeper')],
        const [],
      ],
    );
    final container = ProviderContainer(
      overrides: [relaySessionProvider.overrideWith(() => relay)],
    );
    addTearDown(container.dispose);
    container.read(projectsProvider);
    await _settle();
    expect(relay.operations, ['subscribe', 'fetch', 'fetch']);
    expect(container.read(projectsProvider).referencedChannels, isEmpty);
  });

  test('a failed read is reported as such, not as no projects', () async {
    final relay = RecordingRelaySessionNotifier();
    final container = ProviderContainer(
      overrides: [relaySessionProvider.overrideWith(() => relay)],
    );
    addTearDown(container.dispose);
    container.read(projectsProvider);
    await _settle();
    relay.failHistory(Exception('boom'));
    await _settle();
    final read = container.read(projectsProvider);
    expect(read.connection, ProjectsConnection.error);
    expect(read.hasRead, isFalse);
    expect(read.projects, isEmpty);
    expect(read.lastError, contains('boom'));
  });
}
