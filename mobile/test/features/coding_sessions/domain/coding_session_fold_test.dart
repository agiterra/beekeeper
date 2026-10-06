import 'dart:convert';
import 'dart:io';

import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/shared/relay/nostr_models.dart';
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
  String? id,
}) => decodeCodingSessionReceipt(
  receiptEvent(
    commandId: commandId,
    status: status,
    forTarget: forTarget,
    createdAt: createdAt,
    pubkey: pubkey,
    error: error,
    turnId: turnId,
    id: id,
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

/// A 44223 written out field by field so a test can vary any one of them.
///
/// The shared fixture exposes no knob for the reference/summary fields or the
/// B1 code coordinates, and those are exactly the ones D6's "distinct
/// payloads" rule has to cover, so the payload is written out here.
CodingSessionMetadata _metadataVariant({
  required String id,
  String? projectRef,
  Map<String, Object?> extra = const {},
  int createdAt = 1000,
}) {
  final resolved = target();
  return decodeCodingSessionMetadata(
    event(
      kind: EventKind.codingSessionMetadata,
      createdAt: createdAt,
      id: id,
      tags: [
        ['h', channelId],
        ['csm-v', 'csm1-1'],
        ['cs-target', resolved.key],
        ['csm-key', resolved.metadataSemanticKey],
      ],
      content: jsonEncode({
        'schema': 'buzz-coding-session-metadata/v1',
        'session': resolved.toJson(),
        'projectRef': projectRef,
        'repoRef': null,
        'title': null,
        'agentRef': null,
        'provider': 'buzz-session-provider',
        'runtime': 'claude-agent-acp',
        'model': 'opus',
        'status': 'running',
        'branch': null,
        'capabilities': const {
          'threadTurnStart': true,
          'threadTurnInterrupt': true,
          'threadSteer': false,
          'context': true,
          'diff': false,
          'plan': false,
        },
        ...extra,
      }),
    ),
  ).value!;
}

void main() {
  group('source hygiene', () {
    // A raw NUL byte in a source file makes git treat the file as binary:
    // no reviewable diff and no three-way merge on the rebase onto main. The
    // escape is byte-identical at runtime, so there is no reason to keep one.
    test('no domain source file carries a raw NUL byte', () {
      for (final path in const [
        'lib/features/coding_sessions/domain/coding_session_fold.dart',
        'lib/features/coding_sessions/domain/coding_session_pending_turn.dart',
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

    // D6: a conflict is counted over the whole payload, not over the handful
    // of fields the header happens to print. Two same-second events differing
    // anywhere are distinct payloads; two byte-equal ones are not a conflict
    // at all, and accusing the provider of one would be an invented fact.
    test(
      'same-second metadata differing outside the header fields conflicts',
      () {
        final executions = resolveCodingSessionGenerations(
          receipts: [_receipt(commandId: 'cmd-1', status: 'created')],
          metadata: [
            _metadataVariant(projectRef: 'project-a', id: '0' * 63 + '1'),
            _metadataVariant(projectRef: 'project-b', id: '0' * 63 + '2'),
          ],
        );
        expect(executions.single.statusConflict, isTrue);
      },
    );

    // The decoder validates the B1 code coordinates and then drops them: this
    // observer surfaces no commit or dirty flag in v1. Dropped is not the
    // same as irrelevant — two providers disagreeing about which commit the
    // session is on is a disagreement, and a conflict test built from a
    // hand-written list of modelled fields cannot see it.
    test(
      'same-second metadata differing only in a dropped field conflicts',
      () {
        final executions = resolveCodingSessionGenerations(
          receipts: [_receipt(commandId: 'cmd-1', status: 'created')],
          metadata: [
            _metadataVariant(
              id: '0' * 63 + '3',
              extra: {
                'observedCommit': 'a' * 40,
                'dirty': false,
                'relayReachable': true,
                'verifiedAt': 1700000000,
              },
            ),
            _metadataVariant(
              id: '0' * 63 + '4',
              extra: {
                'observedCommit': 'b' * 40,
                'dirty': false,
                'relayReachable': true,
                'verifiedAt': 1700000000,
              },
            ),
          ],
        );
        expect(executions.single.statusConflict, isTrue);
      },
    );

    test('two byte-equal metadata events in one second are not a conflict', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [_receipt(commandId: 'cmd-1', status: 'created')],
        metadata: [
          _metadataVariant(projectRef: 'project-a', id: '0' * 63 + '1'),
          _metadataVariant(projectRef: 'project-a', id: '0' * 63 + '2'),
        ],
      );
      expect(executions.single.statusConflict, isFalse);
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
      final founded = foundedByFounder();
      final sessions = groupCodingSessionUmbrellas(
        executions: executions,
        creates: founded.creates,
        targetKeyByCommandId: founded.targetKeyByCommandId,
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
      final founded = foundedByFounder();
      final sessions = groupCodingSessionUmbrellas(
        executions: executions,
        creates: founded.creates,
        targetKeyByCommandId: founded.targetKeyByCommandId,
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

    test(
      'an unnamed, untitled session is "Untitled session", not its driver',
      () {
        final executions = resolveCodingSessionGenerations(
          receipts: [_receipt(commandId: 'cmd-1', status: 'created')],
          metadata: const [],
        );
        final sessions = groupCodingSessionUmbrellas(executions: executions);
        expect(sessions.single.displayName, 'Untitled session');
        expect(
          sessions.single.resolvedName.origin,
          CodingSessionNameOrigin.fallback,
        );
        expect(
          sessions.single.executions.single.label,
          isNot('Untitled session'),
        );
      },
    );
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
      // The stray genesis founds a session of its own — B, never started —
      // and the close, keyed to A, closes neither.
      expect(sessions, hasLength(2));
      final a = sessions.singleWhere((s) => s.sessionRef == sessionRefA);
      final b = sessions.singleWhere((s) => s.sessionRef == sessionRefB);
      expect(a.closed, isFalse);
      expect(a.isFounded, isFalse);
      expect(b.closed, isFalse);
      expect(b.isFounded, isTrue);
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

    // D8, read through the seam production uses. The three gates below are
    // properties of `CodingSessionChannelView.reachabilityFor`, not of the
    // pure function: they fail if the view stops passing the accepted command
    // id, the authority pubkey, or the current-generation filter.
    test('a lease no accepted command backs does not prove reachability', () {
      CodingSessionReachability read(String leaseCommandId) {
        final view = readCodingSessionChannel(
          channelId: channelId,
          verifier: null,
          events: [
            createEvent(commandId: 'cmd-1', sessionRef: sessionRefA),
            receiptEvent(commandId: 'cmd-1', status: 'created'),
            metadataEvent(status: 'running', sessionRef: sessionRefA),
            leaseEvent(commandId: leaseCommandId, createdAt: 2000),
          ],
        );
        return view.reachabilityFor(
          view.sessions.single,
          now: DateTime.fromMillisecondsSinceEpoch(2010 * 1000, isUtc: true),
        );
      }

      // Control: the lease the accepted create minted does prove it.
      expect(read('cmd-1').kind, CodingSessionReachabilityKind.reachable);
      expect(
        read('a-command-that-minted-nothing').kind,
        CodingSessionReachabilityKind.noProviderAnswering,
      );
    });

    test('a lease from a signer no create named proves nothing', () {
      final view = readCodingSessionChannel(
        channelId: channelId,
        verifier: null,
        events: [
          createEvent(commandId: 'cmd-1', sessionRef: sessionRefA),
          receiptEvent(commandId: 'cmd-1', status: 'created'),
          metadataEvent(status: 'running', sessionRef: sessionRefA),
          leaseEvent(
            commandId: 'cmd-1',
            pubkey: otherProviderPubkey,
            createdAt: 2000,
          ),
        ],
      );
      expect(
        view
            .reachabilityFor(
              view.sessions.single,
              now: DateTime.fromMillisecondsSinceEpoch(
                2010 * 1000,
                isUtc: true,
              ),
            )
            .kind,
        CodingSessionReachabilityKind.noProviderAnswering,
      );
    });

    test('a superseded generation\'s live lease answers for nothing', () {
      final view = readCodingSessionChannel(
        channelId: channelId,
        verifier: null,
        events: [
          createEvent(commandId: 'cmd-1', sessionRef: sessionRefA),
          receiptEvent(commandId: 'cmd-1', status: 'created'),
          metadataEvent(status: 'running', sessionRef: sessionRefA),
          resumeEvent(commandId: 'cmd-2'),
          receiptEvent(
            commandId: 'cmd-2',
            status: 'resumed',
            forTarget: target(generation: 2),
            createdAt: 1900,
          ),
          metadataEvent(
            forTarget: target(generation: 2),
            status: 'running',
            sessionRef: sessionRefA,
            createdAt: 1950,
          ),
          // Live, in date, signed by the authority — but for generation 1.
          leaseEvent(commandId: 'cmd-1', createdAt: 2000),
        ],
      );
      expect(view.sessions.single.executions, hasLength(2));
      expect(
        view
            .reachabilityFor(
              view.sessions.single,
              now: DateTime.fromMillisecondsSinceEpoch(
                2010 * 1000,
                isUtc: true,
              ),
            )
            .kind,
        CodingSessionReachabilityKind.noProviderAnswering,
      );
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
}
