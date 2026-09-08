import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/features/coding_sessions/state/coding_sessions_state.dart';
import 'package:buzz/shared/relay/relay.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../domain/coding_session_fixtures.dart';

/// Tests for the mobile coding-session observer's state layer.
///
/// The relay is stubbed with [_FakeRelaySession], which records every filter
/// the notifier sends, answers `queryRelay` / `fetchHistoryAll` from a canned
/// event list the way the relay does (each filter trimmed to its own limit,
/// then one union), and delivers live events only to subscriptions whose
/// filters actually match — so a filter shape regression fails here rather
/// than silently reading less.
void main() {
  const otherChannelId = 'd0d0d0d0-0000-4000-8000-000000000002';

  setUp(resetEventIds);

  group('history reads', () {
    test('reads the contract filter set, every one #h-scoped, in one bridge '
        'query', () async {
      final session = _FakeRelaySession();
      final container = _container(session);
      addTearDown(container.dispose);

      await _start(container);

      // One REQ for the four live filters, one POST /query for the ten
      // history filters; nothing on the socket's read lane.
      expect(session.operations, ['subscribeAll', 'query']);
      expect(session.queryBundles.single, hasLength(10));
      expect(session.reqBundles, isEmpty);
      for (final filter in session.historyFilters) {
        expect(filter.kinds, isNotEmpty, reason: 'a filter without kinds 403s');
        expect(filter.tags['#h'], [channelId]);
        expect(filter.authors, isNull, reason: 'authority is channel-scoped');
      }

      expect(
        session.historyFilters.map(_shape),
        unorderedEquals([
          '[44223, 44224, 44225] limit 1000',
          '[44221] limit 1000',
          '[44224] limit 1000',
          '[44226] limit 1000',
          '[44229] limit 1000',
          '[44227] limit 1000',
          '[44230] limit 1000',
          '[24223] limit 1000',
          '[44228] limit 500',
          '[40099] limit 500',
        ]),
      );
    });

    test('opens facts, creates, names, goals, closures and leases as one '
        'live REQ', () async {
      final session = _FakeRelaySession();
      final container = _container(session);
      addTearDown(container.dispose);

      await _start(container);

      expect(session.subscribeBundles.single, hasLength(6));
      for (final filter in session.subscribeFilters) {
        expect(filter.tags['#h'], [channelId]);
        expect(filter.kinds, isNotEmpty);
      }
      expect(
        session.subscribeFilters.map(_shape),
        unorderedEquals([
          '[44223, 44224, 44225] limit 0',
          '[44221, 44226] limit 0',
          '[44229] limit 0',
          '[44227] limit 0',
          '[44230] limit 0',
          '[24223] limit 1000',
        ]),
      );
    });

    test(
      'a failed bridge read falls back to one REQ of the same ten filters',
      () async {
        final session = _FakeRelaySession(
          events: _liveSession(),
          failQuery: true,
        );
        final container = _container(session);
        addTearDown(container.dispose);

        final snapshot = await _start(container);

        expect(session.operations, ['subscribeAll', 'query', 'fetchAll']);
        expect(
          session.reqBundles.single.map(_shape),
          unorderedEquals(session.queryBundles.single.map(_shape)),
        );
        expect(session.reqBundles.single, hasLength(10));
        expect(snapshot.connection, CodingSessionObserverConnection.open);
        expect(snapshot.sessions, hasLength(1));
        expect(snapshot.leasesRead, isTrue);
      },
    );

    test('folds history into an umbrella session', () async {
      final session = _FakeRelaySession(events: _liveSession());
      final container = _container(session);
      addTearDown(container.dispose);

      final snapshot = await _start(container);

      expect(snapshot.sessions, hasLength(1));
      final umbrella = snapshot.sessions.single;
      expect(umbrella.sessionRef, sessionRefA);
      expect(umbrella.founder.pubkey, founderPubkey);
      expect(
        umbrella.founder.resolution,
        CodingSessionFounderResolution.genesis,
      );
      expect(umbrella.status.kind, CodingSessionFoldedStatusKind.working);
      expect(snapshot.executions, hasLength(1));
      expect(snapshot.executions.single.authority.verified, isTrue);
      expect(snapshot.counts.isClean, isTrue);
      expect(snapshot.truncatedAt1000, isFalse);
    });

    test('discloses a full history page as truncated', () async {
      final session = _FakeRelaySession(
        events: [
          ..._liveSession(),
          for (var index = 0; index < codingSessionHistoryPageLimit; index++)
            nameEvent(content: 'name $index', createdAt: 1200 + index),
        ],
      );
      final container = _container(session);
      addTearDown(container.dispose);

      final snapshot = await _start(container);

      expect(snapshot.truncatedAt1000, isTrue);
    });

    test('a full roster page is not a truncated history', () async {
      // 44228/40099 are read at limit 500 and thrown away — 40099 is an
      // ordinary relay receipt, so any busy channel fills that page. Reporting
      // it as truncated history would put a false statement over a complete
      // read of the facts the page actually shows.
      final session = _FakeRelaySession(
        events: [
          ..._liveSession(),
          for (var index = 0; index < 500; index++)
            event(
              kind: EventKind.relayReceipt,
              tags: [
                ['h', channelId],
              ],
              content: '',
            ),
        ],
      );
      final container = _container(session);
      addTearDown(container.dispose);

      final snapshot = await _start(container);

      expect(snapshot.truncatedAt1000, isFalse);
      expect(snapshot.sessions, hasLength(1));
    });

    test('a full lease page is not a truncated history', () async {
      // 24223 leases are read at limit 1000 and are never paginated (D2): a
      // provider republishes one every few seconds, so a busy channel fills
      // that page routinely. "History truncated at 1000 events" is a claim
      // about the transcript the page shows, which a full lease snapshot
      // cannot support.
      final session = _FakeRelaySession(
        events: [
          ..._liveSession(),
          for (var index = 0; index < codingSessionHistoryPageLimit; index++)
            leaseEvent(leaseSequence: index + 1, createdAt: 1400 + index),
        ],
      );
      final container = _container(session);
      addTearDown(container.dispose);

      final snapshot = await _start(container);

      expect(snapshot.truncatedAt1000, isFalse);
      expect(snapshot.sessions, hasLength(1));
      expect(snapshot.leasesRead, isTrue);
    });
  });

  group('live reads', () {
    test('a live transcript event appends and re-folds', () async {
      final session = _FakeRelaySession(events: _liveSession());
      final container = _container(session);
      addTearDown(container.dispose);

      final seeded = await _start(container);
      expect(seeded.transcriptBlocksByExecution, isEmpty);

      session.emit(
        transcriptEvent(
          eventSeq: 1,
          item: {'kind': 'assistant_text', 'text': 'first row'},
        ),
      );
      await pumpEventQueue();

      final snapshot = container.read(
        codingSessionChannelObserverProvider(channelId),
      );
      final blocks = snapshot.transcriptFor(snapshot.sessions.single);
      expect(blocks, hasLength(1));
      expect(blocks.single.items.single.text, 'first row');
      expect(snapshot.transcriptBlocksByExecution[target().key], hasLength(1));
    });

    test('a session founded after the history read appears, verified, from '
        'its live genesis and create', () async {
      // Live finding 2026-09-08: the desktop created a session while the
      // phone already watched the channel, and the phone never showed it —
      // the live REQ carried facts but not the create or the genesis, so
      // the execution had no readable founder and no umbrella to sit under.
      final session = _FakeRelaySession();
      final container = _container(session);
      addTearDown(container.dispose);

      final seeded = await _start(container);
      expect(seeded.sessions, isEmpty);

      for (final event in _liveSession()) {
        session.emit(event);
      }
      await pumpEventQueue();

      final snapshot = container.read(
        codingSessionChannelObserverProvider(channelId),
      );
      expect(snapshot.sessions.single.sessionRef, sessionRefA);
      expect(snapshot.executions.single.authority.verified, isTrue);
    });

    test(
      'a refusal of a create sent after the history read settles it',
      () async {
        // The same finding from this device's side: its own 44221 went out
        // after the observer's read, the provider's `failed` 44224 arrived
        // live, and with no readable create the receipt was refused as an
        // unknown author — the row said "waiting" forever.
        final session = _FakeRelaySession();
        final container = _container(session);
        addTearDown(container.dispose);

        await _start(container);
        session.emit(
          createEvent(
            commandId: 'cmd-9',
            sessionRef: sessionRefB,
            genesisRef: genesisEventIdB,
          ),
        );
        session.emit(
          receiptEvent(
            commandId: 'cmd-9',
            status: 'failed',
            error: const {
              'code': 'PROJECT_CWD_UNRESOLVED',
              'message': 'no working directory is configured for project p',
            },
          ),
        );
        await pumpEventQueue();

        final snapshot = container.read(
          codingSessionChannelObserverProvider(channelId),
        );
        final receipt = snapshot.lifecycleReceiptsByCommandId['cmd-9']!.single;
        expect(receipt.status, CodingSessionReceiptStatus.failed);
        expect(receipt.error?.code, 'PROJECT_CWD_UNRESOLVED');
        expect(snapshot.counts.rejectedAuthor, 0);
        expect(snapshot.sessions, isEmpty);
      },
    );

    test('ignores a live event scoped to another channel', () async {
      final session = _FakeRelaySession(events: _liveSession());
      final container = _container(session);
      addTearDown(container.dispose);

      await _start(container);
      session.emit(
        event(
          kind: EventKind.codingSessionTranscript,
          tags: [
            ['h', otherChannelId],
            ['cst-v', 'cst1-1'],
          ],
          content: '{}',
        ),
        force: true,
      );
      await pumpEventQueue();

      final snapshot = container.read(
        codingSessionChannelObserverProvider(channelId),
      );
      expect(snapshot.counts.malformed, 0);
      expect(snapshot.transcriptBlocksByExecution, isEmpty);
    });

    test('a turn receipt never brings a generation into existence', () async {
      final session = _FakeRelaySession(
        events: [
          receiptEvent(
            commandId: 'cmd-turn',
            status: 'turn_started',
            turnId: 'turn-1',
          ),
          metadataEvent(status: 'running', sessionRef: sessionRefA),
        ],
      );
      final container = _container(session);
      addTearDown(container.dispose);

      final beforeCreate = await _start(container);
      expect(beforeCreate.executions, isEmpty);
      expect(beforeCreate.sessions, isEmpty);

      session.emit(receiptEvent(commandId: 'cmd-1', status: 'created'));
      await pumpEventQueue();

      final snapshot = container.read(
        codingSessionChannelObserverProvider(channelId),
      );
      expect(snapshot.executions, hasLength(1));
      expect(
        snapshot.executions.single.authority.verified,
        isFalse,
        reason: 'no readable create, so the metadata signer is the fallback',
      );
    });

    test(
      'retains at most 2000 raw events per generation under a flood',
      () async {
        final session = _FakeRelaySession(
          events: _liveSession(metadataCreatedAt: 5000),
        );
        final container = _container(session);
        addTearDown(container.dispose);

        await _start(container);
        for (var seq = 1; seq <= 2100; seq++) {
          session.emit(
            transcriptEvent(
              eventSeq: seq,
              item: {'kind': 'assistant_text', 'text': 'row $seq'},
            ),
          );
        }
        await pumpEventQueue();

        final snapshot = container.read(
          codingSessionChannelObserverProvider(channelId),
        );
        final items = snapshot
            .transcriptFor(snapshot.sessions.single)
            .single
            .items;
        // The transcript bucket holds 2000 transcript events: the generation's
        // 44223 metadata is capped separately, so the flood no longer eats a
        // row of its own history to make room for the session's status.
        expect(items, hasLength(2000));
        expect(items.first.eventSeq, 101);
        expect(items.last.eventSeq, 2100);
        expect(snapshot.sessions.single.executions.single.metadata, isNotNull);
        // D10's other loss, and the one only this device knows about: 100
        // events it read and threw away. Both pages disclose it, so the
        // shortened transcript above is never presented as a short one.
        expect(snapshot.evictedByGeneration[target().key], 100);
      },
    );
  });

  group('connection', () {
    test('walks idle -> connecting -> open', () async {
      final session = _FakeRelaySession(
        events: _liveSession(),
        initialStatus: SessionStatus.disconnected,
      );
      final container = _container(session);
      addTearDown(container.dispose);

      final seen = <CodingSessionObserverConnection>[];
      container.listen(codingSessionChannelObserverProvider(channelId), (
        _,
        next,
      ) {
        seen.add(next.connection);
      }, fireImmediately: true);
      await pumpEventQueue();

      expect(seen.first, CodingSessionObserverConnection.idle);
      expect(session.historyFilters, isEmpty);

      session.setStatus(SessionStatus.connected);
      await pumpEventQueue();

      expect(seen, contains(CodingSessionObserverConnection.connecting));
      expect(seen.last, CodingSessionObserverConnection.open);
    });

    test(
      'a failed history read reports the error and keeps the live read',
      () async {
        // One bundle, one answer: when both the bridge and the socket refuse
        // it there is no partial page to show, so the page says error — and
        // the live subscription, opened first, still delivers.
        final session = _FakeRelaySession(
          events: _liveSession(),
          failingKinds: {EventKind.codingSessionGoal},
        );
        final container = _container(session);
        addTearDown(container.dispose);

        final failed = await _start(container);

        expect(failed.connection, CodingSessionObserverConnection.error);
        expect(failed.lastError, contains('history failed'));
        expect(failed.sessions, isEmpty);
        expect(session.operations, ['subscribeAll', 'query', 'fetchAll']);

        for (final event in _liveSession()) {
          session.emit(event, force: true);
        }
        await pumpEventQueue();

        final snapshot = container.read(
          codingSessionChannelObserverProvider(channelId),
        );
        expect(snapshot.sessions, hasLength(1));
      },
    );

    // A first connect still in flight is not "Not connected to this
    // community / Nothing is being read while the connection is down": the
    // socket is up and about to answer, and that page also offers a Retry
    // that cannot help. Report the spinner the state actually warrants.
    test(
      'a first connect in flight is a spinner, not a disconnection',
      () async {
        final session = _FakeRelaySession(
          events: _liveSession(),
          initialStatus: SessionStatus.connecting,
        );
        final container = _container(session);
        addTearDown(container.dispose);

        final snapshot = await _start(container);

        expect(snapshot.connection, CodingSessionObserverConnection.connecting);
        expect(session.historyFilters, isEmpty);
      },
    );

    test('a disconnect keeps the last read and says it is not open', () async {
      final session = _FakeRelaySession(events: _liveSession());
      final container = _container(session);
      addTearDown(container.dispose);

      await _start(container);
      session.setStatus(SessionStatus.reconnecting);
      await pumpEventQueue();

      final snapshot = container.read(
        codingSessionChannelObserverProvider(channelId),
      );
      expect(snapshot.connection, CodingSessionObserverConnection.idle);
      expect(snapshot.sessions, hasLength(1));
      expect(session.activeSubscriptionCount, 0);
    });

    test('reconnect re-reads history and a replay stays one fact', () async {
      final session = _FakeRelaySession(events: _liveSession());
      final container = _container(session);
      addTearDown(container.dispose);

      await _start(container);
      final firstRead = session.historyFilters.length;
      session.emit(
        transcriptEvent(
          eventSeq: 1,
          id: 'f' * 64,
          item: {'kind': 'assistant_text', 'text': 'before the drop'},
        ),
      );
      await pumpEventQueue();

      session.setStatus(SessionStatus.reconnecting);
      await pumpEventQueue();
      session.setStatus(SessionStatus.connected);
      await pumpEventQueue();

      // The relay replays live subscriptions from lastSeen - 5s on reconnect.
      session.emit(
        transcriptEvent(
          eventSeq: 1,
          id: 'f' * 64,
          item: {'kind': 'assistant_text', 'text': 'before the drop'},
        ),
      );
      await pumpEventQueue();

      final snapshot = container.read(
        codingSessionChannelObserverProvider(channelId),
      );
      expect(session.historyFilters.length, firstRead * 2);
      expect(session.activeSubscriptionCount, 1);
      final items = snapshot
          .transcriptFor(snapshot.sessions.single)
          .single
          .items;
      expect(items, hasLength(1));
      expect(snapshot.counts.conflicts, 0);
      expect(snapshot.connection, CodingSessionObserverConnection.open);
    });

    test('refresh drops the read and fetches again', () async {
      final session = _FakeRelaySession(events: _liveSession());
      final container = _container(session);
      addTearDown(container.dispose);

      await _start(container);
      final firstRead = session.historyFilters.length;

      await container
          .read(codingSessionChannelObserverProvider(channelId).notifier)
          .refresh();
      await pumpEventQueue();

      final snapshot = container.read(
        codingSessionChannelObserverProvider(channelId),
      );
      expect(session.historyFilters.length, firstRead * 2);
      expect(snapshot.sessions, hasLength(1));
      expect(snapshot.connection, CodingSessionObserverConnection.open);
    });
  });

  group('reachability', () {
    test('a lease inside its TTL proves a provider is answering', () async {
      final session = _FakeRelaySession(
        events: [..._liveSession(), leaseEvent(createdAt: 1400)],
      );
      final container = _container(session);
      addTearDown(container.dispose);

      final snapshot = await _start(container);
      final verdict = snapshot.reachabilityFor(
        snapshot.sessions.single,
        now: DateTime.fromMillisecondsSinceEpoch(1410 * 1000, isUtc: true),
      );

      expect(snapshot.leasesRead, isTrue);
      expect(verdict.kind, CodingSessionReachabilityKind.reachable);
    });

    test('a lease past its TTL flips to nobody answering', () async {
      final session = _FakeRelaySession(
        events: [..._liveSession(), leaseEvent(createdAt: 1400)],
      );
      final container = _container(session);
      addTearDown(container.dispose);

      final snapshot = await _start(container);
      final verdict = snapshot.reachabilityFor(
        snapshot.sessions.single,
        now: DateTime.fromMillisecondsSinceEpoch(1600 * 1000, isUtc: true),
      );

      expect(verdict.kind, CodingSessionReachabilityKind.noProviderAnswering);
      expect(verdict.leaseAge, const Duration(seconds: 200));
    });

    test(
      'an unread lease query reads unknown, never nobody answering',
      () async {
        final session = _FakeRelaySession(
          events: [..._liveSession(), leaseEvent(createdAt: 1400)],
          failingKinds: {EventKind.codingSessionLease},
        );
        final container = _container(session);
        addTearDown(container.dispose);

        await _start(container);
        // The history bundle failed as a whole; the session arrives live.
        for (final event in _liveSession()) {
          session.emit(event, force: true);
        }
        await pumpEventQueue();
        final snapshot = container.read(
          codingSessionChannelObserverProvider(channelId),
        );
        final verdict = snapshot.reachabilityFor(
          snapshot.sessions.single,
          now: DateTime.fromMillisecondsSinceEpoch(1410 * 1000, isUtc: true),
        );

        expect(snapshot.leasesRead, isFalse);
        expect(verdict.kind, CodingSessionReachabilityKind.unknown);
      },
    );

    test('leases are re-read on the poll interval', () async {
      final session = _FakeRelaySession(events: _liveSession());
      final container = _container(
        session,
        config: const CodingSessionObserverConfig(
          verifier: UnavailableSignatureVerifier(),
          leaseRefreshInterval: Duration(milliseconds: 20),
        ),
      );
      addTearDown(container.dispose);

      await _start(container);
      final firstPoll = session.leaseHistoryCount;
      await Future<void>.delayed(const Duration(milliseconds: 120));

      expect(session.leaseHistoryCount, greaterThan(firstPoll));
    });
  });

  group('signatures', () {
    test('reports null before a read and false when unverifiable', () async {
      final session = _FakeRelaySession(
        events: _liveSession(),
        initialStatus: SessionStatus.disconnected,
      );
      final container = _container(session);
      addTearDown(container.dispose);

      final initial = container.read(
        codingSessionChannelObserverProvider(channelId),
      );
      expect(initial.signaturesVerified, isNull);
      expect(initial.hasRead, isFalse);

      session.setStatus(SessionStatus.connected);
      final snapshot = await _start(container);

      expect(snapshot.signaturesVerified, isFalse);
    });

    test(
      'the default verifier rejects unsigned facts, never trusts them',
      () async {
        final session = _FakeRelaySession(events: _liveSession());
        final container = _container(
          session,
          config: const CodingSessionObserverConfig(),
        );
        addTearDown(container.dispose);

        final snapshot = await _start(container);

        expect(snapshot.signaturesVerified, isTrue);
        expect(snapshot.counts.invalidSignature, greaterThan(0));
        expect(snapshot.sessions, isEmpty);
      },
    );
  });
}

