import 'dart:convert';
import 'dart:io';

import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

/// The mobile projection binds to `conformance/transcript-prose-join/
/// fixtures/vectors.json` (NIP-CST amendment 3, Join key; CONTRACT.md). Every
/// vector's envelopes go through the real 44225 decoder and the real
/// projector; every expected message — `assistant_text` and `reasoning` —
/// must come out as exactly one row with the same first and last event, the
/// same attribution, the joined text (bounded as one message), and the same
/// `arriving` verdict from the vector's `sessionStatus[]` and `sessionLease[]`.
void main() {
  final vectors =
      jsonDecode(
            File(
              '../conformance/transcript-prose-join/fixtures/vectors.json',
            ).readAsStringSync(),
          )
          as Map<String, dynamic>;

  test('vectors carry the expected schema', () {
    expect(vectors['schema'], 'buzz.conformance/transcript-prose-join@1');
    expect((vectors['vectors'] as List<dynamic>).isNotEmpty, isTrue);
  });

  for (final raw in vectors['vectors'] as List<dynamic>) {
    final vector = raw as Map<String, dynamic>;
    test(vector['name'] as String, () {
      final envelopes = [
        for (final input in vector['input'] as List<dynamic>)
          _decode(input as Map<String, dynamic>),
      ];
      final blocks = projectCodingSessionTranscript(
        envelopes,
        livenessByStream: _liveness(vector),
      );
      final rows = {
        for (final block in blocks)
          for (final item in block.items)
            if (item.type == CodingSessionItemType.message &&
                    item.role == CodingSessionItemRole.assistant ||
                item.type == CodingSessionItemType.thought)
              item.eventId: item,
      };
      final expected = vector['expectedMessages'] as List<dynamic>;
      expect(
        rows.length,
        expected.length,
        reason: 'one row per expected message, no more',
      );
      for (final rawMessage in expected) {
        final message = rawMessage as Map<String, dynamic>;
        final first = message['firstEventId'] as String;
        final row = rows[first];
        expect(row, isNotNull, reason: 'a row keyed on $first');
        final kind = message['kind'] as String;
        expect(
          row!.type,
          kind == 'reasoning'
              ? CodingSessionItemType.thought
              : CodingSessionItemType.message,
        );
        expect(row.signerPubkey, message['signer']);
        expect(row.target, CodingSessionTarget.decode(message['target']));
        expect(row.turnId, message['turnId']);
        expect(row.parentToolId, message['parentToolId']);
        expect(row.lastEventId, message['lastEventId']);
        expect(row.text, _bounded(message['text'] as String));
        expect(row.arriving, message['arriving'], reason: 'arriving of $first');
        final subagent = message['parentToolId'] != null;
        expect(row.title, switch ((kind == 'reasoning', subagent)) {
          (true, true) => codingSessionSubagentReasoningTitle,
          (true, false) => 'Reasoning',
          (false, true) => codingSessionSubagentResponseTitle,
          (false, false) => 'Response',
        });
      }
    });
  }

  test('the joined text is bounded as one message, not piece by piece', () {
    final vector = (vectors['vectors'] as List<dynamic>)
        .cast<Map<String, dynamic>>()
        .singleWhere((v) => v['name'] == 'fence-straddles-size-cut');
    final joined =
        ((vector['expectedMessages'] as List<dynamic>).single
                as Map<String, dynamic>)['text']
            as String;
    expect(joined.length, greaterThan(codingSessionTranscriptMaxTextChars));
    final blocks = projectCodingSessionTranscript([
      for (final input in vector['input'] as List<dynamic>)
        _decode(input as Map<String, dynamic>),
    ]);
    final row = blocks.single.items.singleWhere(
      (item) => item.role == CodingSessionItemRole.assistant,
    );
    // The cut is disclosed, and what precedes it is the joined message's own
    // opening — every piece's text, in order, never re-bounded per piece.
    expect(row.text.endsWith('…'), isTrue);
    expect(
      joined.startsWith(row.text.substring(0, row.text.length - 1)),
      isTrue,
    );
  });

  test('a target with no liveness entry never shows anything arriving', () {
    final vector = (vectors['vectors'] as List<dynamic>)
        .cast<Map<String, dynamic>>()
        .singleWhere((v) => v['name'] == 'open-turn-arriving');
    final blocks = projectCodingSessionTranscript([
      for (final input in vector['input'] as List<dynamic>)
        _decode(input as Map<String, dynamic>),
    ]);
    expect(blocks.single.items.where((item) => item.arriving), isEmpty);
  });
}

CodingSessionTranscriptEnvelope _decode(Map<String, dynamic> input) {
  final content = input['content'] as Map<String, dynamic>;
  final target = CodingSessionTarget.decode(content['session'])!;
  final eventSeq = content['eventSeq'] as int;
  final event = NostrEvent(
    id: input['eventId'] as String,
    pubkey: input['signer'] as String,
    createdAt: 1785512978,
    kind: EventKind.codingSessionTranscript,
    tags: [
      ['h', 'channel-1'],
      ['cst-v', 'cst1-1'],
      ['cs-target', target.key],
      ['cst-seq', '$eventSeq'],
      ['cst-key', target.transcriptSemanticKey(eventSeq)],
    ],
    content: jsonEncode(content),
    sig: '0' * 128,
  );
  final decoded = decodeCodingSessionTranscript(event);
  expect(decoded.value, isNotNull, reason: 'vector envelope decodes');
  return decoded.value!;
}

/// The vector's reader-side facts, keyed the way the projector reads them.
/// Only targets with a lease entry get one: a target with none has no lease
/// this reader holds (CONTRACT, `sessionLease[]`).
Map<String, CodingSessionProseLiveness> _liveness(Map<String, dynamic> vector) {
  String key(Map<String, dynamic> entry) => codingSessionProseStreamKey(
    entry['signer'] as String,
    CodingSessionTarget.decode(entry['target'])!,
  );
  final statuses = {
    for (final raw in vector['sessionStatus'] as List<dynamic>)
      key(raw as Map<String, dynamic>): CodingSessionStatus.fromWire(
        raw['status'],
      ),
  };
  return {
    for (final raw in vector['sessionLease'] as List<dynamic>)
      key(raw as Map<String, dynamic>): CodingSessionProseLiveness(
        status: statuses[key(raw)],
        leaseLive: raw['lease'] == 'live',
      ),
  };
}

String _bounded(String text) =>
    text.length <= codingSessionTranscriptMaxTextChars
    ? text
    : '${text.substring(0, codingSessionTranscriptMaxTextChars)}…';
