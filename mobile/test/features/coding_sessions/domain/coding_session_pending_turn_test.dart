import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/features/coding_sessions/ui/coding_session_labels.dart';
import 'package:flutter_test/flutter_test.dart';

const _signer =
    'aaaaaaaabbbbbbbbccccccccddddddddeeeeeeeeffffffff0000000011111111';
const _other =
    '11111111222222223333333344444444555555556666666677777777dddddddd';

const _target = CodingSessionTarget(
  driver: 'provider-a',
  instanceId: 'instance-1',
  sessionId: 'session-1',
  generation: 1,
);

CodingSessionPendingTurn _turn({bool published = true}) =>
    CodingSessionPendingTurn(
      channelId: 'channel-1',
      executionKey: _target.executionKey,
      generation: 1,
      commandId: 'csc-1',
      text: 'go',
      draft: 'go ',
      recordedAt: 1000,
      published: published,
    );

CodingSessionReceipt _receipt(
  CodingSessionReceiptStatus status, {
  String commandId = 'csc-1',
  String? code,
  int createdAt = 10,
  String eventId = 'r1',
}) => CodingSessionReceipt(
  ref: CodingSessionEventRef(
    channelId: 'channel-1',
    eventId: eventId,
    signerPubkey: _signer,
    createdAt: createdAt,
  ),
  commandId: commandId,
  status: status,
  session: _target,
  error: code == null
      ? null
      : CodingSessionReceiptError(code: code, message: 'because'),
);

CodingSessionUmbrella _session(CodingSessionFounder founder) =>
    CodingSessionUmbrella(
      channelId: 'channel-1',
      key: 'umbrella-1',
      sessionRef: 'umbrella-1',
      executions: const [],
      founder: founder,
      name: null,
      goal: null,
      closed: false,
      status: const CodingSessionFoldedStatus(
        kind: CodingSessionFoldedStatusKind.unknown,
      ),
      lastActivityAt: 0,
    );