/// One filter's kinds and limit, as a comparable string.
String _shape(NostrFilter filter) => '${filter.kinds} limit ${filter.limit}';

/// A create + genesis + lifecycle receipt + metadata, i.e. one running
/// execution whose authority a create vouches for.
List<NostrEvent> _liveSession({int metadataCreatedAt = 1000}) => [
  genesisEvent(eventId: genesisEventIdA),
  createEvent(
    commandId: 'cmd-1',
    sessionRef: sessionRefA,
    genesisRef: genesisEventIdA,
  ),
  receiptEvent(commandId: 'cmd-1', status: 'created'),
  metadataEvent(
    status: 'running',
    sessionRef: sessionRefA,
    createdAt: metadataCreatedAt,
  ),
];

ProviderContainer _container(
  _FakeRelaySession session, {
  CodingSessionObserverConfig config = const CodingSessionObserverConfig(
    verifier: UnavailableSignatureVerifier(),
  ),
}) => ProviderContainer(
  retry: (_, _) => null,
  overrides: [
    relaySessionProvider.overrideWith(() => session),
    codingSessionObserverConfigProvider.overrideWithValue(config),
    myPubkeyProvider.overrideWithValue('aabb'),
  ],
);

Future<CodingSessionObserverSnapshot> _start(
  ProviderContainer container,
) async {
  container.listen(
    codingSessionChannelObserverProvider(channelId),
    (_, _) {},
    fireImmediately: true,
  );
  await pumpEventQueue();
  return container.read(codingSessionChannelObserverProvider(channelId));
}

