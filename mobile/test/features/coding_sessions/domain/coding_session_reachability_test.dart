import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

import 'coding_session_fixtures.dart';

CodingSessionLease _lease({
  CodingSessionTarget? forTarget,
  String state = 'live',
  int leaseSequence = 1,
  int createdAt = 1400,
  String commandId = 'cmd-1',
  String? id,
}) => decodeCodingSessionLease(
  leaseEvent(
    forTarget: forTarget,
    state: state,
    leaseSequence: leaseSequence,
    createdAt: createdAt,
    commandId: commandId,
    id: id,
  ),
).value!;

void main() {
  group('reachability', () {
    final now = DateTime.fromMillisecondsSinceEpoch(2000 * 1000, isUtc: true);

    test(
      'a live lease younger than 150 s proves the provider is reachable',
      () {
        final verdict = deriveCodingSessionReachability(
          leases: [_lease(createdAt: 1900)],
          currentTarget: target(),
          acceptedCommandId: 'cmd-1',
          now: now,
        );
        expect(verdict.kind, CodingSessionReachabilityKind.reachable);
        expect(verdict.leaseAge, const Duration(seconds: 100));
      },
    );

    test('a live lease older than 150 s proves nothing', () {
      final verdict = deriveCodingSessionReachability(
        leases: [_lease(createdAt: 1849)],
        currentTarget: target(),
        acceptedCommandId: 'cmd-1',
        now: now,
      );
      expect(verdict.kind, CodingSessionReachabilityKind.noProviderAnswering);
      expect(verdict.leaseAge, const Duration(seconds: 151));
    });

    test('the highest sequence decides, even when an older one is live', () {
      final verdict = deriveCodingSessionReachability(
        leases: [
          _lease(leaseSequence: 1, createdAt: 1990),
          _lease(leaseSequence: 2, state: 'released', createdAt: 1995),
        ],
        currentTarget: target(),
        acceptedCommandId: 'cmd-1',
        now: now,
      );
      expect(verdict.kind, CodingSessionReachabilityKind.noProviderAnswering);
      expect(verdict.leaseSequence, 2);
    });

    // The lease's commandId matches on purpose: with a different one the
    // command gate short-circuits and D8's current-generation filter is
    // pinned nowhere. Only the generation mismatch is under test here.
    test('a lease for another generation does not answer for this one', () {
      final verdict = deriveCodingSessionReachability(
        leases: [_lease(forTarget: target(generation: 1), createdAt: 1990)],
        currentTarget: target(generation: 2),
        acceptedCommandId: 'cmd-1',
        now: now,
      );
      expect(verdict.kind, CodingSessionReachabilityKind.noProviderAnswering);
    });

    test('two distinct leases at one sequence read unknown, not a denial', () {
      final verdict = deriveCodingSessionReachability(
        leases: [
          _lease(leaseSequence: 3, createdAt: 1990, id: '0' * 63 + 'a'),
          _lease(leaseSequence: 3, createdAt: 1991, id: '0' * 63 + 'b'),
        ],
        currentTarget: target(),
        acceptedCommandId: 'cmd-1',
        now: now,
      );
      expect(verdict.kind, CodingSessionReachabilityKind.unknown);
      expect(verdict.conflict, isTrue);
    });

    test('a lease minted by a command nobody accepted proves nothing', () {
      final verdict = deriveCodingSessionReachability(
        leases: [
          _lease(commandId: 'a-command-that-minted-nothing', createdAt: 1990),
        ],
        currentTarget: target(),
        acceptedCommandId: 'cmd-1',
        now: now,
      );
      expect(verdict.kind, CodingSessionReachabilityKind.noProviderAnswering);
    });

    test('an unread lease query is unknown, never "nobody answering"', () {
      final verdict = deriveCodingSessionReachability(
        leases: const [],
        currentTarget: target(),
        acceptedCommandId: 'cmd-1',
        now: now,
        leasesRead: false,
      );
      expect(verdict.kind, CodingSessionReachabilityKind.unknown);
    });
  });

  group('reachability across a session', () {
    final now = DateTime.fromMillisecondsSinceEpoch(2000 * 1000, isUtc: true);
    final second = target(sessionId: 'session-2');

    // D8: "an unknown read (no lease query yet / partial) must never render
    // as 'nobody answering'". A session whose executions disagree — one
    // undetermined, one with nothing answering — is a partial read, and the
    // header prints "No provider answering" in the error colour. Unknown has
    // to be sticky, or the page states as fact something it did not read.
    test('one undetermined execution keeps the whole session unknown', () {
      final events = <NostrEvent>[
        // Execution one: a lease tie, which reads unknown on its own.
        createEvent(commandId: 'cmd-1', sessionRef: sessionRefA),
        receiptEvent(commandId: 'cmd-1', status: 'created'),
        metadataEvent(status: 'running', sessionRef: sessionRefA),
        leaseEvent(leaseSequence: 3, createdAt: 1990, id: 'a' * 64),
        leaseEvent(leaseSequence: 3, createdAt: 1991, id: 'b' * 64),
        // Execution two: no lease at all, which reads "nobody answering".
        createEvent(commandId: 'cmd-2', sessionRef: sessionRefA),
        receiptEvent(commandId: 'cmd-2', status: 'created', forTarget: second),
        metadataEvent(
          forTarget: second,
          status: 'running',
          sessionRef: sessionRefA,
        ),
      ];
      final view = readCodingSessionChannel(
        channelId: channelId,
        verifier: null,
        events: events,
      );
      final session = view.sessions.single;
      expect(session.executions, hasLength(2));
      final verdict = view.reachabilityFor(session, now: now);
      expect(verdict.kind, CodingSessionReachabilityKind.unknown);
      expect(verdict.conflict, isTrue);
      expect(verdict.leaseSequence, 3);
    });

    // A live lease still wins: one execution proving a provider is answering
    // is what the session is asking about.
    test('a reachable execution still answers for the session', () {
      final events = <NostrEvent>[
        createEvent(commandId: 'cmd-1', sessionRef: sessionRefA),
        receiptEvent(commandId: 'cmd-1', status: 'created'),
        metadataEvent(status: 'running', sessionRef: sessionRefA),
        leaseEvent(leaseSequence: 3, createdAt: 1990, id: 'a' * 64),
        leaseEvent(leaseSequence: 3, createdAt: 1991, id: 'b' * 64),
        createEvent(commandId: 'cmd-2', sessionRef: sessionRefA),
        receiptEvent(commandId: 'cmd-2', status: 'created', forTarget: second),
        metadataEvent(
          forTarget: second,
          status: 'running',
          sessionRef: sessionRefA,
        ),
        leaseEvent(forTarget: second, commandId: 'cmd-2', createdAt: 1990),
      ];
      final view = readCodingSessionChannel(
        channelId: channelId,
        verifier: null,
        events: events,
      );
      final verdict = view.reachabilityFor(view.sessions.single, now: now);
      expect(verdict.kind, CodingSessionReachabilityKind.reachable);
    });

    // When every current generation says the same thing, that is the answer.
    test('every execution silent reads as nobody answering', () {
      final events = <NostrEvent>[
        createEvent(commandId: 'cmd-1', sessionRef: sessionRefA),
        receiptEvent(commandId: 'cmd-1', status: 'created'),
        metadataEvent(status: 'running', sessionRef: sessionRefA),
        createEvent(commandId: 'cmd-2', sessionRef: sessionRefA),
        receiptEvent(commandId: 'cmd-2', status: 'created', forTarget: second),
        metadataEvent(
          forTarget: second,
          status: 'running',
          sessionRef: sessionRefA,
        ),
      ];
      final view = readCodingSessionChannel(
        channelId: channelId,
        verifier: null,
        events: events,
      );
      final verdict = view.reachabilityFor(view.sessions.single, now: now);
      expect(verdict.kind, CodingSessionReachabilityKind.noProviderAnswering);
    });
  });
}
