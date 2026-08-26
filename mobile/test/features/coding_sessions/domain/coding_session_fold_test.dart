import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:flutter_test/flutter_test.dart';

import 'coding_session_fixtures.dart';

CodingSessionMetadata _metadata({
  CodingSessionTarget? forTarget,
  String status = 'running',
  int createdAt = 1000,
  String? id,
  String? sessionRef,
  String? title,
  String pubkey = providerPubkey,
}) => decodeCodingSessionMetadata(
  metadataEvent(
    forTarget: forTarget,
    status: status,
    createdAt: createdAt,
    id: id,
    sessionRef: sessionRef,
    title: title,
    pubkey: pubkey,
  ),
).value!;

CodingSessionReceipt _receipt({
  required String commandId,
  required String status,
  CodingSessionTarget? forTarget,
  int createdAt = 900,
  String pubkey = providerPubkey,
  Map<String, Object?>? error,
  String? turnId,
}) => decodeCodingSessionReceipt(
  receiptEvent(
    commandId: commandId,
    status: status,
    forTarget: forTarget,
    createdAt: createdAt,
    pubkey: pubkey,
    error: error,
    turnId: turnId,
  ),
).value!;

CodingSessionCreate _create({
  required String commandId,
  String? sessionRef,
  String? genesisRef,
  String pubkey = founderPubkey,
  int createdAt = 800,
}) => decodeCodingSessionCreate(
  createEvent(
    commandId: commandId,
    sessionRef: sessionRef,
    genesisRef: genesisRef,
    pubkey: pubkey,
    createdAt: createdAt,
  ),
).value!;

CodingSessionLease _lease({
  CodingSessionTarget? forTarget,
  String state = 'live',
  int leaseSequence = 1,
  int createdAt = 1400,
  String? id,
}) => decodeCodingSessionLease(
  leaseEvent(
    forTarget: forTarget,
    state: state,
    leaseSequence: leaseSequence,
    createdAt: createdAt,
    id: id,
  ),
).value!;