class _FakeRelaySession extends RelaySessionNotifier {
  _FakeRelaySession({
    List<NostrEvent> events = const [],
    this.initialStatus = SessionStatus.connected,
    this.failingKinds = const {},
    this.failQuery = false,
  }) : stored = [...events];

  final List<NostrEvent> stored;
  final SessionStatus initialStatus;

  /// A read carrying a filter whose first kind is in this set throws,
  /// standing in for a relay that refuses the bundle.
  final Set<int> failingKinds;

  /// When true the bridge (`queryRelay`) throws and only the socket answers.
  final bool failQuery;

  /// Relay calls in order: `subscribeAll`, `query` (bridge), `fetchAll`
  /// (socket REQ), `query1` (coalesced one-shot).
  final List<String> operations = [];

  /// Every filter read through any history path, in order.
  final List<NostrFilter> historyFilters = [];

  /// Each `queryRelay` filter list.
  final List<List<NostrFilter>> queryBundles = [];

  /// Each `fetchHistoryAll` filter list.
  final List<List<NostrFilter>> reqBundles = [];

  final List<NostrFilter> subscribeFilters = [];
  final List<List<NostrFilter>> subscribeBundles = [];
  final Map<int, (List<NostrFilter>, void Function(NostrEvent))>
  _subscriptions = {};
  int _nextSubscriptionKey = 0;

