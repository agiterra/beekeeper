import 'package:buzz/features/coding_sessions/state/coding_sessions_state.dart';
import 'package:buzz/shared/relay/relay_session.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../helpers/recording_relay_session.dart';
import '../domain/coding_session_provider_catalog_test.dart' as fixtures;

const _channel = '0b3a7d9c-3d0f-4c2b-9d0e-9f6a1c2b3d4e';

void main() {
  test('reads the channel\'s 44222s once, signature-checked, newest per '
      'signer, and counts what it refused', () async {
    final older = fixtures.rustCatalogContent.replaceFirst(
      '"revision":3',
      '"revision":2',
    );
    final relay = RecordingRelaySessionNotifier(
      historyResults: [
        [
          fixtures.signedCatalog(),
          fixtures.signedCatalog(content: older, revision: 2, createdAt: 10),
          // Not the producer's bytes: refused and counted.
          fixtures.signedCatalog(
            content: older.replaceFirst('"revision":2', '"revision": 2'),
            revision: 2,
          ),
        ],
      ],
    );
    final container = ProviderContainer(
      overrides: [relaySessionProvider.overrideWith(() => relay)],
    );
    addTearDown(container.dispose);

    final read = await container.read(
      codingSessionProviderCatalogsProvider(_channel).future,
    );

    expect(relay.operations, ['fetch']);
    final filter = relay.historyFilters.single;
    expect(filter.kinds, [44222]);
    expect(filter.tags, {
      '#h': [_channel],
    });
    expect(filter.limit, 50);
    expect(read.channelId, _channel);
    expect(read.catalogs.single.revision, 3);
    expect(read.rejected, 1);
    expect(read.isEmpty, isFalse);
  });

  test('a catalog tagged for another channel does not count here', () async {
    final relay = RecordingRelaySessionNotifier(
      historyResults: [
        [fixtures.signedCatalog(channel: 'other-channel')],
      ],
    );
    final container = ProviderContainer(
      overrides: [relaySessionProvider.overrideWith(() => relay)],
    );
    addTearDown(container.dispose);
    final read = await container.read(
      codingSessionProviderCatalogsProvider(_channel).future,
    );
    expect(read.catalogs, isEmpty);
    expect(read.rejected, 1);
  });
}