void main() {
  group('settleCodingSessionPendingTurn', () {
    test('an unpublished row is sending; a published one is waiting', () {
      expect(
        settleCodingSessionPendingTurn(_turn(published: false), const []).phase,
        CodingSessionPendingPhase.sending,
      );
      expect(
        settleCodingSessionPendingTurn(_turn(), const []).phase,
        CodingSessionPendingPhase.published,
      );
    });

    test('settles on turn_started by commandId, never by text', () {
      final started = settleCodingSessionPendingTurn(_turn(), [
        _receipt(CodingSessionReceiptStatus.turnQueued),
        _receipt(
          CodingSessionReceiptStatus.turnStarted,
          createdAt: 11,
          eventId: 'r2',
        ),
      ]);
      expect(started.phase, CodingSessionPendingPhase.started);
      expect(started.settled, isTrue);

      // A started receipt for a different command settles nothing here.
      final foreign = settleCodingSessionPendingTurn(_turn(), [
        _receipt(CodingSessionReceiptStatus.turnStarted, commandId: 'csc-9'),
      ]);
      expect(foreign.phase, CodingSessionPendingPhase.published);
    });

    test('queued and degraded describe a turn still owed', () {
      expect(
        settleCodingSessionPendingTurn(_turn(), [
          _receipt(CodingSessionReceiptStatus.turnQueued),
        ]).phase,
        CodingSessionPendingPhase.queued,
      );
      final degraded = settleCodingSessionPendingTurn(_turn(), [
        _receipt(CodingSessionReceiptStatus.turnQueued),
        _receipt(
          CodingSessionReceiptStatus.turnDegraded,
          code: 'STEER_UNSUPPORTED',
          createdAt: 11,
          eventId: 'r2',
        ),
      ]);
      expect(degraded.phase, CodingSessionPendingPhase.degraded);
      expect(degraded.detail, 'STEER_UNSUPPORTED: because');
      expect(degraded.settled, isFalse);
    });

    test('an injected steer settles the row exactly as a start does', () {
      final injected = settleCodingSessionPendingTurn(_turn(), [
        _receipt(CodingSessionReceiptStatus.turnQueued),
        _receipt(
          CodingSessionReceiptStatus.turnInjected,
          createdAt: 11,
          eventId: 'r2',
        ),
      ]);
      expect(injected.phase, CodingSessionPendingPhase.injected);
      expect(injected.settled, isTrue);
      expect(injected.failed, isFalse);
      expect(
        codingSessionPendingPhaseLabel(injected),
        'Injected into the running turn',
      );
    });

    test('a delivery-unknown answer parks the row: neither settled nor failed, '
        'and reconciled only by a later definite receipt', () {
      final unknown = settleCodingSessionPendingTurn(_turn(), [
        _receipt(CodingSessionReceiptStatus.turnQueued),
        _receipt(
          CodingSessionReceiptStatus.turnDeliveryUnknown,
          code: 'STEER_ACK_LOST',
          createdAt: 11,
          eventId: 'r2',
        ),
      ]);
      expect(unknown.phase, CodingSessionPendingPhase.deliveryUnknown);
      expect(unknown.settled, isFalse);
      // Not failed: the words must not go back to the composer.
      expect(unknown.failed, isFalse);
      expect(unknown.deliveryUnknown, isTrue);
      expect(unknown.detail, 'STEER_ACK_LOST: because');
      expect(unknown.readdressGeneration, isNull);
      expect(
        codingSessionPendingPhaseLabel(unknown),
        'Delivery unknown — STEER_ACK_LOST: because',
      );
      // It outranks the queue and degrade receipts beside it.
      final beside = settleCodingSessionPendingTurn(_turn(), [
        _receipt(
          CodingSessionReceiptStatus.turnDeliveryUnknown,
          code: 'STEER_ACK_LOST',
        ),
        _receipt(
          CodingSessionReceiptStatus.turnDegraded,
          code: 'STEER_UNSUPPORTED',
          createdAt: 11,
          eventId: 'r2',
        ),
      ]);
      expect(beside.phase, CodingSessionPendingPhase.deliveryUnknown);
      // A late acknowledgement that proves the input never arrived is the
      // definite answer, and it is a drop — the sender sends again.
      final reconciledDrop = settleCodingSessionPendingTurn(_turn(), [
        _receipt(
          CodingSessionReceiptStatus.turnDeliveryUnknown,
          code: 'STEER_ACK_LOST',
        ),
        _receipt(
          CodingSessionReceiptStatus.turnDropped,
          code: 'STEER_NOT_DELIVERED',
          createdAt: 12,
          eventId: 'r3',
        ),
      ]);
      expect(reconciledDrop.phase, CodingSessionPendingPhase.dropped);
      expect(reconciledDrop.failed, isTrue);
      // ...and one that proves it landed settles the row.
      final reconciledInjected = settleCodingSessionPendingTurn(_turn(), [
        _receipt(
          CodingSessionReceiptStatus.turnDeliveryUnknown,
          code: 'STEER_ACK_LOST',
        ),
        _receipt(
          CodingSessionReceiptStatus.turnInjected,
          createdAt: 12,
          eventId: 'r3',
        ),
      ]);
      expect(reconciledInjected.phase, CodingSessionPendingPhase.injected);
      expect(reconciledInjected.settled, isTrue);
      // Whichever order the two are signed in, injected wins: the unknown is
      // the answer the provider has since replaced.
      final injectedFirst = settleCodingSessionPendingTurn(_turn(), [
        _receipt(
          CodingSessionReceiptStatus.turnDeliveryUnknown,
          code: 'STEER_ACK_LOST',
          createdAt: 12,
          eventId: 'r3',
        ),
        _receipt(CodingSessionReceiptStatus.turnInjected),
      ]);
      expect(injectedFirst.phase, CodingSessionPendingPhase.injected);
    });

    test('a refusal fails the row and offers readdress only when a newer '
        'generation exists', () {
      final stale = settleCodingSessionPendingTurn(_turn(), [
        _receipt(
          CodingSessionReceiptStatus.turnRefused,
          code: 'STALE_GENERATION',
        ),
      ], currentGeneration: 2);
      expect(stale.phase, CodingSessionPendingPhase.refused);
      expect(stale.failed, isTrue);
      expect(stale.readdressGeneration, 2);

      final sameGeneration = settleCodingSessionPendingTurn(_turn(), [
        _receipt(
          CodingSessionReceiptStatus.turnDropped,
          code: 'NO_LIVE_EXECUTION',
        ),
      ], currentGeneration: 1);
      expect(sameGeneration.phase, CodingSessionPendingPhase.dropped);
      expect(sameGeneration.readdressGeneration, isNull);

      final unauthorized = settleCodingSessionPendingTurn(_turn(), [
        _receipt(
          CodingSessionReceiptStatus.turnRefused,
          code: 'UNAUTHORIZED_OPERATOR',
        ),
      ], currentGeneration: 2);
      expect(unauthorized.readdressGeneration, isNull);
      expect(unauthorized.detail, 'UNAUTHORIZED_OPERATOR: because');
    });

    test('reads receipts in signed order, not arrival order', () {
      // A refusal that arrived first but was signed later still wins over
      // an earlier-signed queued receipt; a started receipt signed earliest
      // settles regardless of arrival.
      final view = settleCodingSessionPendingTurn(_turn(), [
        _receipt(
          CodingSessionReceiptStatus.turnRefused,
          code: 'QUEUE_FULL',
          createdAt: 12,
          eventId: 'late',
        ),
        _receipt(
          CodingSessionReceiptStatus.turnStarted,
          createdAt: 5,
          eventId: 'early',
        ),
      ]);
      expect(view.phase, CodingSessionPendingPhase.started);
    });
  });

  group('codingSessionSteerStanding', () {
    const genesisFounder = CodingSessionFounder(
      pubkey: _signer,
      resolution: CodingSessionFounderResolution.genesis,
      genesisRef: 'g',
    );

    test('the resolved founder may steer, case-insensitively', () {
      expect(
        codingSessionSteerStanding(
          session: _session(genesisFounder),
          myPubkey: _signer.toUpperCase(),
          relayAcceptedMine: false,
        ),
        CodingSessionSteerStanding.founder,
      );
    });

    test('no key means observe only', () {
      expect(
        codingSessionSteerStanding(
          session: _session(genesisFounder),
          myPubkey: null,
          relayAcceptedMine: true,
        ),
        CodingSessionSteerStanding.noKey,
      );
    });

    test('someone else\'s session reads not-founder until the relay accepts '
        'a command', () {
      expect(
        codingSessionSteerStanding(
          session: _session(genesisFounder),
          myPubkey: _other,
          relayAcceptedMine: false,
        ),
        CodingSessionSteerStanding.notFounder,
      );
      expect(
        codingSessionSteerStanding(
          session: _session(genesisFounder),
          myPubkey: _other,
          relayAcceptedMine: true,
        ),
        CodingSessionSteerStanding.acceptedOperator,
      );
    });

    test('an unresolved or disputed founder claims nothing', () {
      for (final founder in [
        CodingSessionFounder.unresolved,
        const CodingSessionFounder(
          pubkey: null,
          resolution: CodingSessionFounderResolution.conflict,
        ),
      ]) {
        expect(
          codingSessionSteerStanding(
            session: _session(founder),
            myPubkey: _signer,
            relayAcceptedMine: false,
          ),
          CodingSessionSteerStanding.founderUnresolved,
        );
      }
    });

    test('maySteer is exactly founder or accepted operator', () {
      for (final standing in CodingSessionSteerStanding.values) {
        expect(
          codingSessionMaySteer(standing),
          standing == CodingSessionSteerStanding.founder ||
              standing == CodingSessionSteerStanding.acceptedOperator,
          reason: '$standing',
        );
      }
    });
  });
}