  int get activeSubscriptionCount => _subscriptions.length;

  int get leaseHistoryCount => historyFilters
      .where((filter) => filter.kinds.contains(EventKind.codingSessionLease))
      .length;

  @override
  SessionState build() => SessionState(status: initialStatus);

  void setStatus(SessionStatus status) => state = SessionState(status: status);

  @override
  Future<List<NostrEvent>> queryRelay(
    List<NostrFilter> filters, {
    Duration timeout = const Duration(seconds: 8),
  }) async {
    operations.add('query');
    queryBundles.add(List.of(filters));
    historyFilters.addAll(filters);
    if (failQuery) throw StateError('bridge unavailable');
    return _answer(filters);
  }

  @override
  Future<List<NostrEvent>> fetchHistoryAll(
    List<NostrFilter> filters, {
    Duration timeout = const Duration(seconds: 8),
  }) async {
    operations.add('fetchAll');
    reqBundles.add(List.of(filters));
    historyFilters.addAll(filters);
    return _answer(filters);
  }

  @override
  Future<List<NostrEvent>> query(NostrFilter filter) async {
    operations.add('query1');
    historyFilters.add(filter);
    return _answer([filter]);
  }

  /// What the relay does with a multi-filter read: each filter's page is
  /// trimmed to its own limit, then the pages are unioned by id.
  List<NostrEvent> _answer(List<NostrFilter> filters) {
    for (final filter in filters) {
      if (failingKinds.contains(filter.kinds.first)) {
        throw StateError('relay refused kinds ${filter.kinds}');
      }
    }
    final seen = <String>{};
    final union = <NostrEvent>[];
    for (final filter in filters) {
      final matches = [
        for (final event in stored)
          if (_matches(filter, event)) event,
      ];
      final page = matches.length > filter.limit
          ? matches.sublist(0, filter.limit)
          : matches;
      for (final event in page) {
        if (seen.add(event.id)) union.add(event);
      }
    }
    return union;
  }

  @override
  Future<void Function()> subscribeAll(
    List<NostrFilter> filters,
    void Function(NostrEvent) onEvent, {
    void Function(String message)? onClosed,
  }) async {
    operations.add('subscribeAll');
    subscribeFilters.addAll(filters);
    subscribeBundles.add(List.of(filters));
    final key = ++_nextSubscriptionKey;
    _subscriptions[key] = (filters, onEvent);
    return () => _subscriptions.remove(key);
  }

  /// Deliver [event] to every subscription one of whose filters matches it.
  void emit(NostrEvent event, {bool force = false}) {
    for (final (filters, listener) in List.of(_subscriptions.values)) {
      if (force || filters.any((filter) => _matches(filter, event))) {
        listener(event);
      }
    }
  }

  static bool _matches(NostrFilter filter, NostrEvent event) {
    if (!filter.kinds.contains(event.kind)) return false;
    for (final entry in filter.tags.entries) {
      final name = entry.key.startsWith('#')
          ? entry.key.substring(1)
          : entry.key;
      final hit = event.tags.any(
        (tag) =>
            tag.length > 1 && tag[0] == name && entry.value.contains(tag[1]),
      );
      if (!hit) return false;
    }
    return true;
  }
}