void main() {
  group('generation resolution', () {
    test('a turn receipt never creates a generation', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [
          _receipt(
            commandId: 'cmd-1',
            status: 'turn_refused',
            error: {'code': 'STALE_GENERATION', 'message': 'stale'},
          ),
          _receipt(commandId: 'cmd-1', status: 'turn_queued'),
          _receipt(
            commandId: 'cmd-1',
            status: 'turn_started',
            turnId: 'turn-1',
          ),
        ],
        metadata: [_metadata()],
      );
      expect(executions, isEmpty);
    });

    test('each generation-creating status brings one into existence', () {
      for (final status in [
        'created',
        'created_with_failed_initial_turn',
        'resumed',
        'resumed_without_context',
      ]) {
        final executions = resolveCodingSessionGenerations(
          receipts: [
            _receipt(
              commandId: 'cmd-1',
              status: status,
              error: switch (status) {
                'created_with_failed_initial_turn' => {
                  'code': 'INITIAL_TURN_FAILED',
                  'message': 'no',
                },
                'resumed_without_context' => {
                  'code': 'CONTEXT_NOT_RECOVERED',
                  'message': 'lost',
                },
                _ => null,
              },
            ),
          ],
          metadata: const [],
        );
        expect(executions, hasLength(1), reason: status);
      }
    });

    test('the newest metadata wins, and a tie goes to the lower event id', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [_receipt(commandId: 'cmd-1', status: 'created')],
        metadata: [
          _metadata(status: 'running', createdAt: 1000, id: '0' * 63 + '1'),
          _metadata(status: 'idle', createdAt: 2000, id: 'f' * 64),
          _metadata(status: 'failed', createdAt: 2000, id: '0' * 63 + '2'),
        ],
      );
      expect(executions.single.status, CodingSessionStatus.failed);
      expect(executions.single.statusAt, 2000);
      expect(executions.single.statusConflict, isTrue);
    });

    test('a stopped receipt newer than the metadata stops the execution', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [
          _receipt(commandId: 'cmd-1', status: 'created'),
          _receipt(commandId: 'cmd-2', status: 'stopped', createdAt: 3000),
        ],
        metadata: [_metadata(status: 'running', createdAt: 2000)],
      );
      expect(executions.single.status, CodingSessionStatus.stopped);
    });

    test('resume mints a new generation and only the newest is current', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [
          _receipt(commandId: 'cmd-1', status: 'created'),
          _receipt(
            commandId: 'cmd-2',
            status: 'resumed',
            forTarget: target(generation: 2),
          ),
        ],
        metadata: const [],
      );
      expect(executions, hasLength(2));
      final current = executions.where(
        (execution) => execution.isCurrentGeneration,
      );
      expect(current.single.target.generation, 2);
    });
  });

  group('umbrella grouping', () {
    test('executions sharing a sessionRef form one session', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [
          _receipt(commandId: 'cmd-1', status: 'created'),
          _receipt(
            commandId: 'cmd-2',
            status: 'created',
            forTarget: target(sessionId: 'session-2'),
          ),
        ],
        metadata: [
          _metadata(sessionRef: sessionRefA),
          _metadata(
            forTarget: target(sessionId: 'session-2'),
            sessionRef: sessionRefA,
          ),
        ],
      );
      final sessions = groupCodingSessionUmbrellas(executions: executions);
      expect(sessions, hasLength(1));
      expect(sessions.single.executions, hasLength(2));
    });

    test('a record with no sessionRef is its own session', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [
          _receipt(commandId: 'cmd-1', status: 'created'),
          _receipt(
            commandId: 'cmd-2',
            status: 'created',
            forTarget: target(sessionId: 'session-2'),
          ),
        ],
        metadata: const [],
      );
      final sessions = groupCodingSessionUmbrellas(executions: executions);
      expect(sessions, hasLength(2));
      expect(sessions.every((session) => session.sessionRef == null), isTrue);
    });

    test('generations of one stream stay one implicit session', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [
          _receipt(commandId: 'cmd-1', status: 'created'),
          _receipt(
            commandId: 'cmd-2',
            status: 'resumed',
            forTarget: target(generation: 2),
          ),
        ],
        metadata: const [],
      );
      final sessions = groupCodingSessionUmbrellas(executions: executions);
      expect(sessions, hasLength(1));
      expect(sessions.single.executions, hasLength(2));
    });

    test('the newest name and closure win', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [_receipt(commandId: 'cmd-1', status: 'created')],
        metadata: [_metadata(sessionRef: sessionRefA, title: 'Fallback title')],
      );
      final sessions = groupCodingSessionUmbrellas(
        executions: executions,
        names: [
          decodeCodingSessionName(
            nameEvent(content: 'Old name', createdAt: 1000),
          ).value!,
          decodeCodingSessionName(
            nameEvent(content: 'New name', createdAt: 2000),
          ).value!,
        ],
        closures: [decodeCodingSessionClosure(closureEvent()).value!],
      );
      expect(sessions.single.name, 'New name');
      expect(sessions.single.displayName, 'New name');
      expect(sessions.single.closed, isTrue);
    });

    test('display name falls back to the metadata title', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [_receipt(commandId: 'cmd-1', status: 'created')],
        metadata: [_metadata(sessionRef: sessionRefA, title: 'Fallback title')],
      );
      final sessions = groupCodingSessionUmbrellas(executions: executions);
      expect(sessions.single.displayName, 'Fallback title');
    });
  });

  group('founder resolution', () {
    test('a genesis-naming create resolves the founder by event id', () {
      final founder = resolveCodingSessionFounder(
        creates: [
          _create(
            commandId: 'cmd-1',
            sessionRef: sessionRefA,
            genesisRef: genesisEventIdA,
          ),
        ],
        genesesByEventId: {
          genesisEventIdA: decodeCodingSessionGenesis(
            genesisEvent(eventId: genesisEventIdA),
          ).value!,
        },
      );
      expect(founder.resolution, CodingSessionFounderResolution.genesis);
      expect(founder.pubkey, founderPubkey);
    });

    test('two distinct genesisRefs are a conflict with no founder', () {
      final founder = resolveCodingSessionFounder(
        creates: [
          _create(
            commandId: 'cmd-1',
            sessionRef: sessionRefA,
            genesisRef: genesisEventIdA,
          ),
          _create(
            commandId: 'cmd-2',
            sessionRef: sessionRefA,
            genesisRef: genesisEventIdB,
          ),
        ],
        genesesByEventId: {
          genesisEventIdA: decodeCodingSessionGenesis(
            genesisEvent(eventId: genesisEventIdA),
          ).value!,
          genesisEventIdB: decodeCodingSessionGenesis(
            genesisEvent(eventId: genesisEventIdB, pubkey: otherFounderPubkey),
          ).value!,
        },
      );
      expect(founder.resolution, CodingSessionFounderResolution.conflict);
      expect(founder.pubkey, isNull);
    });

    test('no genesisRef falls back to the earliest create signer', () {
      final founder = resolveCodingSessionFounder(
        creates: [
          _create(
            commandId: 'cmd-2',
            sessionRef: sessionRefA,
            pubkey: otherFounderPubkey,
            createdAt: 900,
          ),
          _create(
            commandId: 'cmd-1',
            sessionRef: sessionRefA,
            pubkey: founderPubkey,
            createdAt: 800,
          ),
        ],
      );
      expect(founder.resolution, CodingSessionFounderResolution.legacy);
      expect(founder.pubkey, founderPubkey);
    });

    test('no readable create leaves the founder unresolved', () {
      expect(
        resolveCodingSessionFounder(creates: const []).resolution,
        CodingSessionFounderResolution.unresolved,
      );
    });

    test('a named genesis nobody published stays unresolved', () {
      final founder = resolveCodingSessionFounder(
        creates: [
          _create(
            commandId: 'cmd-1',
            sessionRef: sessionRefA,
            genesisRef: genesisEventIdA,
          ),
        ],
      );
      expect(founder.resolution, CodingSessionFounderResolution.unresolved);
    });
  });

  group('umbrella status fold', () {
    CodingSessionExecution execution({
      required CodingSessionStatus status,
      int lastActivityAt = 1000,
      String sessionId = 'session-1',
    }) => CodingSessionExecution(
      channelId: channelId,
      target: target(sessionId: sessionId),
      authority: const CodingSessionAuthority(
        pubkey: providerPubkey,
        verified: true,
      ),
      status: status,
      statusAt: lastActivityAt,
      metadata: null,
      sessionRef: sessionRefA,
      isCurrentGeneration: true,
      statusConflict: false,
      lastActivityAt: lastActivityAt,
      commandId: 'cmd-1',
    );

    test('any running execution makes the session Working', () {
      expect(
        foldCodingSessionUmbrellaStatus([
          execution(status: CodingSessionStatus.stopped),
          execution(status: CodingSessionStatus.running, sessionId: 's2'),
        ]).kind,
        CodingSessionFoldedStatusKind.working,
      );
    });

    test('waiting wins when nothing is running', () {
      expect(
        foldCodingSessionUmbrellaStatus([
          execution(status: CodingSessionStatus.idle),
          execution(
            status: CodingSessionStatus.waitingForInput,
            sessionId: 's2',
          ),
        ]).kind,
        CodingSessionFoldedStatusKind.waiting,
      );
    });

    test('Ended only when every execution is stopped', () {
      expect(
        foldCodingSessionUmbrellaStatus([
          execution(status: CodingSessionStatus.stopped),
          execution(status: CodingSessionStatus.stopped, sessionId: 's2'),
        ]).kind,
        CodingSessionFoldedStatusKind.ended,
      );
      expect(
        foldCodingSessionUmbrellaStatus([
          execution(status: CodingSessionStatus.stopped),
          execution(status: CodingSessionStatus.idle, sessionId: 's2'),
        ]).kind,
        isNot(CodingSessionFoldedStatusKind.ended),
      );
    });

    test('otherwise the most recently active non-stopped status stands', () {
      final folded = foldCodingSessionUmbrellaStatus([
        execution(status: CodingSessionStatus.idle, lastActivityAt: 1000),
        execution(
          status: CodingSessionStatus.disconnected,
          lastActivityAt: 2000,
          sessionId: 's2',
        ),
        execution(
          status: CodingSessionStatus.stopped,
          lastActivityAt: 3000,
          sessionId: 's3',
        ),
      ]);
      expect(folded.kind, CodingSessionFoldedStatusKind.reported);
      expect(folded.status, CodingSessionStatus.disconnected);
    });
  });

  group('reachability', () {
    final now = DateTime.fromMillisecondsSinceEpoch(2000 * 1000, isUtc: true);

    test(
      'a live lease younger than 150 s proves the provider is reachable',
      () {
        final verdict = deriveCodingSessionReachability(
          leases: [_lease(createdAt: 1900)],
          currentTarget: target(),
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
        now: now,
      );
      expect(verdict.kind, CodingSessionReachabilityKind.noProviderAnswering);
      expect(verdict.leaseSequence, 2);
    });

    test('a lease for another generation does not answer for this one', () {
      final verdict = deriveCodingSessionReachability(
        leases: [_lease(forTarget: target(generation: 1), createdAt: 1990)],
        currentTarget: target(generation: 2),
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
        now: now,
      );
      expect(verdict.kind, CodingSessionReachabilityKind.unknown);
      expect(verdict.conflict, isTrue);
    });

    test('an unread lease query is unknown, never "nobody answering"', () {
      final verdict = deriveCodingSessionReachability(
        leases: const [],
        currentTarget: target(),
        now: now,
        leasesRead: false,
      );
      expect(verdict.kind, CodingSessionReachabilityKind.unknown);
    });
  });
}
