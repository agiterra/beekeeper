part of '../channels_provider.dart';

/// The relay accepts at most this many explicit `#h` values in one REQ,
/// summed over every filter the REQ carries
/// (`crates/buzz-relay/src/handlers/req.rs`, `MAX_EXPLICIT_CHANNEL_VALUES`),
/// so one live subscription can cover at most this many channels.
const liveChannelsPerSubscription = 128;

/// Plan the live subscriptions for [channelIds]: one REQ per
/// [liveChannelsPerSubscription] channels, each carrying a single filter over
/// every channel event kind with `limit: 0` (no backlog — history is read
/// separately). Returns one filter list per REQ, in [channelIds] order.
List<List<NostrFilter>> planLiveChannelFilters(Iterable<String> channelIds) {
  final ids = channelIds.toList();
  return [
    for (
      var start = 0;
      start < ids.length;
      start += liveChannelsPerSubscription
    )
      [
        NostrFilter(
          kinds: EventKind.channelEventKinds,
          tags: {
            '#h': ids.sublist(
              start,
              min(start + liveChannelsPerSubscription, ids.length),
            ),
          },
          limit: 0,
        ),
      ],
  ];
}

/// The live subscriptions currently open for one channel set on one relay.
class _LiveBundle {
  _LiveBundle({
    required this.relayBaseUrl,
    required this.channelIds,
    required this.unsubscribers,
  });

  final String relayBaseUrl;
  final Set<String> channelIds;
  final List<void Function()> unsubscribers;

  bool covers(String relayBaseUrl, Set<String> channelIds) =>
      this.relayBaseUrl == relayBaseUrl &&
      setEquals(this.channelIds, channelIds);

  void close() {
    for (final unsubscribe in unsubscribers) {
      unsubscribe();
    }
  }
}

/// Live-subscription lifecycle for [ChannelsNotifier].
///
/// The joined, non-archived channels are covered by one bundle of live REQs
/// ([planLiveChannelFilters]). When the desired set changes the bundle is
/// rebuilt make-before-break — the new REQs are open before the old ones
/// are closed, so no event falls in a gap — after a debounce that folds a
/// burst of membership changes into one rebuild. The first bundle on a relay
/// opens immediately.
extension _ChannelsLiveSubscriptions on ChannelsNotifier {
  /// Record [channels] as the desired live set and converge on it.
  ///
  /// Completes when the first bundle on this relay is open; a later change
  /// is applied after [ChannelsNotifier._liveRebuildDebounce] and this
  /// returns at once so the channel list is not held back by the rebuild.
  Future<void> _subscribeLive(List<Channel> channels) {
    final channelIds = {
      for (final channel in channels)
        if (channel.isMember && !channel.isArchived) channel.id,
    };
    _desiredLiveChannels = channels;
    _desiredLiveChannelIds = channelIds;

    final inFlight = _liveRebuildInFlight;
    if (inFlight != null) {
      // The running rebuild re-reads the desired set before it finishes.
      return inFlight;
    }
    final relayBaseUrl = _relayBaseUrl;
    final active = _liveBundle;
    if (active != null && active.covers(relayBaseUrl, channelIds)) {
      _cancelLiveRebuildTimer();
      _afterLiveSync(channels);
      return Future.value();
    }
    if (active == null || active.relayBaseUrl != relayBaseUrl) {
      // Nothing is listening on this relay yet: open the bundle now.
      _cancelLiveRebuildTimer();
      return _rebuildLiveBundle();
    }
    _scheduleLiveRebuild(channelIds);
    return Future.value();
  }

  void _scheduleLiveRebuild(Set<String> channelIds) {
    if (_liveRebuildTimer != null &&
        setEquals(_liveRebuildPendingIds, channelIds)) {
      // Already scheduled for this very set: keep the deadline, so a stream
      // of events for a just-left channel cannot push the rebuild out forever.
      return;
    }
    _liveRebuildTimer?.cancel();
    _liveRebuildPendingIds = channelIds;
    _liveRebuildTimer = Timer(_liveRebuildDebounce, () {
      _liveRebuildTimer = null;
      _liveRebuildPendingIds = null;
      unawaited(_rebuildLiveBundle());
    });
  }

  void _cancelLiveRebuildTimer() {
    _liveRebuildTimer?.cancel();
    _liveRebuildTimer = null;
    _liveRebuildPendingIds = null;
  }

