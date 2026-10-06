import 'dart:convert';
import 'dart:io';

import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/shared/relay/nostr_models.dart';
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
  _promptImageAmendment();
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
        'continuation_registered',
        'turn_queued',
        'turn_started',
        'turn_injected',
        'turn_degraded',
        'turn_delivery_unknown',
        'turn_dropped',
        'turn_refused',
        'interrupt_delivered',
      ]) {
        final decoded = decodeCodingSessionReceipt(
          receiptEvent(
            commandId: 'cmd-1',
            status: status,
            turnId: status == 'turn_started' || status == 'turn_injected'
                ? 'turn-1'
                : null,
            error:
                status == 'turn_dropped' ||
                    status == 'turn_refused' ||
                    status == 'turn_delivery_unknown'
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

    test('turn_injected is the second six-key status; unknown must say why', () {
      final injected = decodeCodingSessionReceipt(
        receiptEvent(
          commandId: 'cmd-1',
          status: 'turn_injected',
          turnId: 'turn-running',
        ),
      );
      expect(injected.value!.status, CodingSessionReceiptStatus.turnInjected);
      expect(injected.value!.turnId, 'turn-running');
      expect(injected.value!.error, isNull);
      // Without the turn it joined, or with an error, it is malformed.
      expect(
        decodeCodingSessionReceipt(
          receiptEvent(commandId: 'cmd-1', status: 'turn_injected'),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
      expect(
        decodeCodingSessionReceipt(
          receiptEvent(
            commandId: 'cmd-1',
            status: 'turn_injected',
            turnId: 'turn-running',
            error: {'code': 'STEER_ACK_LOST', 'message': 'lost'},
          ),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );

      final unknown = decodeCodingSessionReceipt(
        receiptEvent(
          commandId: 'cmd-1',
          status: 'turn_delivery_unknown',
          error: {'code': 'STEER_ACK_LOST', 'message': 'lost'},
        ),
      );
      expect(
        unknown.value!.status,
        CodingSessionReceiptStatus.turnDeliveryUnknown,
      );
      expect(unknown.value!.error!.code, 'STEER_ACK_LOST');
      expect(unknown.value!.turnId, isNull);
      // No error is a claim of delivery; a turnId claims a turn it cannot name.
      expect(
        decodeCodingSessionReceipt(
          receiptEvent(commandId: 'cmd-1', status: 'turn_delivery_unknown'),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
      expect(
        decodeCodingSessionReceipt(
          receiptEvent(
            commandId: 'cmd-1',
            status: 'turn_delivery_unknown',
            turnId: 'turn-running',
            error: {'code': 'STEER_ACK_LOST', 'message': 'lost'},
          ),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
      // An unrecognised status still fails closed.
      expect(CodingSessionReceiptStatus.fromWire('turn_teleported'), isNull);
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

    test('a seated create (actor + role) decodes', () {
      final decoded = decodeCodingSessionCreate(
        createEvent(
          commandId: 'cmd-1',
          sessionRef: sessionRefA,
          genesisRef: genesisEventIdA,
          actor: 'd' * 64,
          role: 'lead',
        ),
      );
      expect(decoded.value, isNotNull);
      expect(decoded.value!.providerAuthorityPubkey, providerPubkey);
    });

    test('half a seat or a bad seat is malformed', () {
      final cases = <NostrEvent>[
        createEvent(commandId: 'cmd-1', role: 'lead'),
        createEvent(commandId: 'cmd-1', actor: 'd' * 64),
        createEvent(commandId: 'cmd-1', actor: 'D' * 64, role: 'lead'),
        createEvent(commandId: 'cmd-1', actor: 'd' * 64, role: 'Lead'),
      ];
      for (final source in cases) {
        expect(
          decodeCodingSessionCreate(source).reason,
          CodingSessionDecodeReason.malformedPayload,
          reason: source.content,
        );
      }
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

  group('session.resume', () {
    test('binds the provider named for the generation it resumes', () {
      final decoded = decodeCodingSessionResume(
        resumeEvent(commandId: 'cmd-2', forTarget: target(generation: 2)),
      );
      final resume = decoded.value!;
      expect(resume.commandId, 'cmd-2');
      expect(resume.providerAuthorityPubkey, providerPubkey);
      expect(resume.session.generation, 2);
      expect(resume.ref.signerPubkey, founderPubkey);
    });

    test('a create is not a resume, and a resume is not a create', () {
      expect(
        decodeCodingSessionResume(createEvent(commandId: 'cmd-1')).reason,
        CodingSessionDecodeReason.wrongKind,
      );
      expect(
        decodeCodingSessionCreate(resumeEvent(commandId: 'cmd-2')).reason,
        CodingSessionDecodeReason.wrongKind,
      );
    });

    test('an extra action key is malformed, never read past', () {
      final base = resumeEvent(commandId: 'cmd-2');
      final payload = jsonDecode(base.content) as Map<String, dynamic>;
      (payload['action'] as Map<String, dynamic>)['sessionRef'] = sessionRefA;
      final tampered = event(
        kind: base.kind,
        pubkey: base.pubkey,
        tags: base.tags,
        content: jsonEncode(payload),
      );
      expect(
        decodeCodingSessionResume(tampered).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
    });

    test('a resume naming a pubkey that is not 64-hex is malformed', () {
      final base = resumeEvent(commandId: 'cmd-2');
      final payload = jsonDecode(base.content) as Map<String, dynamic>;
      (payload['action'] as Map<String, dynamic>)['providerAuthorityPubkey'] =
          'not-a-key';
      final tampered = event(
        kind: base.kind,
        pubkey: base.pubkey,
        tags: base.tags,
        content: jsonEncode(payload),
      );
      expect(
        decodeCodingSessionResume(tampered).reason,
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

  // The 2026-08-30 routing amendment, plus the two amendments this decoder was
  // already dropping: a seated session's `role` and an umbrella's
  // `turnBudget`. All three ship in buzz-core and on the desktop; until now a
  // payload carrying any of them decoded as corruption here and the session
  // vanished from the observer.
  group('additive metadata amendments', () {
    Map<String, Object?> routingRecord() => <String, Object?>{
      'class': 'builder',
      'tier': 'standard',
      'risk': <String, Object?>{
        'impact': 3,
        'uncertainty': 3,
        'irreversibility': 2,
        'score': 18,
      },
      'chosen': <String, Object?>{
        'provider': 'claude-primary',
        'model': 'sonnet',
        'effort': 'medium',
      },
      'runnerUp': null,
      'reason': 'cleared the builder gates and was the cheapest of them.',
      'reviewRequired': false,
      'reviewReasons': <Object?>[],
      'challengerSample': false,
      'override': null,
      'registryVersion': 1,
      'catalogRevision': null,
    };

    test('a seated, budgeted, routed metadata payload still decodes', () {
      final decoded = decodeCodingSessionMetadata(
        metadataEvent(
          agentRef: 'd' * 64,
          role: 'builder',
          sessionRef: sessionRefA,
          turnBudget: <String, Object?>{'used': 3, 'limit': 20},
          routing: routingRecord(),
        ),
      );
      expect(decoded.value, isNotNull);
      expect(decoded.value!.model, 'opus');
    });

    test('a malformed routing record is refused, never quietly dropped', () {
      final bad = <Map<String, Object?>>[
        <String, Object?>{'class': 'builder'},
        routingRecord()
          ..['chosen'] = <String, Object?>{
            'provider': 'claude-primary',
            'model': 'sonnet',
            'effort': 'xhigh',
          },
        routingRecord()
          ..['risk'] = <String, Object?>{
            'impact': 3,
            'uncertainty': 3,
            'irreversibility': 2,
            'score': 19,
          },
      ];
      for (final routing in bad) {
        expect(
          decodeCodingSessionMetadata(metadataEvent(routing: routing)).reason,
          CodingSessionDecodeReason.malformedPayload,
          reason: jsonEncode(routing),
        );
      }
    });

    // `Routing::profile` in buzz-core has no `skip_serializing_if`
    // (coding_session_routing.rs:838), so a record with no extra trait
    // minimums rides as `profile: null` — and a decoder that accepted only a
    // map refused every record the CLI ever wrote.
    test('profile: null is the shape the canonical producer writes', () {
      final record = routingRecord()..['profile'] = null;
      expect(
        decodeCodingSessionMetadata(metadataEvent(routing: record)).value,
        isNotNull,
      );
    });

    // The host's one sentence when its own choice differs from the `proposed`
    // decision the hire carried. A decoder that refused it would drop exactly
    // the records that disclose a disagreement.
    test('a disclosed disagreement with the proposal decodes', () {
      final record = routingRecord()
        ..['proposedDisagreement'] =
            'the request proposed codex-primary/gpt-5.6-luna (low); this host '
            'routed claude-primary/sonnet (medium).';
      expect(
        decodeCodingSessionMetadata(metadataEvent(routing: record)).value,
        isNotNull,
      );
      final blank = routingRecord()..['proposedDisagreement'] = '   ';
      expect(
        decodeCodingSessionMetadata(metadataEvent(routing: blank)).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
    });

    // The shared fixture the CLI, buzz-core and the desktop are all pinned
    // to. A decoder that refuses a record the canonical producer writes shows
    // the seat with no routing at all, and the create's whole "why it is the
    // model it is" is gone without a word.
    test('every record in the shared fixture decodes here too', () {
      final fixture =
          jsonDecode(
                File(
                  '../testdata/routing/create-record-fixture.json',
                ).readAsStringSync(),
              )
              as Map<String, dynamic>;
      final records = fixture['records'] as List<dynamic>;
      expect(records.length, 3);
      for (final entry in records) {
        final row = entry as Map<String, dynamic>;
        final routing = row['routing'] as Map<String, dynamic>;
        expect(
          decodeCodingSessionMetadata(
            metadataEvent(routing: Map<String, Object?>.from(routing)),
          ).value,
          isNotNull,
          reason: row['name'] as String,
        );
      }
    });

    test('the router\'s own review reasons decode, value and all', () {
      // The canonical router renders the two numeric §6 triggers with the
      // number that fired them (coding_session_routing.rs:1555). This decoder
      // once policed `reviewReasons` against a closed set of slugs and so
      // refused the router's own record as malformed; the vocabulary is open
      // and bounded on both sides now.
      final routed = routingRecord()
        ..['reviewRequired'] = true
        ..['reviewReasons'] = <Object?>[
          'risk 80 >= 40',
          'irreversibility 4 >= 4',
          'securityBoundary',
        ];
      expect(
        decodeCodingSessionMetadata(metadataEvent(routing: routed)).value,
        isNotNull,
      );
    });

    test(
      'a review reason is still bounded, blank and oversized are refused',
      () {
        final bad = <List<Object?>>[
          <Object?>['   '],
          <Object?>['x' * 257],
          <Object?>[42],
          List<Object?>.filled(17, 'leadRequests'),
        ];
        for (final reasons in bad) {
          final routing = routingRecord()
            ..['reviewRequired'] = true
            ..['reviewReasons'] = reasons;
          expect(
            decodeCodingSessionMetadata(metadataEvent(routing: routing)).reason,
            CodingSessionDecodeReason.malformedPayload,
            reason: jsonEncode(reasons),
          );
        }
        final sixteen = routingRecord()
          ..['reviewRequired'] = true
          ..['reviewReasons'] = List<Object?>.filled(16, 'leadRequests');
        expect(
          decodeCodingSessionMetadata(metadataEvent(routing: sixteen)).value,
          isNotNull,
          reason: 'sixteen is the cap buzz-core signs, not one fewer',
        );
      },
    );

    test('a routed create decodes, and a malformed one is refused', () {
      expect(
        decodeCodingSessionCreate(
          createEvent(
            commandId: 'cmd-1',
            sessionRef: sessionRefA,
            genesisRef: genesisEventIdA,
            routing: routingRecord(),
          ),
        ).value,
        isNotNull,
      );
      expect(
        decodeCodingSessionCreate(
          createEvent(
            commandId: 'cmd-1',
            routing: <String, Object?>{'class': 'builder'},
          ),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
    });
  });
}

void _promptImageAmendment() {
  group('44223 capabilities.promptImage', () {
    test('a payload carrying promptImage decodes, and keeps the flag', () {
      final source = metadataEvent();
      final payload = jsonDecode(source.content) as Map<String, dynamic>;
      (payload['capabilities'] as Map<String, dynamic>)['promptImage'] = true;
      final decoded = decodeCodingSessionMetadata(
        _withContent(source, jsonEncode(payload)),
      );
      expect(decoded.value, isNotNull, reason: decoded.reason.toString());
      expect(decoded.value!.capabilities['promptImage'], isTrue);
      expect(decoded.value!.capabilities['plan'], isNotNull);
    });

    test('a non-boolean promptImage is still corruption', () {
      final source = metadataEvent();
      final payload = jsonDecode(source.content) as Map<String, dynamic>;
      (payload['capabilities'] as Map<String, dynamic>)['promptImage'] = 'yes';
      final decoded = decodeCodingSessionMetadata(
        _withContent(source, jsonEncode(payload)),
      );
      expect(decoded.reason, CodingSessionDecodeReason.malformedPayload);
    });

    // The exact event the dev relay held on 2026-09-08 for a session the
    // desktop showed as idle and the phone showed as "unknown": promptImage,
    // turnBudget, sessionRef and the four B1 facts, all at once.
    test('the 2026-09-08 dev-relay metadata event decodes', () {
      final json =
          jsonDecode(
                File(
                  'test/features/coding_sessions/domain/fixtures/'
                  'metadata_44223_dev_relay_2026-09-08.json',
                ).readAsStringSync(),
              )
              as Map<String, dynamic>;
      final decoded = decodeCodingSessionMetadata(NostrEvent.fromJson(json));
      expect(decoded.value, isNotNull, reason: decoded.reason.toString());
      expect(decoded.value!.status, CodingSessionStatus.idle);
      expect(decoded.value!.title, 'ping 2');
      expect(decoded.value!.capabilities['promptImage'], isTrue);
    });
  });
}
