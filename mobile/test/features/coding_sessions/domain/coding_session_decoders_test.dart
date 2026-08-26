import 'dart:convert';

import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nostr/nostr.dart' as nostr;

import 'coding_session_fixtures.dart';

NostrEvent _withTags(NostrEvent source, List<List<String>> tags) => NostrEvent(
  id: source.id,
  pubkey: source.pubkey,
  createdAt: source.createdAt,
  kind: source.kind,
  tags: tags,
  content: source.content,
  sig: source.sig,
);

NostrEvent _withContent(NostrEvent source, String content) => NostrEvent(
  id: source.id,
  pubkey: source.pubkey,
  createdAt: source.createdAt,
  kind: source.kind,
  tags: source.tags,
  content: content,
  sig: source.sig,
);

void main() {
  group('44223 metadata', () {
    test('decodes the exact payload and reads statusAt off the event', () {
      final decoded = decodeCodingSessionMetadata(
        metadataEvent(status: 'waiting_for_input', createdAt: 1234),
      );
      final metadata = decoded.value!;
      expect(metadata.status, CodingSessionStatus.waitingForInput);
      expect(metadata.statusAt, 1234);
      expect(metadata.runtime, 'claude-agent-acp');
      expect(metadata.model, 'opus');
      expect(metadata.capabilities['threadTurnStart'], isTrue);
    });

    test('rejects an unknown status rather than coercing it', () {
      final source = metadataEvent();
      final payload = jsonDecode(source.content) as Map<String, dynamic>
        ..['status'] = 'busy';
      final decoded = decodeCodingSessionMetadata(
        _withContent(source, jsonEncode(payload)),
      );
      expect(decoded.value, isNull);
      expect(decoded.reason, CodingSessionDecodeReason.malformedPayload);
    });

    test('rejects an unexpected key', () {
      final source = metadataEvent();
      final payload = jsonDecode(source.content) as Map<String, dynamic>
        ..['surprise'] = 1;
      final decoded = decodeCodingSessionMetadata(
        _withContent(source, jsonEncode(payload)),
      );
      expect(decoded.reason, CodingSessionDecodeReason.malformedPayload);
    });

    test('rejects a partial B1 fact subset but accepts all four', () {
      final source = metadataEvent();
      final partial = jsonDecode(source.content) as Map<String, dynamic>
        ..['observedCommit'] = 'abc';
      expect(
        decodeCodingSessionMetadata(
          _withContent(source, jsonEncode(partial)),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
      final full = jsonDecode(source.content) as Map<String, dynamic>
        ..['observedCommit'] = 'abc'
        ..['dirty'] = false
        ..['relayReachable'] = true
        ..['verifiedAt'] = 1700000000;
      expect(
        decodeCodingSessionMetadata(
          _withContent(source, jsonEncode(full)),
        ).isValid,
        isTrue,
      );
    });

    test('rejects a cs-target tag that disagrees with the payload', () {
      final source = metadataEvent();
      final decoded = decodeCodingSessionMetadata(
        _withTags(source, [
          ['h', channelId],
          ['csm-v', 'csm1-1'],
          ['cs-target', target(generation: 9).key],
          ['csm-key', target().metadataSemanticKey],
        ]),
      );
      expect(decoded.reason, CodingSessionDecodeReason.badTags);
    });

    test('rejects tags in the wrong order', () {
      final source = metadataEvent();
      final decoded = decodeCodingSessionMetadata(
        _withTags(source, [
          ['csm-v', 'csm1-1'],
          ['h', channelId],
          ['cs-target', target().key],
          ['csm-key', target().metadataSemanticKey],
        ]),
      );
      expect(decoded.reason, CodingSessionDecodeReason.badTags);
    });

    test('rejects content past 32 KiB', () {
      final source = metadataEvent(title: 'x' * (33 * 1024));
      expect(
        decodeCodingSessionMetadata(source).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
    });
  });

  group('44224 receipt', () {
    test('decodes a create and marks it not a turn stage', () {
      final decoded = decodeCodingSessionReceipt(
        receiptEvent(commandId: 'cmd-1', status: 'created'),
      );
      expect(decoded.value!.status, CodingSessionReceiptStatus.created);
      expect(decoded.value!.isTurnStage, isFalse);
      expect(decoded.value!.session, target());
    });

    test('every turn status decodes as a turn stage', () {
      for (final status in [
        'turn_queued',
        'turn_started',
        'turn_degraded',
        'turn_dropped',
        'turn_refused',
        'interrupt_delivered',
      ]) {
        final decoded = decodeCodingSessionReceipt(
          receiptEvent(
            commandId: 'cmd-1',
            status: status,
            turnId: status == 'turn_started' ? 'turn-1' : null,
            error: status == 'turn_dropped' || status == 'turn_refused'
                ? {'code': 'QUEUE_FULL', 'message': 'full'}
                : null,
          ),
        );
        expect(decoded.value, isNotNull, reason: status);
        expect(decoded.value!.isTurnStage, isTrue, reason: status);
        expect(
          decoded.value!.status.createsGeneration,
          isFalse,
          reason: status,
        );
      }
    });

    test('turnId is accepted only on turn_started', () {
      expect(
        decodeCodingSessionReceipt(
          receiptEvent(
            commandId: 'cmd-1',
            status: 'turn_started',
            turnId: 'turn-1',
          ),
        ).value!.turnId,
        'turn-1',
      );
      expect(
        decodeCodingSessionReceipt(
          receiptEvent(commandId: 'cmd-1', status: 'created', turnId: 'turn-1'),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
      expect(
        decodeCodingSessionReceipt(
          receiptEvent(commandId: 'cmd-1', status: 'turn_started'),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
    });

    test('failed carries no session and requires an error', () {
      expect(
        decodeCodingSessionReceipt(
          receiptEvent(
            commandId: 'cmd-1',
            status: 'failed',
            error: {'code': 'NO_CAPACITY', 'message': 'busy'},
          ),
        ).value!.session,
        isNull,
      );
      expect(
        decodeCodingSessionReceipt(
          receiptEvent(commandId: 'cmd-1', status: 'failed'),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
    });

    test('resumed_without_context pins its error code', () {
      expect(
        decodeCodingSessionReceipt(
          receiptEvent(
            commandId: 'cmd-1',
            status: 'resumed_without_context',
            error: {'code': 'CONTEXT_NOT_RECOVERED', 'message': 'lost'},
          ),
        ).isValid,
        isTrue,
      );
      expect(
        decodeCodingSessionReceipt(
          receiptEvent(
            commandId: 'cmd-1',
            status: 'resumed_without_context',
            error: {'code': 'SOMETHING_ELSE', 'message': 'lost'},
          ),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
    });

    test('a turn receipt whose csl-key omits its stage is rejected', () {
      final source = receiptEvent(commandId: 'cmd-1', status: 'turn_queued');
      final decoded = decodeCodingSessionReceipt(
        _withTags(source, [
          ['h', channelId],
          ['cslr-v', 'cslr1-1'],
          ['csl-command', 'cmd-1'],
          [
            'csl-key',
            codingSessionReceiptSemanticKey(
              'cmd-1',
              CodingSessionReceiptStatus.created,
            ),
          ],
        ]),
      );
      expect(decoded.reason, CodingSessionDecodeReason.badTags);
    });
  });

  group('44225 transcript envelope', () {
    test('decodes the six-key envelope', () {
      final decoded = decodeCodingSessionTranscript(
        transcriptEvent(
          eventSeq: 3,
          turnId: 'turn-1',
          item: {'kind': 'assistant_text', 'text': 'hello'},
        ),
      );
      expect(decoded.value!.eventSeq, 3);
      expect(decoded.value!.turnId, 'turn-1');
      expect(decoded.value!.itemKind, 'assistant_text');
    });

    test('rejects a non-positive eventSeq and a kindless item', () {
      expect(
        decodeCodingSessionTranscript(
          transcriptEvent(eventSeq: 0, item: {'kind': 'assistant_text'}),
        ).isValid,
        isFalse,
      );
      expect(
        decodeCodingSessionTranscript(
          transcriptEvent(eventSeq: 1, item: {'text': 'no kind'}),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
    });

    test('rejects an item nested deeper than 24', () {
      Map<String, Object?> nest(int depth) =>
          depth == 0 ? {'leaf': 1} : {'next': nest(depth - 1)};
      final decoded = decodeCodingSessionTranscript(
        transcriptEvent(
          eventSeq: 1,
          item: {'kind': 'assistant_text', 'deep': nest(40)},
        ),
      );
      expect(decoded.reason, CodingSessionDecodeReason.malformedPayload);
    });

    test('rejects a cst-seq tag that disagrees with the payload', () {
      final source = transcriptEvent(
        eventSeq: 4,
        item: {'kind': 'assistant_text'},
      );
      final decoded = decodeCodingSessionTranscript(
        _withTags(source, [
          ['h', channelId],
          ['cst-v', 'cst1-1'],
          ['cs-target', target().key],
          ['cst-seq', '5'],
          ['cst-key', target().transcriptSemanticKey(4)],
        ]),
      );
      expect(decoded.reason, CodingSessionDecodeReason.badTags);
    });
  });

  group('24223 lease', () {
    test('decodes the exact ordered tags and four-key content', () {
      final decoded = decodeCodingSessionLease(
        leaseEvent(leaseSequence: 7, commandId: 'cmd-9'),
      );
      expect(decoded.value!.state, CodingSessionLeaseState.live);
      expect(decoded.value!.leaseSequence, 7);
      expect(decoded.value!.commandId, 'cmd-9');
    });

    test('rejects a cslease-seq tag that disagrees with the payload', () {
      final source = leaseEvent(leaseSequence: 7);
      final decoded = decodeCodingSessionLease(
        _withTags(source, [
          ['h', channelId],
          ['cslease-v', 'cslease1-1'],
          ['cs-target', target().key],
          ['csl-command', 'command-1'],
          ['cslease-seq', '8'],
        ]),
      );
      expect(decoded.reason, CodingSessionDecodeReason.badTags);
    });

    test('rejects an unknown state', () {
      final source = leaseEvent();
      final payload = jsonDecode(source.content) as Map<String, dynamic>
        ..['state'] = 'maybe';
      expect(
        decodeCodingSessionLease(
          _withContent(source, jsonEncode(payload)),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
    });
  });

  group('session kinds', () {
    test('a create names its provider authority', () {
      final decoded = decodeCodingSessionCreate(
        createEvent(
          commandId: 'cmd-1',
          sessionRef: sessionRefA,
          genesisRef: genesisEventIdA,
        ),
      );
      expect(decoded.value!.providerAuthorityPubkey, providerPubkey);
      expect(decoded.value!.sessionRef, sessionRefA);
      expect(decoded.value!.genesisRef, genesisEventIdA);
    });

    test('a genesisRef without a sessionRef is malformed', () {
      final source = createEvent(commandId: 'cmd-1');
      final payload = jsonDecode(source.content) as Map<String, dynamic>;
      (payload['action']! as Map<String, dynamic>)['genesisRef'] =
          genesisEventIdA;
      expect(
        decodeCodingSessionCreate(
          _withContent(source, jsonEncode(payload)),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
    });

    test('a resume is wrong-kind for the create reader, not malformed', () {
      final source = createEvent(commandId: 'cmd-1');
      final payload = jsonDecode(source.content) as Map<String, dynamic>;
      payload['action'] = {
        'type': 'session.resume',
        'session': target().toJson(),
        'providerAuthorityPubkey': providerPubkey,
      };
      expect(
        decodeCodingSessionCreate(
          _withContent(source, jsonEncode(payload)),
        ).reason,
        CodingSessionDecodeReason.wrongKind,
      );
    });

    test('genesis, name, goal and closure decode with their own envelopes', () {
      expect(
        decodeCodingSessionGenesis(
          genesisEvent(eventId: genesisEventIdA),
        ).value!.founderPubkey,
        founderPubkey,
      );
      expect(
        decodeCodingSessionName(nameEvent(content: 'Refactor')).value!.content,
        'Refactor',
      );
      expect(
        decodeCodingSessionGoal(goalEvent(content: 'Ship it')).value!.content,
        'Ship it',
      );
      expect(decodeCodingSessionClosure(closureEvent()).value!.closed, isTrue);
      expect(
        decodeCodingSessionClosure(closureEvent(closed: false)).value!.closed,
        isFalse,
      );
    });

    test('a multi-line name is rejected', () {
      expect(
        decodeCodingSessionName(nameEvent(content: 'one\ntwo')).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
    });

    test('a name past 256 bytes is rejected', () {
      expect(
        decodeCodingSessionName(nameEvent(content: 'x' * 257)).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
    });
  });

  group('signature verification', () {
    test('the nostr package verifies a genuinely signed event', () {
      const secretKey =
          '5ee1c8000ab28edd64d74a7d951ac2dd559814887b1b9e1ac7c5f89e96125c12';
      final signed = nostr.Event.from(
        kind: EventKind.codingSessionName,
        content: 'Signed name',
        secretKey: secretKey,
        createdAt: 1700000000,
        tags: [
          ['h', channelId],
          ['d', sessionRefA],
          ['csnm-v', 'csnm1-1'],
        ],
      );
      final inbound = NostrEvent(
        id: signed.id,
        pubkey: signed.pubkey,
        createdAt: signed.createdAt,
        kind: signed.kind,
        tags: signed.tags,
        content: signed.content,
        sig: signed.sig,
      );
      const verifier = NostrPackageSignatureVerifier();
      expect(verifier.available, isTrue);
      expect(verifier.verify(inbound), CodingSessionSignatureVerdict.valid);
      expect(
        decodeCodingSessionName(inbound, verifier: verifier).isValid,
        isTrue,
      );
    });

    test('a forged signature is rejected, never silently trusted', () {
      const verifier = NostrPackageSignatureVerifier();
      final forged = nameEvent(content: 'Forged');
      expect(verifier.verify(forged), CodingSessionSignatureVerdict.invalid);
      expect(
        decodeCodingSessionName(forged, verifier: verifier).reason,
        CodingSessionDecodeReason.badSignature,
      );
    });

    test('an unavailable verifier reports unavailable, not valid', () {
      const verifier = UnavailableSignatureVerifier();
      expect(verifier.available, isFalse);
      expect(
        verifier.verify(nameEvent(content: 'x')),
        CodingSessionSignatureVerdict.unavailable,
      );
    });
  });
}
