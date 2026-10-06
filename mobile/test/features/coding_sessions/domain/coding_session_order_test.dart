import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:flutter_test/flutter_test.dart';

import 'coding_session_fixtures.dart';

/// Order-independence pins for the fold.
///
/// Every rule here decides between two facts that arrive together, and each
/// one is asserted in both arrival orders. Relay-arrival order is not a signed
/// fact — history pages and live deliveries land in whatever order the relay
/// sends them — so any rule that reads it lets two devices holding the same
/// events show different things.
CodingSessionReceipt _receipt({
  required String commandId,
  required String status,
  int createdAt = 900,
  String? id,
}) => decodeCodingSessionReceipt(
  receiptEvent(
    commandId: commandId,
    status: status,
    createdAt: createdAt,
    id: id,
  ),
).value!;

CodingSessionMetadata _metadata({
  String status = 'running',
  int createdAt = 1000,
  String? sessionRef,
}) => decodeCodingSessionMetadata(
  metadataEvent(status: status, createdAt: createdAt, sessionRef: sessionRef),
).value!;

void main() {
  setUp(resetEventIds);

  group('generation resolution', () {
    // The command a generation is filed under decides which 24223 leases
    // answer for it (D8), so it must be a function of the signed facts alone.
    // Relay-arrival order is not: history pages and live deliveries land in
    // whatever order the relay sends them, and two devices holding the same
    // events would otherwise disagree about whether a provider is answering.
    test(
      'the earliest generation receipt names the command, in any read order',
      () {
        final earlier = _receipt(
          commandId: 'cmd-earlier',
          status: 'created',
          createdAt: 900,
        );
        final later = _receipt(
          commandId: 'cmd-later',
          status: 'resumed',
          createdAt: 1500,
        );

        for (final order in [
          [earlier, later],
          [later, earlier],
        ]) {
          final executions = resolveCodingSessionGenerations(
            receipts: order,
            metadata: [_metadata(status: 'running', createdAt: 2000)],
          );
          expect(executions.single.commandId, 'cmd-earlier');
        }
      },
    );

    test('generation receipts sharing a second break the tie by event id', () {
      final lowerId = _receipt(
        commandId: 'cmd-lower-id',
        status: 'created',
        createdAt: 900,
        id: '0' * 63 + '1',
      );
      final higherId = _receipt(
        commandId: 'cmd-higher-id',
        status: 'resumed',
        createdAt: 900,
        id: 'f' * 64,
      );

      for (final order in [
        [lowerId, higherId],
        [higherId, lowerId],
      ]) {
        final executions = resolveCodingSessionGenerations(
          receipts: order,
          metadata: [_metadata(status: 'running', createdAt: 2000)],
        );
        expect(executions.single.commandId, 'cmd-lower-id');
      }
    });
  });

  group('session names', () {
    // The tie-break is the desktop's (codingSessionName.ts:129-135,
    // codingSessionClosure.ts): same second, *higher* event id wins — the
    // opposite of the metadata rule, and pinned in both directions so a
    // reversed comparison cannot pass by accident.
    test('names sharing a second break the tie to the higher event id', () {
      final executions = resolveCodingSessionGenerations(
        receipts: [_receipt(commandId: 'cmd-1', status: 'created')],
        metadata: [_metadata(sessionRef: sessionRefA)],
      );
      final lower = decodeCodingSessionName(
        nameEvent(content: 'Lower id', createdAt: 2000, id: '0' * 63 + '1'),
      ).value!;
      final higher = decodeCodingSessionName(
        nameEvent(content: 'Higher id', createdAt: 2000, id: 'f' * 64),
      ).value!;

      for (final order in [
        [lower, higher],
        [higher, lower],
      ]) {
        final founded = foundedByFounder();
        final sessions = groupCodingSessionUmbrellas(
          executions: executions,
          creates: founded.creates,
          targetKeyByCommandId: founded.targetKeyByCommandId,
          names: order,
        );
        expect(sessions.single.name, 'Higher id');
      }
    });
  });
}
