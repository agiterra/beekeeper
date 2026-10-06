import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:flutter_test/flutter_test.dart';

import '../ui/fake_observer.dart';

const _otherProvider =
    '2222222233333333444444445555555566666666777777778888888899999999';

CodingSessionPendingCreate _pending({bool published = true}) =>
    CodingSessionPendingCreate(
      channelId: testChannelId,
      commandId: 'csl-1',
      sessionRef: 'umbrella-new',
      genesisRef: 'ab' * 32,
      title: 'Fix the gate',
      providerAuthorityPubkey: testSignerPubkey,
      recordedAt: 5000,
      published: published,
    );

void main() {
  group('settleCodingSessionPendingCreate', () {
    test('an unanswered create is sending, then awaiting the provider', () {
      expect(
        settleCodingSessionPendingCreate(
          _pending(published: false),
          receipts: const [],
          sessions: const [],
        ).kind,
        CodingSessionPendingCreateKind.sending,
      );
      expect(
        settleCodingSessionPendingCreate(
          _pending(),
          receipts: const [],
          sessions: const [],
        ).kind,
        CodingSessionPendingCreateKind.awaitingProvider,
      );
    });

    test('a created receipt by commandId settles it', () {
      for (final status in [
        CodingSessionReceiptStatus.created,
        CodingSessionReceiptStatus.createdWithFailedInitialTurn,
      ]) {
        final phase = settleCodingSessionPendingCreate(
          _pending(),
          receipts: [testLifecycleReceipt(commandId: 'csl-1', status: status)],
          sessions: const [],
        );
        expect(phase.kind, CodingSessionPendingCreateKind.created);
        expect(phase.isSettled, isTrue);
      }
    });

    test('a failed receipt carries the provider\'s own refusal', () {
      final phase = settleCodingSessionPendingCreate(
        _pending(),
        receipts: [
          testLifecycleReceipt(
            commandId: 'csl-1',
            status: CodingSessionReceiptStatus.failed,
            code: 'PROJECT_CWD_UNRESOLVED',
            message: 'no working directory is configured for project x',
          ),
        ],
        sessions: const [],
      );
      expect(phase.kind, CodingSessionPendingCreateKind.failed);
      expect(phase.error?.code, 'PROJECT_CWD_UNRESOLVED');
      expect(phase.error?.message, contains('no working directory'));
    });

    test('only the addressed provider\'s receipt counts, and only by '
        'commandId', () {
      final phase = settleCodingSessionPendingCreate(
        _pending(),
        receipts: [
          testLifecycleReceipt(
            commandId: 'csl-1',
            status: CodingSessionReceiptStatus.failed,
            signerPubkey: _otherProvider,
            code: 'PROVIDER_UNAVAILABLE',
          ),
          testLifecycleReceipt(
            commandId: 'csl-other',
            status: CodingSessionReceiptStatus.created,
          ),
        ],
        sessions: const [],
      );
      expect(phase.kind, CodingSessionPendingCreateKind.awaitingProvider);
    });

    test(
      'an umbrella claiming the sessionRef settles it without a receipt',
      () {
        final phase = settleCodingSessionPendingCreate(
          _pending(),
          receipts: const [],
          sessions: [testUmbrella(key: 'k', sessionRef: 'umbrella-new')],
        );
        expect(phase.kind, CodingSessionPendingCreateKind.created);
      },
    );

    test('the founded umbrella this device\'s own genesis folds into does '
        'not settle the create it is still waiting on', () {
      final phase = settleCodingSessionPendingCreate(
        _pending(),
        receipts: const [],
        sessions: [
          testUmbrella(
            key: 'umbrella-new',
            sessionRef: 'umbrella-new',
            executions: const [],
            status: const CodingSessionFoldedStatus(
              kind: CodingSessionFoldedStatusKind.founded,
            ),
          ),
        ],
      );
      expect(phase.kind, CodingSessionPendingCreateKind.awaitingProvider);
    });
  });

  test('the store key is the channel and the command', () {
    expect(_pending().key, '$testChannelId csl-1');
    expect(_pending().copyWith(published: true).published, isTrue);
    expect(_pending().copyWith(genesisRef: 'cd' * 32).genesisRef, 'cd' * 32);
  });
}
