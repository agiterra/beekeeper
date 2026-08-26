import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
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

    test('a lease for another generation does not answer for this one', () {
      final verdict = deriveCodingSessionReachability(
        leases: [_lease(forTarget: target(generation: 1), createdAt: 1990)],
        currentTarget: target(generation: 2),
        acceptedCommandId: 'cmd-2',
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
}