  /// Run one rebuild worker at a time; a second caller shares its future.
  Future<void> _rebuildLiveBundle() {
    final inFlight = _liveRebuildInFlight;
    if (inFlight != null) return inFlight;
    final completer = Completer<void>();
    final future = completer.future;
    _liveRebuildInFlight = future;
    unawaited(
      _runLiveRebuild().whenComplete(() {
        if (identical(_liveRebuildInFlight, future)) {
          _liveRebuildInFlight = null;
        }
        completer.complete();
      }),
    );
    return future;
  }

  /// Converge the open bundle on the desired set, looping while the desired
  /// set moves under it. Never throws: a failed REQ leaves the previous
  /// bundle listening and the next refresh or reconnect tries again.
  Future<void> _runLiveRebuild() async {
    while (true) {
      final generation = _liveGeneration;
      final relayBaseUrl = _relayBaseUrl;
      final channels = _desiredLiveChannels;
      final channelIds = _desiredLiveChannelIds;
      final active = _liveBundle;
      if (active != null && active.covers(relayBaseUrl, channelIds)) {
        _afterLiveSync(channels);
        return;
      }
      if (!_isConnected) {
        // The connected transition runs a refresh that lands here again.
        return;
      }

      final unsubscribers = <void Function()>[];
      var opened = false;
      try {
        for (final filters in planLiveChannelFilters(channelIds)) {
          final unsubscribe = await _session.subscribeAll(
            filters,
            _handleLiveEvent,
          );
          unsubscribers.add(unsubscribe);
          if (generation != _liveGeneration) {
            _closeAll(unsubscribers);
            return;
          }
          if (_relayBaseUrl != relayBaseUrl || !_isConnected) {
            _closeAll(unsubscribers);
            return;
          }
        }
        opened = true;
      } catch (error) {
        debugPrint('[ChannelsNotifier] live subscription failed: $error');
        _closeAll(unsubscribers);
      }
      if (!opened) return;

      // Make-before-break: the new bundle listens before the old one closes.
      final previous = _liveBundle;
      _liveBundle = _LiveBundle(
        relayBaseUrl: relayBaseUrl,
        channelIds: channelIds,
        unsubscribers: unsubscribers,
      );
      _unknownLiveChannelIds.clear();
      previous?.close();
      // Loop: the desired set may have moved on while the REQs were in flight.
    }
  }

  void _closeAll(List<void Function()> unsubscribers) {
    for (final unsubscribe in unsubscribers) {
      unsubscribe();
    }
  }

  /// After the bundle covers [channels]: catch up on unread events and make
  /// sure the backstop poll is running.
  void _afterLiveSync(List<Channel> channels) {
    unawaited(_catchUpUnreadEvents(channels));
    _backstopTimer ??= _startBackstopTimer();
  }

  /// The 60 s backstop that notices channels created on another device.
  ///
  /// Its first tick lands at a phase offset derived from the key, so a phone
  /// and a desktop on one key — and this poll and the others on this phone —
  /// do not all fire together at connect time; it then repeats every
  /// [ChannelsNotifier._backstopInterval].
  Timer _startBackstopTimer() {
    final offset = phaseOffset(
      'channels-backstop',
      ChannelsNotifier._backstopInterval,
      pubkey: _myPubkey ?? '',
      random: Random().nextDouble(),
    );
    return Timer(offset, () {
      _backstopTimer = Timer.periodic(
        ChannelsNotifier._backstopInterval,
        (_) => _backstopRefresh(),
      );
      unawaited(_backstopRefresh());
    });
  }

  /// A live event named a channel the list does not know. Refresh once per
  /// unknown channel per bundle: the channel is either new (the refresh adds
  /// it and the bundle is rebuilt) or just left (its events keep arriving on
  /// the old bundle until the rebuild lands, and must not refresh again).
  void _refreshForUnknownChannel(String channelId) {
    if (!_unknownLiveChannelIds.add(channelId)) return;
    if (_unknownChannelRefresh != null) {
      _unknownChannelRefreshQueued = true;
      return;
    }
    _unknownChannelRefresh = _runUnknownChannelRefresh();
  }

  Future<void> _runUnknownChannelRefresh() async {
    try {
      do {
        _unknownChannelRefreshQueued = false;
        await refresh();
      } while (_unknownChannelRefreshQueued);
    } finally {
      _unknownChannelRefresh = null;
      _unknownChannelRefreshQueued = false;
    }
  }

  void _clearLiveSubscriptions() {
    _liveGeneration++;
    _cancelLiveRebuildTimer();
    _liveRebuildInFlight = null;
    _desiredLiveChannels = const [];
    _desiredLiveChannelIds = const {};
    _liveBundle?.close();
    _liveBundle = null;
    _unknownLiveChannelIds.clear();
    _backstopTimer?.cancel();
    _backstopTimer = null;
  }
}
