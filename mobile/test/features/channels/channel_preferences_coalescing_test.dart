import 'package:buzz/features/channels/channel_mutes/channel_mutes_manager.dart';
import 'package:buzz/features/channels/channel_sections/channel_sections_manager.dart';
import 'package:buzz/features/channels/channel_sort/channel_sort_manager.dart';
import 'package:buzz/features/channels/channel_stars/channel_stars_manager.dart';
import 'package:buzz/shared/relay/relay.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nostr/nostr.dart' as nostr;
import 'package:shared_preferences/shared_preferences.dart';

import '../../helpers/recording_relay_session.dart';

/// The four channel-preference managers (mutes, stars, sections, sort) each
/// read one encrypted blob at start. Their providers all schedule
/// `initialize()` in the same event-loop turn, so through the session's
/// 50 ms coalescer the four reads must leave as one `POST /query` — one
/// call against the bridge budget instead of four REQs against the socket's.
void main() {
  test('the four preference managers start with one coalesced query', () async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await SharedPreferences.getInstance();
    final keys = nostr.Keys.generate();
    final relay = RecordingRelaySessionNotifier()..completeHistory(const []);

    final mutes = ChannelMutesManager(
      pubkey: keys.public,
      prefs: prefs,
      crypto: ChannelMutesCrypto(keys.nsec, keys.public),
      relaySession: relay,
      signedEventRelay: null,
      remoteEnabled: true,
      onChanged: () {},
    );
    final stars = ChannelStarsManager(
      pubkey: keys.public,
      prefs: prefs,
      crypto: ChannelStarsCrypto(keys.nsec, keys.public),
      relaySession: relay,
      signedEventRelay: null,
      remoteEnabled: true,
      onChanged: () {},
    );
    final sections = ChannelSectionsManager(
      pubkey: keys.public,
      prefs: prefs,
      crypto: ChannelSectionsCrypto(keys.nsec, keys.public),
      relaySession: relay,
      signedEventRelay: null,
      remoteEnabled: true,
      onChanged: () {},
    );
    final sort = ChannelSortManager(
      pubkey: keys.public,
      relayUrl: 'wss://relay.example',
      prefs: prefs,
      crypto: ChannelSortCrypto(keys.nsec, keys.public),
      relaySession: relay,
      signedEventRelay: null,
      remoteEnabled: true,
      onChanged: () {},
    );
    addTearDown(() {
      mutes.dispose(flushPending: false);
      stars.dispose(flushPending: false);
      sections.dispose(flushPending: false);
      sort.dispose();
    });

    // The providers start every manager from a `Future.microtask` of the
    // same build pass; four un-awaited calls in one turn model that.
    await Future.wait([
      mutes.initialize(),
      stars.initialize(),
      sections.initialize(),
      sort.initialize(),
    ]);

    expect(relay.operations.where((op) => op == 'query1'), hasLength(1));
    expect(relay.operations.where((op) => op == 'fetch'), isEmpty);
    final group = relay.coalescedQueryGroups.single;
    expect(
      group.map((filter) => filter.tags['#d']!.single),
      unorderedEquals([
        'channel-mutes',
        'channel-stars',
        'channel-sections',
        'channel-sort',
      ]),
    );
    for (final filter in group) {
      expect(filter.kinds, [EventKind.readState]);
      expect(filter.authors, [keys.public]);
      expect(filter.limit, 1);
    }
    // The live subscriptions stay, one per blob.
    expect(relay.operations.where((op) => op == 'subscribe'), hasLength(4));
  });
}
