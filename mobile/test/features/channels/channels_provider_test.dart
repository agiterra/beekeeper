import 'dart:async';
import 'dart:convert';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:buzz/features/channels/channel_management_provider.dart';
import 'package:buzz/features/channels/channels_provider.dart';
import 'package:buzz/shared/relay/relay.dart';

/// Tests for [ChannelsNotifier] in the pure-Nostr world.
///
/// The provider performs a two-step read:
///   1. kind:39002 memberships tagged `#p:<my-pubkey>` over one WS REQ
///   2. kind:39000 metadata, kind:39002 member lists, DM profiles and the
///      hidden-DM list for those ids, started in one tick so they travel
///      as one coalesced `POST /query`
/// then layers one live subscription per 128 channels on the `#h` tag.
///
/// Tests stub out the relay session by overriding [relaySessionProvider] with
/// a [_FakeRelaySession] that answers reads from canned events, groups
/// [query] calls per event-loop turn the way the real coalescer does, and
/// records live subscriptions so we can assert filter shapes, count frames,
/// and emit live events on demand.
void main() {
  const myPk = 'me';

  test(
    'seeds members from the channel-list snapshot during reconnect',
    () async {
      final session = _FakeRelaySession(
        memberships: [_membership(_channelA, myPk, additionalPubkey: 'alice')],
        metadata: [_meta(id: _channelA, name: 'general')],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      await container.read(channelsProvider.future);
      final memberQueryCount = session.readFilters
          .where(
            (filter) =>
                filter.kinds.contains(39002) && filter.tags['#d'] != null,
          )
          .length;

      session.setStatus(SessionStatus.reconnecting);
      final members = await container.read(
        channelMembersProvider(_channelA).future,
      );

      expect(members.map((member) => member.pubkey), [myPk, 'alice']);
      expect(
        session.readFilters
            .where(
              (filter) =>
                  filter.kinds.contains(39002) && filter.tags['#d'] != null,
            )
            .length,
        memberQueryCount,
      );
    },
  );

  test(
    'covers the joined, non-archived channels with one live filter',
    () async {
      final session = _FakeRelaySession(
        memberships: [
          _membership(_channelA, myPk),
          _membership(_channelB, myPk),
          _membership(_channelD, myPk),
        ],
        metadata: [
          _meta(id: _channelA, name: 'general'),
          _meta(id: _channelB, name: 'random'),
          // channelD metadata missing -> won't appear in channel list
        ],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      await container.read(channelsProvider.future);

      // One REQ, one filter, every joined non-archived channel in its `#h`.
      expect(session.liveFilterBundles, hasLength(1));
      final filter = session.liveFilterBundles.single.single;
      expect(filter.tags['#h']!.toSet(), {_channelA, _channelB});
      expect(filter.kinds, EventKind.channelEventKinds);
      expect(filter.limit, 0);
      expect(filter.since, isNull);
    },
  );

  test(
    '25 channels cost one live REQ: two REQ frames and three HTTP calls',
    () async {
      final ids = _channelIds(25);
      final session = _FakeRelaySession(
        memberships: [for (final id in ids) _membership(id, myPk)],
        metadata: [for (final id in ids) _meta(id: id, name: 'ch-$id')],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      final channels = await container.read(channelsProvider.future);
      await _waitUntil(() => session.queryBatches.length == 2);

      expect(channels, hasLength(25));
      expect(session.liveFilterBundles, hasLength(1));
      final filter = session.liveFilterBundles.single.single;
      expect(filter.tags['#h'], hasLength(25));
      expect(filter.tags['#h']!.toSet(), ids.toSet());
      expect(filter.kinds, EventKind.channelEventKinds);
      expect(filter.limit, 0);

      // WS: the membership page and the live bundle. HTTP: the coalesced
      // metadata reads, the latest-message batch and the unread catch-up.
      expect(session.reqFrames, 2);
      expect(session.closeFrames, 0);
      expect(session.httpCalls, 3);
      expect(session.coalescedQueryGroups, hasLength(1));
    },
  );

  test('300 channels open one live REQ per 128 channels', () async {
    final ids = _channelIds(300);
    final session = _FakeRelaySession(
      memberships: [for (final id in ids) _membership(id, myPk)],
      metadata: [for (final id in ids) _meta(id: id, name: 'ch-$id')],
    );
    final container = _buildContainer(session: session);
    addTearDown(container.dispose);

    await container.read(channelsProvider.future);

    expect(session.liveFilterBundles.map((bundle) => bundle.length).toList(), [
      1,
      1,
      1,
    ]);
    expect(
      session.liveFilterBundles
          .map((bundle) => bundle.single.tags['#h']!.length)
          .toList(),
      [128, 128, 44],
    );
    expect({
      for (final bundle in session.liveFilterBundles)
        ...bundle.single.tags['#h']!,
    }, ids.toSet());
    for (final bundle in session.liveFilterBundles) {
      expect(bundle.single.kinds, EventKind.channelEventKinds);
      expect(bundle.single.limit, 0);
    }
    expect(session.reqFrames, 1 + 3);
  });

  test('planLiveChannelFilters chunks at the relay #h cap', () {
    expect(planLiveChannelFilters(const []), isEmpty);
    expect(planLiveChannelFilters([_channelA]).single.single.tags['#h'], [
      _channelA,
    ]);
    final plan = planLiveChannelFilters(_channelIds(129));
    expect(plan.map((filters) => filters.single.tags['#h']!.length), [128, 1]);
  });

  test('retains channel-list member snapshots for immediate reuse', () async {
    final joinedAt = DateTime.fromMillisecondsSinceEpoch(1000, isUtc: true);
    final session = _FakeRelaySession(
      memberships: [_membership(_channelA, myPk, additionalPubkey: 'alice')],
      metadata: [_meta(id: _channelA, name: 'general')],
    );
    final container = _buildContainer(session: session);
    addTearDown(container.dispose);

    await container.read(channelsProvider.future);
    final members = container
        .read(channelsProvider.notifier)
        .cachedMembersForChannel(_channelA);

    expect(members, hasLength(2));
    expect(members.map((member) => member.pubkey), [myPk, 'alice']);
    expect(members.every((member) => member.joinedAt == joinedAt), isTrue);
  });

  test(
    'refreshing an unchanged channel set issues zero new live REQs',
    () async {
      final session = _FakeRelaySession(
        memberships: [
          _membership(_channelA, myPk),
          _membership(_channelB, myPk),
        ],
        metadata: [
          _meta(id: _channelA, name: 'general'),
          _meta(id: _channelB, name: 'random'),
        ],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      await container.read(channelsProvider.future);
      final initialSubscribeCount = session.totalSubscribeCount;

      await container.read(channelsProvider.notifier).refresh();
      await _settle();

      expect(session.totalSubscribeCount, initialSubscribeCount);
      expect(session.unsubscribeCount, 0);
      expect(session.activeChannels, {_channelA, _channelB});
    },
  );

  test(
    'a membership change opens the new bundle before closing the old one',
    () async {
      final session = _FakeRelaySession(
        memberships: [
          _membership(_channelA, myPk),
          _membership(_channelB, myPk),
        ],
        metadata: [
          _meta(id: _channelA, name: 'general'),
          _meta(id: _channelB, name: 'random'),
        ],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      await container.read(channelsProvider.future);
      session.memberships = [
        _membership(_channelB, myPk),
        _membership(_channelD, myPk),
      ];
      session.metadata = [
        _meta(id: _channelB, name: 'random'),
        _meta(id: _channelD, name: 'support'),
      ];

      await container.read(channelsProvider.notifier).refresh();
      await _waitUntil(() => session.unsubscribeCount == 1);

      expect(session.liveFilterBundles, hasLength(2));
      expect(session.liveFilterBundles.last.single.tags['#h']!.toSet(), {
        _channelB,
        _channelD,
      });
      expect(session.activeChannels, {_channelB, _channelD});
      expect(session.activeSubscriptionCount, 1);
      // Make-before-break: the new REQ went out before the old CLOSE, so
      // the listener count never dropped to zero.
      expect(session.liveOperations, ['subscribe', 'subscribe', 'close']);
      expect(session.activeCountHistory, [1, 2, 1]);
      expect(session.activeCountHistory.skip(1), isNot(contains(0)));
    },
  );

  test(
    'a burst of membership changes costs one rebuild after the debounce',
    () async {
      final session = _FakeRelaySession(
        memberships: [_membership(_channelA, myPk)],
        metadata: [_meta(id: _channelA, name: 'general')],
      );
      final container = _buildContainer(
        session: session,
        liveRebuildDebounce: ChannelsNotifier.liveRebuildDebounce,
      );
      addTearDown(container.dispose);

      await container.read(channelsProvider.future);
      final notifier = container.read(channelsProvider.notifier);

      session.memberships = [
        _membership(_channelA, myPk),
        _membership(_channelB, myPk),
      ];
      session.metadata = [
        _meta(id: _channelA, name: 'general'),
        _meta(id: _channelB, name: 'random'),
      ];
      await notifier.refresh();
      session.memberships = [
        _membership(_channelA, myPk),
        _membership(_channelB, myPk),
        _membership(_channelD, myPk),
      ];
      session.metadata = [
        _meta(id: _channelA, name: 'general'),
        _meta(id: _channelB, name: 'random'),
        _meta(id: _channelD, name: 'support'),
      ];
      await notifier.refresh();

      // The list is current at once; the bundle waits out the debounce.
      expect(container.read(channelsProvider).value, hasLength(3));
      await Future<void>.delayed(const Duration(milliseconds: 100));
      expect(session.liveFilterBundles, hasLength(1));

      await Future<void>.delayed(const Duration(milliseconds: 300));
      expect(session.liveFilterBundles, hasLength(2));
      expect(session.liveFilterBundles.last.single.tags['#h']!.toSet(), {
        _channelA,
        _channelB,
        _channelD,
      });
      expect(session.unsubscribeCount, 1);
    },
  );

  test('an empty channel refresh closes the live bundle', () async {
    final session = _FakeRelaySession(
      memberships: [_membership(_channelA, myPk), _membership(_channelB, myPk)],
      metadata: [
        _meta(id: _channelA, name: 'general'),
        _meta(id: _channelB, name: 'random'),
      ],
    );
    final container = _buildContainer(session: session);
    addTearDown(container.dispose);

    await container.read(channelsProvider.future);
    session.memberships = [];
    session.metadata = [];

    await container.read(channelsProvider.notifier).refresh();
    await _waitUntil(() => session.activeSubscriptionCount == 0);

    expect(session.activeChannels, isEmpty);
    expect(session.unsubscribeCount, 1);
    expect(session.totalSubscribeCount, 1);
  });

  test(
    'overlapping refreshes open one bundle for the final channel set',
    () async {
      final session = _FakeRelaySession(
        memberships: [
          _membership(_channelA, myPk),
          _membership(_channelB, myPk),
        ],
        metadata: [
          _meta(id: _channelA, name: 'general'),
          _meta(id: _channelB, name: 'random'),
        ],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      await container.read(channelsProvider.future);
      session.pauseNextSubscribe();
      session.memberships = [
        _membership(_channelA, myPk),
        _membership(_channelB, myPk),
        _membership(_channelD, myPk),
      ];
      session.metadata = [
        _meta(id: _channelA, name: 'general'),
        _meta(id: _channelB, name: 'random'),
        _meta(id: _channelD, name: 'support'),
      ];

      final firstRefresh = container.read(channelsProvider.notifier).refresh();
      await session.nextSubscribeStarted;
      final secondRefresh = container.read(channelsProvider.notifier).refresh();
      session.resumePausedSubscribe();
      await Future.wait([firstRefresh, secondRefresh]);
      await _waitUntil(() => session.unsubscribeCount == 1);

      expect(session.activeChannels, {_channelA, _channelB, _channelD});
      expect(session.activeSubscriptionCount, 1);
      expect(session.totalSubscribeCount, 2);
    },
  );

  test(
    'community switch replaces retained live subscriptions on the new relay',
    () async {
      final session = _FakeRelaySession(
        memberships: [_membership(_channelA, myPk)],
        metadata: [_meta(id: _channelA, name: 'general')],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      await container.read(channelsProvider.future);
      expect(session.activeChannels, {_channelA});

      session.setStatus(SessionStatus.disconnected);
      session.memberships = [_membership(_channelB, myPk)];
      session.metadata = [_meta(id: _channelB, name: 'random')];
      container
          .read(relayConfigProvider.notifier)
          .update(baseUrl: 'https://new-community.example');
      await Future<void>.delayed(Duration.zero);
      session.setStatus(SessionStatus.connected);
      await container.read(channelsProvider.future);
      await _waitUntil(
        () =>
            session.activeChannels.length == 1 &&
            session.activeChannels.contains(_channelB),
      );

      expect(session.activeChannels, {_channelB});
      expect(session.activeSubscriptionCount, 1);
      expect(session.unsubscribeCount, 1);
    },
  );

  test('live channel events update channel lastMessageAt', () async {
    final session = _FakeRelaySession(
      memberships: [_membership(_channelA, myPk)],
      metadata: [_meta(id: _channelA, name: 'general', createdAt: 10)],
    );
    final container = _buildContainer(session: session);
    addTearDown(container.dispose);

    await container.read(channelsProvider.future);

    // Emit a live message event on channelA.
    session.emit(_message('event-1', channel: _channelA, createdAt: 20));

    final channels = container.read(channelsProvider).value!;
    expect(channels.single.lastMessageAt?.millisecondsSinceEpoch, 20 * 1000);
  });

  test(
    'events for an unknown channel refresh once, not once per event',
    () async {
      final session = _FakeRelaySession(
        memberships: [_membership(_channelA, myPk)],
        metadata: [_meta(id: _channelA, name: 'general')],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      await container.read(channelsProvider.future);
      expect(session.historyFilters, hasLength(1));

      // The relay fans out three events for a channel the list does not
      // hold (its metadata never arrives, so every refresh comes back the
      // same). Only the first may trigger a refresh.
      for (var i = 0; i < 3; i++) {
        session.emit(
          _message('unknown-$i', channel: _channelD, createdAt: 20 + i),
        );
        await _settle();
      }

      expect(session.historyFilters, hasLength(2));
      expect(session.totalSubscribeCount, 1);
    },
  );

  test(
    'loads all channel timestamps through one batched relay query',
    () async {
      final session = _FakeRelaySession(
        memberships: [
          _membership(_channelA, myPk),
          _membership(_channelB, myPk),
        ],
        metadata: [
          _meta(id: _channelA, name: 'general'),
          _meta(id: _channelB, name: 'direct', channelType: 'dm'),
        ],
        recentMessages: [
          _message('stream-message', channel: _channelA, createdAt: 30),
          _message('dm-message', channel: _channelB, createdAt: 40, kind: 9),
        ],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      final channels = await container.read(channelsProvider.future);

      await _waitUntil(() => session.queryBatches.length == 2);
      expect(session.queryBatches, hasLength(2));
      expect(session.queryBatches.first, hasLength(2));
      expect(
        session.queryBatches.first
            .map((filter) => filter.tags['#h']!.single)
            .toSet(),
        {_channelA, _channelB},
      );
      expect(session.queryBatches.last, hasLength(2));
      expect(
        session.queryBatches.last.every(
          (filter) => filter.limit == 1000 && filter.since == 0,
        ),
        isTrue,
      );
      expect(
        session.historyFilters.where((filter) {
          final kinds = filter.kinds.toSet();
          return kinds.length == EventKind.channelMessageEventKinds.length &&
              kinds.containsAll(EventKind.channelMessageEventKinds);
        }),
        isEmpty,
      );
      expect(
        channels.firstWhere((channel) => channel.id == _channelA).lastMessageAt,
        DateTime.fromMillisecondsSinceEpoch(30 * 1000, isUtc: true),
      );
      expect(
        channels.firstWhere((channel) => channel.id == _channelB).lastMessageAt,
        DateTime.fromMillisecondsSinceEpoch(40 * 1000, isUtc: true),
      );
    },
  );

  test(
    'metadata, members, DM profiles and hidden DMs travel in one query',
    () async {
      final session = _FakeRelaySession(
        memberships: [
          _membership(_channelA, myPk),
          _membership(_channelB, myPk, additionalPubkey: 'alice'),
        ],
        metadata: [
          _meta(id: _channelA, name: 'general'),
          _meta(
            id: _channelB,
            name: 'DM',
            channelType: 'dm',
            participants: const [myPk, 'alice'],
          ),
        ],
        profiles: [_profile('alice', displayName: 'Alice')],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      final channels = await container.read(channelsProvider.future);

      // Cold start: the DM's other member is known from the membership
      // event, so its profile joins the first (and only) query.
      expect(session.coalescedQueryGroups, hasLength(1));
      _expectMetadataGroup(
        session.coalescedQueryGroups.single,
        channelIds: [_channelA, _channelB],
        myPk: myPk,
        profileAuthors: ['alice'],
      );
      expect(
        channels.firstWhere((channel) => channel.id == _channelB).participants,
        contains('Alice'),
      );

      // A refresh (the backstop, a resume, a pull) reads the same four in
      // one query again, the DM participant now known from the list.
      await container.read(channelsProvider.notifier).refresh();
      expect(session.coalescedQueryGroups, hasLength(2));
      _expectMetadataGroup(
        session.coalescedQueryGroups.last,
        channelIds: [_channelA, _channelB],
        myPk: myPk,
        profileAuthors: ['alice'],
      );
      expect(session.historyFilters, hasLength(2));
      expect(
        session.historyFilters.every((f) => f.kinds.single == 39002),
        isTrue,
      );
    },
  );

  test(
    'a DM participant the memberships did not name costs one follow-up read',
    () async {
      final session = _FakeRelaySession(
        memberships: [_membership(_channelB, myPk)],
        metadata: [
          _meta(
            id: _channelB,
            name: 'DM',
            channelType: 'dm',
            participants: const [myPk, 'bob'],
          ),
        ],
        profiles: [_profile('bob', displayName: 'Bob')],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      final channels = await container.read(channelsProvider.future);

      expect(session.coalescedQueryGroups, hasLength(2));
      _expectMetadataGroup(
        session.coalescedQueryGroups.first,
        channelIds: [_channelB],
        myPk: myPk,
        profileAuthors: null,
      );
      final followUp = session.coalescedQueryGroups.last.single;
      expect(followUp.kinds, [0]);
      expect(followUp.authors, ['bob']);
      expect(channels.single.participants, contains('Bob'));
    },
  );

  test('ephemeral (TTL) channels appear in the list', () async {
    // Regression: previously the provider unconditionally dropped any channel
    // with a `ttl` tag, which made TTL channels invisible on iOS even when the
    // user was a member. They should be included so the existing
    // `_EphemeralBadge` UI in `channels_page.dart` can render them.
    final session = _FakeRelaySession(
      memberships: [_membership(_channelA, myPk), _membership(_channelB, myPk)],
      metadata: [
        _meta(id: _channelA, name: 'general'),
        _meta(
          id: _channelB,
          name: 'agent-creation-deep-dive',
          ttlSeconds: 86400,
        ),
      ],
    );
    final container = _buildContainer(session: session);
    addTearDown(container.dispose);

    final channels = await container.read(channelsProvider.future);

    expect(
      channels.map((c) => c.name),
      containsAll(['general', 'agent-creation-deep-dive']),
    );
    final ephemeral = channels.firstWhere(
      (c) => c.name == 'agent-creation-deep-dive',
    );
    expect(ephemeral.isEphemeral, isTrue);
    expect(ephemeral.ttlSeconds, 86400);
  });

  test('hidden DMs are filtered from the channel list', () async {
    final session = _FakeRelaySession(
      memberships: [_membership(_channelA, myPk), _membership(_channelB, myPk)],
      metadata: [
        _meta(id: _channelA, name: 'Alice', channelType: 'dm'),
        _meta(id: _channelB, name: 'Bob', channelType: 'dm'),
      ],
      hiddenDmEvents: [
        _hiddenDms([_channelA], pubkey: myPk),
      ],
    );
    final container = _buildContainer(session: session);
    addTearDown(container.dispose);

    final channels = await container.read(channelsProvider.future);

    expect(channels.map((c) => c.id), [_channelB]);
    expect(
      session.readFilters.any(
        (filter) =>
            filter.kinds.contains(EventKind.dmVisibility) &&
            filter.tags['#p']?.single == myPk,
      ),
      isTrue,
    );
  });

  test(
    'archived kind:39000 metadata sets Channel.isArchived (covers TTL auto-archive)',
    () async {
      // The relay's TTL reaper auto-archives expired ephemeral channels and
      // republishes kind:39000 with `["archived", "true"]`. The Channel needs
      // `archivedAt != null` so the `_SliverChannelsList` filter
      // (`!channel.isArchived`) hides it from the sidebar after expiry.
      // Previously the mobile parser ignored the `archived` tag, so expired
      // TTL channels would have stayed visible after the `!isEphemeral` guard
      // was removed.
      final session = _FakeRelaySession(
        memberships: [
          _membership(_channelA, myPk),
          _membership(_channelB, myPk),
        ],
        metadata: [
          _meta(id: _channelA, name: 'active'),
          _meta(
            id: _channelB,
            name: 'expired-ttl',
            ttlSeconds: 86400,
            archived: true,
          ),
        ],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      final channels = await container.read(channelsProvider.future);
      final expired = channels.firstWhere((c) => c.name == 'expired-ttl');
      expect(expired.isArchived, isTrue);
      expect(expired.isEphemeral, isTrue);
      // The active channel must not be flagged archived.
      final active = channels.firstWhere((c) => c.name == 'active');
      expect(active.isArchived, isFalse);
    },
  );

  test(
    'archive transition invalidates cached channelDetailsProvider',
    () async {
      // Codex review v2 caught: if a TTL channel is opened (caching its
      // ChannelDetails) and then the reaper archives it, the cached details
      // — built from the pre-archive kind:39000 — would clobber the newer
      // archivedAt set on the base Channel during `mergeDetails`. We invalidate
      // the details provider when the archived state flips so the next
      // mergeDetails sees fresh data.
      final session = _FakeRelaySession(
        memberships: [_membership(_channelA, myPk)],
        metadata: [_meta(id: _channelA, name: 'active')],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      int detailsFetches() => session.readFilters
          .where((f) => f.kinds.contains(39000) && f.tags['#d'] != null)
          .length;

      // Initial load.
      final initial = await container.read(channelsProvider.future);
      expect(initial.single.isArchived, isFalse);

      // Prime the detail cache.
      final detailsFiltersBefore = detailsFetches();
      await container.read(channelDetailsProvider(_channelA).future);
      expect(detailsFetches() - detailsFiltersBefore, 1);

      // Simulate the reaper auto-archiving the channel by swapping the
      // metadata the fake returns, then refreshing the channels provider.
      session.metadata
        ..clear()
        ..add(_meta(id: _channelA, name: 'active', archived: true));
      await container.read(channelsProvider.notifier).refresh();
      final refreshed = container.read(channelsProvider).value!;
      expect(refreshed.single.isArchived, isTrue);

      // Take a fresh baseline AFTER the refresh — the refresh itself issues a
      // `kinds:[39000], #d:[id]` query as part of channel metadata refetch and
      // we must not count that toward our invalidation assertion. Only the
      // fetch triggered by the second `channelDetailsProvider` read should be
      // attributed to invalidation.
      final detailsFiltersAfterRefresh = detailsFetches();

      // Reading the details provider again must trigger a fresh fetch — proving
      // the prior cache was invalidated by the archive transition. Without
      // invalidation, Riverpod would return the cached pre-archive details and
      // no new `kinds:[39000], #d:[id]` filter would be sent.
      await container.read(channelDetailsProvider(_channelA).future);
      expect(detailsFetches() - detailsFiltersAfterRefresh, greaterThan(0));
    },
  );

  test(
    'keeps cached channels and live subscriptions during reconnect',
    () async {
      final session = _FakeRelaySession(
        memberships: [_membership(_channelA, myPk)],
        metadata: [_meta(id: _channelA, name: 'general')],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      final initial = await container.read(channelsProvider.future);
      expect(initial.single.name, 'general');
      expect(session.liveFilterBundles, hasLength(1));

      session.setStatus(SessionStatus.reconnecting);
      final reconnecting = await container.read(channelsProvider.future);

      expect(reconnecting.single.name, 'general');
      expect(session.liveFilterBundles, hasLength(1));
      expect(session.activeSubscriptionCount, 1);
      expect(session.unsubscribeCount, 0);
    },
  );

  test(
    'refreshes cached channels after a disconnected community switch',
    () async {
      final session = _FakeRelaySession(
        memberships: [_membership(_channelA, myPk)],
        metadata: [_meta(id: _channelA, name: 'general')],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      expect(
        (await container.read(channelsProvider.future)).single.name,
        'general',
      );

      session.setStatus(SessionStatus.disconnected);
      session.memberships = [_membership(_channelB, myPk)];
      session.metadata = [_meta(id: _channelB, name: 'random')];
      container
          .read(relayConfigProvider.notifier)
          .update(baseUrl: 'https://new-community.example');
      await Future<void>.delayed(Duration.zero);
      expect(container.read(channelsProvider).value?.single.name, 'general');

      session.setStatus(SessionStatus.connected);
      await _waitUntil(
        () => container.read(channelsProvider).value?.single.name == 'random',
      );

      expect(container.read(channelsProvider).value?.single.name, 'random');
    },
  );

  test('recovers an initial fetch failure after reconnecting', () async {
    final session = _FakeRelaySession(
      memberships: [_membership(_channelA, myPk)],
      metadata: [_meta(id: _channelA, name: 'general')],
      membershipFailures: 1,
    );
    final container = _buildContainer(session: session);
    addTearDown(container.dispose);

    await expectLater(container.read(channelsProvider.future), throwsException);

    session.setStatus(SessionStatus.reconnecting);
    session.setStatus(SessionStatus.connected);
    // The connected transition's refresh reads through the coalescer, which
    // answers at the end of the turn; wait for the value rather than a turn.
    await _waitUntil(() => container.read(channelsProvider).hasValue);

    final recovered = await container.read(channelsProvider.future);
    expect(recovered.single.name, 'general');
  });

  test(
    'preserves a successfully loaded empty list while disconnected',
    () async {
      final session = _FakeRelaySession(memberships: [], metadata: []);
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      expect(await container.read(channelsProvider.future), isEmpty);
      final fetchCount = session.readFilters.length;

      session.setStatus(SessionStatus.reconnecting);
      expect(await container.read(channelsProvider.future), isEmpty);
      expect(session.readFilters, hasLength(fetchCount));
    },
  );

  test(
    'initial fetch issues the membership REQ then one coalesced query',
    () async {
      final session = _FakeRelaySession(
        memberships: [_membership(_channelA, myPk)],
        metadata: [_meta(id: _channelA, name: 'general')],
      );
      final container = _buildContainer(session: session);
      addTearDown(container.dispose);

      await container.read(channelsProvider.future);

      // The membership page is the one WebSocket read; it feeds the rest.
      expect(session.historyFilters, hasLength(1));
      expect(session.historyFilters.single.kinds, [39002]);
      expect(session.historyFilters.single.tags['#p'], [myPk]);

      // Everything keyed by the channel ids goes out as one `/query`.
      expect(session.coalescedQueryGroups, hasLength(1));
      _expectMetadataGroup(
        session.coalescedQueryGroups.single,
        channelIds: [_channelA],
        myPk: myPk,
        profileAuthors: null,
      );

      // And one live subscription on the resulting channel.
      expect(session.liveFilterBundles, hasLength(1));
      expect(session.liveFilterBundles.single.single.tags['#h'], [_channelA]);
    },
  );

  test(
    'resume does not refresh when the session is about to reconnect',
    () async {
      final session = _FakeRelaySession(
        memberships: [_membership(_channelA, myPk)],
        metadata: [_meta(id: _channelA, name: 'general')],
      );
      final lifecycle = _FakeAppLifecycleNotifier();
      final container = _buildContainer(session: session, lifecycle: lifecycle);
      addTearDown(container.dispose);

      await container.read(channelsProvider.future);
      expect(session.historyFilters, hasLength(1));
      expect(session.coalescedQueryGroups, hasLength(1));

      // Ten seconds in the background: the socket is gone and the session
      // will reconnect on resume, replaying the live subscriptions itself.
      session.reconnectOnResume = true;
      lifecycle.set(AppLifecycleState.paused);
      lifecycle.set(AppLifecycleState.resumed);
      await _settle();

      expect(session.historyFilters, hasLength(1));
      expect(session.coalescedQueryGroups, hasLength(1));

      // The reconnect's connected transition is the one round that runs.
      session.setStatus(SessionStatus.reconnecting);
      session.setStatus(SessionStatus.connected);
      await _waitUntil(() => session.historyFilters.length == 2);
      await _settle();

      expect(session.historyFilters, hasLength(2));
      expect(session.coalescedQueryGroups, hasLength(2));
    },
  );

  test('resume with a live socket refreshes once', () async {
    final session = _FakeRelaySession(
      memberships: [_membership(_channelA, myPk)],
      metadata: [_meta(id: _channelA, name: 'general')],
    );
    final lifecycle = _FakeAppLifecycleNotifier();
    final container = _buildContainer(session: session, lifecycle: lifecycle);
    addTearDown(container.dispose);

    await container.read(channelsProvider.future);

    session.reconnectOnResume = false;
    lifecycle.set(AppLifecycleState.paused);
    lifecycle.set(AppLifecycleState.resumed);
    await _waitUntil(() => session.historyFilters.length == 2);
    await _settle();

    expect(session.historyFilters, hasLength(2));
    expect(session.coalescedQueryGroups, hasLength(2));
  });
}

const _channelA = '11111111-1111-4111-8111-111111111111';
const _channelB = '22222222-2222-4222-8222-222222222222';
const _channelD = '44444444-4444-4444-8444-444444444444';

List<String> _channelIds(int count) => [
  for (var i = 0; i < count; i++)
    '${(i + 1).toRadixString(16).padLeft(8, '0')}-0000-4000-8000-000000000000',
];

/// The coalesced group every channel load sends: metadata by `#d`, member
/// lists by `#d`, the hidden-DM list by `#p`, and — when a DM participant is
/// already known — their kind:0 profiles by author.
void _expectMetadataGroup(
  List<NostrFilter> group, {
  required List<String> channelIds,
  required String myPk,
  required List<String>? profileAuthors,
}) {
  expect(group, hasLength(profileAuthors == null ? 3 : 4));
  final metadata = group.singleWhere((f) => f.kinds.contains(39000));
  expect(metadata.tags['#d'], channelIds);
  expect(metadata.limit, channelIds.length);
  final members = group.singleWhere((f) => f.kinds.contains(39002));
  expect(members.tags['#d'], channelIds);
  expect(members.limit, channelIds.length);
  final hidden = group.singleWhere(
    (f) => f.kinds.contains(EventKind.dmVisibility),
  );
  expect(hidden.tags['#p'], [myPk]);
  if (profileAuthors != null) {
    final profiles = group.singleWhere((f) => f.kinds.contains(0));
    expect(profiles.authors, profileAuthors);
  }
}

/// Build a kind:39002 membership event tagged with the channel id and member.
NostrEvent _membership(
  String channelId,
  String pubkey, {
  String? additionalPubkey,
}) => NostrEvent(
  id: 'mem-$channelId',
  pubkey: 'creator',
  createdAt: 1,
  kind: 39002,
  tags: [
    ['d', channelId],
    ['p', pubkey],
    if (additionalPubkey != null) ['p', additionalPubkey],
  ],
  content: '',
  sig: 'sig',
);

NostrEvent _hiddenDms(List<String> channelIds, {required String pubkey}) =>
    NostrEvent(
      id: 'hidden-${channelIds.join('-')}',
      pubkey: 'relay',
      createdAt: 2,
      kind: EventKind.dmVisibility,
      tags: [
        ['d', pubkey],
        ['p', pubkey],
        for (final channelId in channelIds) ['h', channelId],
      ],
      content: '',
      sig: 'sig',
    );

/// Build a kind:39000 channel metadata event.
NostrEvent _meta({
  required String id,
  required String name,
  String channelType = 'stream',
  int createdAt = 1,
  int? ttlSeconds,
  bool archived = false,
  List<String> participants = const [],
}) => NostrEvent(
  id: 'meta-$id',
  pubkey: 'creator',
  createdAt: createdAt,
  kind: 39000,
  tags: [
    ['d', id],
    ['name', name],
    ['t', channelType],
    ['public'],
    if (ttlSeconds != null) ['ttl', '$ttlSeconds'],
    if (archived) ['archived', 'true'],
    for (final pubkey in participants) ['p', pubkey],
  ],
  content: '',
  sig: 'sig',
);

/// Build a kind:0 profile event.
NostrEvent _profile(String pubkey, {required String displayName}) => NostrEvent(
  id: 'profile-$pubkey',
  pubkey: pubkey,
  createdAt: 1,
  kind: 0,
  tags: const [],
  content: jsonEncode({'display_name': displayName}),
  sig: 'sig',
);

/// Build a channel message event.
NostrEvent _message(
  String id, {
  required String channel,
  required int createdAt,
  int kind = EventKind.streamMessageV2,
}) => NostrEvent(
  id: id,
  pubkey: 'alice',
  createdAt: createdAt,
  kind: kind,
  tags: [
    ['h', channel],
  ],
  content: 'hello',
  sig: 'sig',
);

ProviderContainer _buildContainer({
  required _FakeRelaySession session,
  _FakeAppLifecycleNotifier? lifecycle,
  Duration liveRebuildDebounce = Duration.zero,
}) {
  return ProviderContainer(
    retry: (_, _) => null,
    overrides: [
      appLifecycleProvider.overrideWith(
        () => lifecycle ?? _FakeAppLifecycleNotifier(),
      ),
      relaySessionProvider.overrideWith(() => session),
      myPubkeyProvider.overrideWithValue('me'),
      channelsProvider.overrideWith(
        () => ChannelsNotifier(liveRebuildDebounce: liveRebuildDebounce),
      ),
    ],
  );
}

Future<void> _settle() async {
  for (var i = 0; i < 10; i++) {
    await Future<void>.delayed(Duration.zero);
  }
}

Future<void> _waitUntil(bool Function() predicate) async {
  for (var i = 0; i < 100; i++) {
    if (predicate()) return;
    await Future<void>.delayed(Duration.zero);
  }
  fail('Timed out waiting for asynchronous provider work');
}

/// Fake [RelaySessionNotifier] that answers reads from canned events, groups
/// [query] calls per event-loop turn like the real coalescer, records live
/// subscriptions, and counts frames.
class _FakeRelaySession extends RelaySessionNotifier {
  _FakeRelaySession({
    required this.memberships,
    required this.metadata,
    this.hiddenDmEvents = const [],
    this.recentMessages = const [],
    this.profiles = const [],
    this.membershipFailures = 0,
  });

  List<NostrEvent> memberships;
  List<NostrEvent> metadata;
  final List<NostrEvent> hiddenDmEvents;
  final List<NostrEvent> recentMessages;
  final List<NostrEvent> profiles;
  int membershipFailures;

  /// Filters read over a WebSocket REQ ([fetchHistory], [fetchHistoryAll]).
  final List<NostrFilter> historyFilters = [];

  /// Filters read through the coalescer ([query]), in call order.
  final List<NostrFilter> coalescedQueryFilters = [];

  /// [query] filters grouped by event-loop turn — one `POST /query` each.
  final List<List<NostrFilter>> coalescedQueryGroups = [];

  /// Every one-shot read, WebSocket or coalesced, in call order.
  final List<NostrFilter> readFilters = [];

  /// Filter lists handed to [queryRelay].
  final List<List<NostrFilter>> queryBatches = [];

  /// Filter lists handed to [subscribeAll] (one REQ each), in order.
  final List<List<NostrFilter>> liveFilterBundles = [];

  /// `'subscribe'` / `'close'` in the order the live REQs and CLOSEs went out.
  final List<String> liveOperations = [];

  /// The number of open live subscriptions after each subscribe or close.
  final List<int> activeCountHistory = [];

  /// WebSocket REQ frames: every history read and every live subscription.
  int reqFrames = 0;

  /// WebSocket CLOSE frames: every live unsubscribe.
  int closeFrames = 0;

  /// HTTP calls: every [queryRelay] and every coalesced [query] group.
  int httpCalls = 0;

  /// What [willReconnectOnResume] reports.
  bool reconnectOnResume = false;

  final Map<int, (List<NostrFilter>, void Function(NostrEvent))>
  _subscriptions = {};
  int _nextSubscriptionKey = 0;
  Completer<void>? _pausedSubscribe;
  Completer<void>? _subscribeStarted;
  List<(NostrFilter, Completer<List<NostrEvent>>)>? _openQueryGroup;
  int unsubscribeCount = 0;
  int totalSubscribeCount = 0;

  Set<String> get activeChannels => {
    for (final (filters, _) in _subscriptions.values)
      for (final filter in filters) ...?filter.tags['#h'],
  };

  int get activeSubscriptionCount => _subscriptions.length;

  Future<void> get nextSubscribeStarted async {
    final started = _subscribeStarted;
    if (started == null) {
      throw StateError('No paused subscription is pending');
    }
    await started.future;
  }

  void pauseNextSubscribe() {
    if (_pausedSubscribe != null) {
      throw StateError('A subscription is already paused');
    }
    _pausedSubscribe = Completer<void>();
    _subscribeStarted = Completer<void>();
  }

  void resumePausedSubscribe() {
    final paused = _pausedSubscribe;
    if (paused == null) throw StateError('No subscription is paused');
    paused.complete();
  }

  @override
  SessionState build() => const SessionState(status: SessionStatus.connected);

  @override
  bool get willReconnectOnResume => reconnectOnResume;

  List<NostrEvent> _answer(NostrFilter filter) {
    if (filter.kinds.contains(39002) && filter.tags['#p'] != null) {
      if (membershipFailures > 0) {
        membershipFailures--;
        throw Exception('membership fetch failed');
      }
      // Membership query — return all memberships we have for this pubkey.
      final myPk = filter.tags['#p']?.single;
      return memberships
          .where(
            (e) =>
                e.tags.any((t) => t.length >= 2 && t[0] == 'p' && t[1] == myPk),
          )
          .toList();
    }
    if (filter.kinds.contains(39002) && filter.tags['#d'] != null) {
      final ids = filter.tags['#d']!.toSet();
      return memberships
          .where((e) => ids.contains(e.getTagValue('d')))
          .toList();
    }
    if (filter.kinds.contains(EventKind.dmVisibility)) {
      return hiddenDmEvents;
    }
    if (filter.kinds.contains(39000)) {
      // Metadata query — return all metadata events whose `d` tag matches.
      final ids = (filter.tags['#d'] ?? const <String>[]).toSet();
      return metadata.where((e) => ids.contains(e.getTagValue('d'))).toList();
    }
    if (filter.kinds.contains(0)) {
      final authors = (filter.authors ?? const <String>[]).toSet();
      return profiles.where((e) => authors.contains(e.pubkey)).toList();
    }
    return const [];
  }

  @override
  Future<List<NostrEvent>> fetchHistory(
    NostrFilter filter, {
    Duration timeout = const Duration(seconds: 8),
  }) async {
    reqFrames++;
    historyFilters.add(filter);
    readFilters.add(filter);
    return _answer(filter);
  }

  @override
  Future<List<NostrEvent>> fetchHistoryAll(
    List<NostrFilter> filters, {
    Duration timeout = const Duration(seconds: 8),
  }) async {
    reqFrames++;
    historyFilters.addAll(filters);
    readFilters.addAll(filters);
    return [for (final filter in filters) ..._answer(filter)];
  }

  @override
  Future<List<NostrEvent>> query(NostrFilter filter) {
    coalescedQueryFilters.add(filter);
    readFilters.add(filter);
    var group = _openQueryGroup;
    if (group == null) {
      final newGroup = <(NostrFilter, Completer<List<NostrEvent>>)>[];
      group = _openQueryGroup = newGroup;
      coalescedQueryGroups.add([]);
      httpCalls++;
      Timer(Duration.zero, () {
        _openQueryGroup = null;
        for (final (pending, completer) in newGroup) {
          try {
            completer.complete(_answer(pending));
          } catch (error, stack) {
            completer.completeError(error, stack);
          }
        }
      });
    }
    final completer = Completer<List<NostrEvent>>();
    group.add((filter, completer));
    coalescedQueryGroups.last.add(filter);
    return completer.future;
  }

  @override
  Future<List<NostrEvent>> queryRelay(
    List<NostrFilter> filters, {
    Duration timeout = const Duration(seconds: 8),
  }) async {
    httpCalls++;
    queryBatches.add(filters);
    return recentMessages.where((event) {
      return filters.any((filter) {
        if (!filter.kinds.contains(event.kind)) return false;
        for (final entry in filter.tags.entries) {
          final tagName = entry.key.startsWith('#')
              ? entry.key.substring(1)
              : entry.key;
          if (!event.tags.any(
            (tag) =>
                tag.length > 1 &&
                tag[0] == tagName &&
                entry.value.contains(tag[1]),
          )) {
            return false;
          }
        }
        return true;
      });
    }).toList();
  }

  @override
  Future<void Function()> subscribe(
    NostrFilter filter,
    void Function(NostrEvent) onEvent, {
    void Function(String message)? onClosed,
  }) => subscribeAll([filter], onEvent, onClosed: onClosed);

  @override
  Future<void Function()> subscribeAll(
    List<NostrFilter> filters,
    void Function(NostrEvent) onEvent, {
    void Function(String message)? onClosed,
  }) async {
    totalSubscribeCount++;
    reqFrames++;
    liveFilterBundles.add(List.of(filters));
    final paused = _pausedSubscribe;
    if (paused != null) {
      _subscribeStarted!.complete();
      await paused.future;
      _pausedSubscribe = null;
      _subscribeStarted = null;
    }
    final subscriptionKey = ++_nextSubscriptionKey;
    _subscriptions[subscriptionKey] = (filters, onEvent);
    liveOperations.add('subscribe');
    activeCountHistory.add(_subscriptions.length);
    return () {
      final subscription = _subscriptions.remove(subscriptionKey);
      if (subscription == null) return;
      unsubscribeCount++;
      closeFrames++;
      liveOperations.add('close');
      activeCountHistory.add(_subscriptions.length);
    };
  }

  void setStatus(SessionStatus status) {
    state = SessionState(status: status);
  }

  /// Emit a live event to all subscribers.
  void emit(NostrEvent event) {
    for (final (_, listener) in List.of(_subscriptions.values)) {
      listener(event);
    }
  }
}

class _FakeAppLifecycleNotifier extends AppLifecycleNotifier {
  @override
  AppLifecycleState build() => AppLifecycleState.resumed;

  /// Drive the lifecycle by hand; the real notifier also pokes the session,
  /// which these tests script through [_FakeRelaySession] directly.
  void set(AppLifecycleState next) {
    state = next;
  }
}
