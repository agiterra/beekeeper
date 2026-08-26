import 'dart:io';

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
  group('source hygiene', () {
    // A raw NUL byte in a source file makes git treat the file as binary:
    // no reviewable diff and no three-way merge on the rebase onto main. The
    // escape is byte-identical at runtime, so there is no reason to keep one.
    test('no domain source file carries a raw NUL byte', () {
      for (final path in const [
        'lib/features/coding_sessions/domain/coding_session_fold.dart',
        'lib/features/coding_sessions/domain/coding_session_transcript.dart',
        'lib/features/coding_sessions/domain/coding_session_transcript_item.dart',
      ]) {
        expect(
          File(path).readAsBytesSync().contains(0),
          isFalse,
          reason: '$path contains a raw NUL byte; write it as the escape',
        );
      }
    });
  });

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

    test('a resumed generation whose metadata lags stays in its session', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [
          _receipt(commandId: 'cmd-1', status: 'created'),
          _receipt(
            commandId: 'cmd-2',
            status: 'resumed',
            forTarget: target(generation: 2),
            createdAt: 1500,
          ),
        ],
        // Only generation 1 has echoed the umbrella claim so far.
        metadata: [_metadata(sessionRef: sessionRefA)],
      );
      final sessions = groupCodingSessionUmbrellas(
        executions: executions,
        names: [decodeCodingSessionName(nameEvent(content: 'Ship it')).value!],
        closures: [decodeCodingSessionClosure(closureEvent()).value!],
        genesesByEventId: {
          genesisEventIdA: decodeCodingSessionGenesis(
            genesisEvent(eventId: genesisEventIdA),
          ).value!,
        },
      );
      expect(sessions, hasLength(1));
      expect(sessions.single.sessionRef, sessionRefA);
      expect(sessions.single.executions, hasLength(2));
      expect(sessions.single.name, 'Ship it');
      expect(sessions.single.closed, isTrue);
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
        genesesByEventId: {
          genesisEventIdA: decodeCodingSessionGenesis(
            genesisEvent(eventId: genesisEventIdA),
          ).value!,
        },
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

  group('closure authority', () {
    List<CodingSessionExecution> closableExecutions() =>
        resolveCodingSessionGenerations(
          receipts: [_receipt(commandId: 'cmd-1', status: 'created')],
          metadata: [_metadata(sessionRef: sessionRefA)],
        );

    Map<String, CodingSessionGenesis> genesisA({
      String sessionRef = sessionRefA,
    }) => {
      genesisEventIdA: decodeCodingSessionGenesis(
        genesisEvent(eventId: genesisEventIdA, sessionRef: sessionRef),
      ).value!,
    };

    test('the founder of the named genesis can close the session', () {
      final sessions = groupCodingSessionUmbrellas(
        executions: closableExecutions(),
        closures: [decodeCodingSessionClosure(closureEvent()).value!],
        genesesByEventId: genesisA(),
      );
      expect(sessions.single.closed, isTrue);
    });

    test('a close signed by anyone but the founder is refused', () {
      final sessions = groupCodingSessionUmbrellas(
        executions: closableExecutions(),
        closures: [
          decodeCodingSessionClosure(
            closureEvent(pubkey: otherFounderPubkey),
          ).value!,
        ],
        genesesByEventId: genesisA(),
      );
      expect(sessions.single.closed, isFalse);
    });

    test('a close whose genesis is unreadable is refused', () {
      final sessions = groupCodingSessionUmbrellas(
        executions: closableExecutions(),
        closures: [decodeCodingSessionClosure(closureEvent()).value!],
      );
      expect(sessions.single.closed, isFalse);
    });

    test('a close anchored to another session\'s genesis is refused', () {
      final sessions = groupCodingSessionUmbrellas(
        executions: closableExecutions(),
        closures: [decodeCodingSessionClosure(closureEvent()).value!],
        genesesByEventId: genesisA(sessionRef: sessionRefB),
      );
      expect(sessions.single.closed, isFalse);
    });

    test('a newer reopen from any member reopens the session', () {
      final sessions = groupCodingSessionUmbrellas(
        executions: closableExecutions(),
        closures: [
          decodeCodingSessionClosure(closureEvent(createdAt: 1300)).value!,
          decodeCodingSessionClosure(
            closureEvent(
              closed: false,
              pubkey: otherFounderPubkey,
              createdAt: 1400,
            ),
          ).value!,
        ],
        genesesByEventId: genesisA(),
      );
      expect(sessions.single.closed, isFalse);
    });

    test('a refused close never outranks an older authorized one', () {
      final sessions = groupCodingSessionUmbrellas(
        executions: closableExecutions(),
        closures: [
          decodeCodingSessionClosure(closureEvent(createdAt: 1300)).value!,
          // A stranger publishing `open` after the founder closed cannot
          // reopen through a genesis this observer cannot resolve.
          decodeCodingSessionClosure(
            closureEvent(
              closed: false,
              genesisRef: genesisEventIdB,
              pubkey: otherFounderPubkey,
              createdAt: 1400,
            ),
          ).value!,
        ],
        genesesByEventId: genesisA(),
      );
      expect(sessions.single.closed, isTrue);
    });
  });

  group('founder resolution', () {
    test('a genesis-naming create resolves the founder by event id', () {
      final founder = resolveCodingSessionFounder(
        sessionRef: sessionRefA,
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
        sessionRef: sessionRefA,
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
        sessionRef: sessionRefA,
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
        resolveCodingSessionFounder(
          sessionRef: sessionRefA,
          creates: const [],
        ).resolution,
        CodingSessionFounderResolution.unresolved,
      );
    });

    test('a genesis anchoring another session never founds this one', () {
      final founder = resolveCodingSessionFounder(
        sessionRef: sessionRefA,
        creates: [
          _create(
            commandId: 'cmd-1',
            sessionRef: sessionRefA,
            genesisRef: genesisEventIdB,
          ),
        ],
        genesesByEventId: {
          genesisEventIdB: decodeCodingSessionGenesis(
            genesisEvent(
              eventId: genesisEventIdB,
              sessionRef: sessionRefB,
              pubkey: otherFounderPubkey,
            ),
          ).value!,
        },
      );
      expect(founder.resolution, CodingSessionFounderResolution.unresolved);
      expect(
        founder.pubkey,
        isNull,
        reason: 'the named genesis founded a different session',
      );
    });

    test('a named genesis nobody published stays unresolved', () {
      final founder = resolveCodingSessionFounder(
        sessionRef: sessionRefA,
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

  group('receipt-joined creates', () {
    test('an unanswered create is not founder evidence', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [_receipt(commandId: 'cmd-1', status: 'created')],
        metadata: [_metadata(sessionRef: sessionRefA)],
        creates: [_create(commandId: 'cmd-1', sessionRef: sessionRefA)],
      );
      final sessions = groupCodingSessionUmbrellas(
        executions: executions,
        creates: [
          _create(commandId: 'cmd-1', sessionRef: sessionRefA),
          // Never answered by a receipt: it minted no execution here, so it
          // says nothing about who founded this session.
          _create(
            commandId: 'cmd-99',
            sessionRef: sessionRefA,
            genesisRef: genesisEventIdA,
            pubkey: otherFounderPubkey,
          ),
        ],
        genesesByEventId: {
          genesisEventIdA: decodeCodingSessionGenesis(
            genesisEvent(eventId: genesisEventIdA, pubkey: otherFounderPubkey),
          ).value!,
        },
        targetKeyByCommandId: {'cmd-1': target().key},
      );
      expect(
        sessions.single.founder.resolution,
        CodingSessionFounderResolution.legacy,
      );
      expect(sessions.single.founder.pubkey, founderPubkey);
    });

    test('a create joined to another execution stays out of this one', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [_receipt(commandId: 'cmd-1', status: 'created')],
        metadata: [_metadata(sessionRef: sessionRefA)],
      );
      final sessions = groupCodingSessionUmbrellas(
        executions: executions,
        creates: [
          _create(
            commandId: 'cmd-2',
            sessionRef: sessionRefA,
            pubkey: otherFounderPubkey,
          ),
        ],
        targetKeyByCommandId: {
          'cmd-2': target(sessionId: 'somewhere-else').key,
        },
      );
      expect(
        sessions.single.founder.resolution,
        CodingSessionFounderResolution.unresolved,
      );
    });
  });

  group('channel read', () {
    test('a stranger\'s unanswered create cannot dispute the founder', () {
      final view = readCodingSessionChannel(
        channelId: channelId,
        verifier: null,
        events: [
          genesisEvent(eventId: genesisEventIdA),
          createEvent(
            commandId: 'cmd-1',
            sessionRef: sessionRefA,
            genesisRef: genesisEventIdA,
          ),
          receiptEvent(commandId: 'cmd-1', status: 'created'),
          metadataEvent(status: 'running', sessionRef: sessionRefA),
          // A create nobody answered, naming a genesis nobody else names.
          genesisEvent(
            eventId: genesisEventIdB,
            sessionRef: sessionRefA,
            pubkey: otherFounderPubkey,
          ),
          createEvent(
            commandId: 'cmd-99',
            sessionRef: sessionRefA,
            genesisRef: genesisEventIdB,
            pubkey: otherFounderPubkey,
          ),
        ],
      );
      expect(view.sessions, hasLength(1));
      expect(
        view.sessions.single.founder.resolution,
        CodingSessionFounderResolution.genesis,
      );
      expect(view.sessions.single.founder.pubkey, founderPubkey);
    });
  });

  group('generation-aware status', () {
    test(
      'a stopped resume ends the session the old generation left running',
      () {
        final executions = resolveCodingSessionGenerations(
          receipts: [
            _receipt(commandId: 'cmd-1', status: 'created'),
            _receipt(
              commandId: 'cmd-2',
              status: 'resumed',
              forTarget: target(generation: 2),
              createdAt: 1500,
            ),
          ],
          metadata: [
            _metadata(sessionRef: sessionRefA, status: 'running'),
            _metadata(
              forTarget: target(generation: 2),
              sessionRef: sessionRefA,
              status: 'stopped',
              createdAt: 1600,
            ),
          ],
        );
        final sessions = groupCodingSessionUmbrellas(executions: executions);
        expect(sessions.single.executions, hasLength(2));
        expect(
          sessions.single.status.kind,
          CodingSessionFoldedStatusKind.ended,
          reason: 'generation 1 was superseded, not still working',
        );
      },
    );
  });

  group('umbrella status fold', () {
    CodingSessionExecution execution({
      required CodingSessionStatus status,
      int lastActivityAt = 1000,
      String sessionId = 'session-1',
      int generation = 1,
      bool isCurrentGeneration = true,
    }) => CodingSessionExecution(
      channelId: channelId,
      target: target(sessionId: sessionId, generation: generation),
      authority: const CodingSessionAuthority(
        pubkey: providerPubkey,
        verified: true,
      ),
      status: status,
      statusAt: lastActivityAt,
      metadata: null,
      sessionRef: sessionRefA,
      isCurrentGeneration: isCurrentGeneration,
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

    test('a superseded generation never keeps a session Working', () {
      final folded = foldCodingSessionUmbrellaStatus([
        // Generation 1 was still "running" when it was superseded; the read
        // never sees a stop for it because generation 2 took over.
        execution(
          status: CodingSessionStatus.running,
          lastActivityAt: 1000,
          isCurrentGeneration: false,
        ),
        execution(
          status: CodingSessionStatus.stopped,
          lastActivityAt: 2000,
          generation: 2,
        ),
      ]);
      expect(folded.kind, CodingSessionFoldedStatusKind.ended);
    });

    test('a superseded generation does not speak for a live one', () {
      final folded = foldCodingSessionUmbrellaStatus([
        execution(
          status: CodingSessionStatus.waitingForInput,
          lastActivityAt: 1000,
          isCurrentGeneration: false,
        ),
        execution(
          status: CodingSessionStatus.idle,
          lastActivityAt: 2000,
          generation: 2,
        ),
      ]);
      expect(folded.kind, CodingSessionFoldedStatusKind.reported);
      expect(folded.status, CodingSessionStatus.idle);
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
