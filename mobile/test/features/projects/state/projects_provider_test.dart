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
        queryResults: [
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

      // Heads and tombstones leave in one coalesced query; the metadata of
      // the channels they name is a second, dependent one. The trailing
      // subscribe is the tombstone watch, which can only be armed once the
      // heads are known — it names their addresses.
      expect(relay.operations, ['subscribe', 'query1', 'query1', 'subscribe']);
      expect(relay.liveFilters.first.kinds, [30621]);
      // Scoped by `#a` to the project just read, never an unscoped kind:5:
      // that would carry every message deletion in the community, and
      // tombstones for private projects this reader cannot see.
      final tombstoneWatch = relay.liveFilters.last;
      expect(tombstoneWatch.kinds, [5]);
      expect(tombstoneWatch.tags['#a'], ['30621:$owner:beekeeper']);
      final [first, second] = relay.coalescedQueryGroups;
      expect(first.map((filter) => filter.kinds), [
        [30621],
        [5],
      ]);
      expect(second.single.kinds, [39000]);
      expect(second.single.tags['#d'], ['c-transport']);

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
      queryResults: [
        [_head('beekeeper')],
        const <NostrEvent>[],
      ],
    );
    final container = ProviderContainer(
      overrides: [relaySessionProvider.overrideWith(() => relay)],
    );
    addTearDown(container.dispose);
    container.read(projectsProvider);
    await _settle();
    // The trailing subscribe is the tombstone watch over the one head read.
    expect(relay.operations, ['subscribe', 'query1', 'subscribe']);
    final read = container.read(projectsProvider);
    expect(read.connection, ProjectsConnection.open);
    expect(read.referencedChannels, isEmpty);
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
