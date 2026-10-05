import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:flutter_test/flutter_test.dart';

import 'coding_session_fixtures.dart';

/// The founded fold: a 44226 genesis whose ref no execution claims is a
/// session in its own right — founded, never started — and reads as one.
///
/// Kept apart from `coding_session_fold_test.dart`, which is at the file-size
/// ceiling.
void main() {
  CodingSessionGenesis genesis({
    String eventId = genesisEventIdA,
    String sessionRef = sessionRefA,
    String pubkey = founderPubkey,
    int createdAt = 700,
  }) => decodeCodingSessionGenesis(
    genesisEvent(
      eventId: eventId,
      sessionRef: sessionRef,
      pubkey: pubkey,
      createdAt: createdAt,
    ),
  ).value!;

  group('founded sessions', () {
    test('a genesis nobody has started folds to a zero-execution umbrella '
        'whose founder is the genesis signer', () {
      final sessions = groupCodingSessionUmbrellas(
        executions: const [],
        genesesByEventId: {genesisEventIdA: genesis()},
        names: [
          decodeCodingSessionName(
            nameEvent(content: 'Keystone lead', createdAt: 900),
          ).value!,
        ],
        goals: [
          decodeCodingSessionGoal(
            goalEvent(content: 'Ship the founded row', createdAt: 1100),
          ).value!,
        ],
      );
      final session = sessions.single;
      expect(session.executions, isEmpty);
      expect(session.isFounded, isTrue);
      expect(session.key, sessionRefA);
      expect(session.sessionRef, sessionRefA);
      expect(session.channelId, channelId);
      expect(session.founder.pubkey, founderPubkey);
      expect(
        session.founder.resolution,
        CodingSessionFounderResolution.genesis,
      );
      expect(session.founder.genesisRef, genesisEventIdA);
      expect(session.name, 'Keystone lead');
      expect(session.displayName, 'Keystone lead');
      expect(session.goal, 'Ship the founded row');
      expect(session.closed, isFalse);
      expect(session.status.kind, CodingSessionFoldedStatusKind.founded);
      expect(session.status.status, isNull);
      // The newest founding fact, so the default date range keeps the row.
      expect(session.lastActivityAt, 1100);
    });

    test('a bare genesis is dated by the genesis itself', () {
      final sessions = groupCodingSessionUmbrellas(
        executions: const [],
        genesesByEventId: {genesisEventIdA: genesis(createdAt: 4321)},
      );
      expect(sessions.single.lastActivityAt, 4321);
      expect(sessions.single.name, isNull);
      expect(sessions.single.displayName, 'Untitled session');
    });

    test('a founded session can be closed by its founder', () {
      final sessions = groupCodingSessionUmbrellas(
        executions: const [],
        genesesByEventId: {genesisEventIdA: genesis()},
        closures: [
          decodeCodingSessionClosure(closureEvent(createdAt: 1300)).value!,
        ],
      );
      expect(sessions.single.closed, isTrue);
      expect(sessions.single.lastActivityAt, 1300);
      expect(
        sessions.single.status.kind,
        CodingSessionFoldedStatusKind.founded,
      );
    });

    test('two geneses for one ref are a dispute: no umbrella at all', () {
      final sessions = groupCodingSessionUmbrellas(
        executions: const [],
        genesesByEventId: {
          genesisEventIdA: genesis(),
          genesisEventIdB: genesis(
            eventId: genesisEventIdB,
            pubkey: otherFounderPubkey,
          ),
        },
      );
      expect(sessions, isEmpty);
    });

    test('two refs founded in one channel are two sessions', () {
      final sessions = groupCodingSessionUmbrellas(
        executions: const [],
        genesesByEventId: {
          genesisEventIdA: genesis(),
          genesisEventIdB: genesis(
            eventId: genesisEventIdB,
            sessionRef: sessionRefB,
            pubkey: otherFounderPubkey,
            createdAt: 800,
          ),
        },
      );
      expect(sessions.map((s) => s.key), [sessionRefB, sessionRefA]);
      expect(sessions.every((s) => s.isFounded), isTrue);
    });

    test('a receipt-joined create replaces the founded umbrella with the '
        'ordinary one', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [
          decodeCodingSessionReceipt(
            receiptEvent(commandId: 'cmd-1', status: 'created'),
          ).value!,
        ],
        metadata: [
          decodeCodingSessionMetadata(
            metadataEvent(sessionRef: sessionRefA),
          ).value!,
        ],
      );
      final create = decodeCodingSessionCreate(
        createEvent(
          commandId: 'cmd-1',
          sessionRef: sessionRefA,
          genesisRef: genesisEventIdA,
        ),
      ).value!;
      final sessions = groupCodingSessionUmbrellas(
        executions: executions,
        creates: [create],
        targetKeyByCommandId: {'cmd-1': target().key},
        genesesByEventId: {genesisEventIdA: genesis()},
      );
      final session = sessions.single;
      expect(session.isFounded, isFalse);
      expect(session.executions, hasLength(1));
      expect(session.sessionRef, sessionRefA);
      expect(session.founder.pubkey, founderPubkey);
      expect(
        session.founder.resolution,
        CodingSessionFounderResolution.genesis,
      );
      expect(session.status.kind, isNot(CodingSessionFoldedStatusKind.founded));
    });

    test('a genesis for a started ref adds no second row beside it', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [
          decodeCodingSessionReceipt(
            receiptEvent(commandId: 'cmd-1', status: 'created'),
          ).value!,
        ],
        metadata: [
          decodeCodingSessionMetadata(
            metadataEvent(sessionRef: sessionRefA),
          ).value!,
        ],
      );
      final sessions = groupCodingSessionUmbrellas(
        executions: executions,
        genesesByEventId: {
          genesisEventIdA: genesis(),
          genesisEventIdB: genesis(
            eventId: genesisEventIdB,
            sessionRef: sessionRefB,
          ),
        },
      );
      expect(sessions.map((s) => s.sessionRef), [sessionRefA, sessionRefB]);
      expect(sessions.first.isFounded, isFalse);
      expect(sessions.last.isFounded, isTrue);
    });
  });

  test('no executions fold to founded, never to unknown', () {
    final folded = foldCodingSessionUmbrellaStatus(const []);
    expect(folded.kind, CodingSessionFoldedStatusKind.founded);
    expect(folded.status, isNull);
  });
}
