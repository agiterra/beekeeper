import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/features/coding_sessions/state/coding_sessions_state.dart'
    as state;
import 'package:beekeeper/features/coding_sessions/ui/observer_contract.dart';
import 'package:beekeeper/shared/relay/nostr_models.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../domain/coding_session_fixtures.dart';

/// The binding that joins the state layer to the pages.
///
/// Without these the integration is unpinned: the pages have their own fake
/// binding and the notifier has its own container tests, so replacing the
/// default with [UnboundCodingSessionObserverBinding] — a build that reads
/// nothing from the relay — leaves every other test green.
void main() {
  setUp(resetEventIds);

  test('the pages default to the relay observer, not the unbound one', () {
    final container = ProviderContainer();
    addTearDown(container.dispose);

    expect(
      container.read(codingSessionObserverBindingProvider),
      isA<RelayCodingSessionObserverBinding>(),
    );
  });

  testWidgets('projects the folded read the notifier holds', (tester) async {
    final projected = await _watch(
      tester,
      _read(leasesRead: true, historyTruncated: true),
    );

    expect(projected.channelId, channelId);
    expect(projected.sessions, hasLength(1));
    expect(projected.sessions.single.sessionRef, sessionRefA);
    expect(projected.executions, hasLength(1));
    expect(projected.connection, CodingSessionObserverConnection.open);
    expect(projected.truncatedAt1000, isTrue);
    expect(
      projected.signaturesVerified,
      isFalse,
      reason: 'no verifier was reachable for this read',
    );
    expect(projected.transcriptBlocksByExecution, isEmpty);
  });

  // The projection is the only place these three cross the seam, and a page
  // that renders a transcript, a refusal count or an error it was never given
  // is a page inventing facts. Pinned here rather than implied by the pages'
  // own fakes.
  testWidgets('carries the transcript, the counts and the error across', (
    tester,
  ) async {
    final projected = await _watch(
      tester,
      _read(
        leasesRead: true,
        lastError: 'relay unreachable',
        extra: [
          transcriptEvent(
            eventSeq: 1,
            item: {'kind': 'assistant_text', 'text': 'on it'},
          ),
          // Correctly shaped, signed by nobody this target's create named.
          metadataEvent(
            status: 'failed',
            pubkey: otherProviderPubkey,
            createdAt: 5000,
          ),
        ],
      ),
    );
    final session = projected.sessions.single;

    expect(projected.blocksFor(session).single.items.single.text, 'on it');
    expect(projected.counts.rejectedAuthor, 1);
    expect(projected.counts.rejectedAuthorByKind, {
      EventKind.codingSessionMetadata: 1,
    });
    expect(projected.lastError, 'relay unreachable');
  });

  testWidgets('carries this device\'s own losses to the pages', (tester) async {
    final projected = await _watch(
      tester,
      _read(leasesRead: true, evictedByGeneration: {target().key: 42}),
    );

    expect(projected.evictedEventCount, 42);
    expect(projected.evictedFor(projected.sessions.single), 42);
  });

  testWidgets('keys reachability by the session key the pages look up', (
    tester,
  ) async {
    final nowSeconds = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final projected = await _watch(
      tester,
      _read(leasesRead: true, extra: [leaseEvent(createdAt: nowSeconds)]),
    );
    final session = projected.sessions.single;

    expect(projected.reachabilityBySession.keys, [session.key]);
    expect(
      projected.reachabilityFor(session.key).kind,
      CodingSessionReachabilityKind.reachable,
    );
  });

  testWidgets('an unread lease query projects unknown, not a denial', (
    tester,
  ) async {
    final nowSeconds = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final projected = await _watch(
      tester,
      // The lease is in hand; the lease *query* has not come back, and D8 says
      // that is not an answer either way.
      _read(leasesRead: false, extra: [leaseEvent(createdAt: nowSeconds)]),
    );
    final session = projected.sessions.single;

    expect(
      projected.reachabilityFor(session.key).kind,
      CodingSessionReachabilityKind.unknown,
    );
  });
}

/// One create-backed running execution, folded exactly as the notifier folds.
state.CodingSessionObserverSnapshot _read({
  required bool leasesRead,
  bool historyTruncated = false,
  List<NostrEvent> extra = const [],
  Map<String, int> evictedByGeneration = const {},
  String? lastError,
}) {
  final view = readCodingSessionChannel(
    channelId: channelId,
    verifier: null,
    leasesRead: leasesRead,
    historyTruncated: historyTruncated,
    events: [
      genesisEvent(eventId: genesisEventIdA),
      createEvent(
        commandId: 'cmd-1',
        sessionRef: sessionRefA,
        genesisRef: genesisEventIdA,
      ),
      receiptEvent(commandId: 'cmd-1', status: 'created'),
      metadataEvent(status: 'running', sessionRef: sessionRefA),
      ...extra,
    ],
  );
  return state.CodingSessionObserverSnapshot.fromView(
    view,
    connection: state.CodingSessionObserverConnection.open,
    evictedByGeneration: evictedByGeneration,
    lastError: lastError,
  );
}

/// Watch the bound observer through a real [WidgetRef] and return what the
/// pages would see.
Future<CodingSessionObserverSnapshot> _watch(
  WidgetTester tester,
  state.CodingSessionObserverSnapshot held,
) async {
  late CodingSessionObserverSnapshot seen;
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        state
            .codingSessionChannelObserverProvider(channelId)
            .overrideWith(() => _HeldObserverNotifier(held)),
      ],
      child: _BindingProbe(onSnapshot: (snapshot) => seen = snapshot),
    ),
  );
  await tester.pump();
  return seen;
}

/// A notifier that holds one folded read, standing in for the relay.
class _HeldObserverNotifier extends state.CodingSessionChannelObserverNotifier {
  _HeldObserverNotifier(this.held) : super(channelId);

  final state.CodingSessionObserverSnapshot held;

  @override
  state.CodingSessionObserverSnapshot build() => held;
}

/// Reads the binding the pages read, through a widget's own ref.
class _BindingProbe extends ConsumerWidget {
  const _BindingProbe({required this.onSnapshot});

  final void Function(CodingSessionObserverSnapshot snapshot) onSnapshot;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final binding = ref.watch(codingSessionObserverBindingProvider);
    onSnapshot(binding.watch(ref, channelId));
    return const SizedBox.shrink();
  }
}
