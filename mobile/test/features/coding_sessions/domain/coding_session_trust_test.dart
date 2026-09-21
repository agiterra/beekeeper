import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

import 'coding_session_fixtures.dart';

CodingSessionTrustedFacts _gate(List<NostrEvent> events) =>
    applyCodingSessionTrustGate(channelId: channelId, events: events);

void main() {
  group('authority', () {
    test('a receipt-joined create pins the provider whose facts count', () {
      final facts = _gate([
        createEvent(commandId: 'cmd-1'),
        receiptEvent(commandId: 'cmd-1', status: 'created'),
        metadataEvent(status: 'running'),
      ]);
      final authority = facts.authorityByTarget[target().key]!;
      expect(authority.pubkey, providerPubkey);
      expect(authority.verified, isTrue);
      expect(facts.metadata, hasLength(1));
      expect(facts.counts.rejectedAuthor, 0);
    });

    // Ledger 216(a). `hireRef` is the 2026-09-01 attribution amendment every
    // hired seat's create carries, and `createKeys` here is an exact-key list
    // that never grew it — so from that day until lane 216 **every** hired
    // seat's create decoded as `malformedPayload` on this device. The
    // execution then fell through to D5's first-seen-metadata-signer
    // fallback, which renders as "authority unverified" in the session
    // header, and the read's malformed counter ticked for kind 44221.
    test('a hired seat\'s create binds authority like any other', () {
      final facts = _gate([
        createEvent(
          commandId: 'cmd-1',
          actor: seatActorPubkey,
          role: 'builder',
          hireRef: hireEventId,
        ),
        receiptEvent(commandId: 'cmd-1', status: 'created'),
        metadataEvent(status: 'running'),
      ]);
      final authority = facts.authorityByTarget[target().key]!;
      expect(authority.pubkey, providerPubkey);
      expect(
        authority.verified,
        isTrue,
        reason: 'a create carrying hireRef must still pin the provider',
      );
      expect(facts.creates, hasLength(1));
      expect(facts.counts.malformed, 0);
    });

    test('facts from a signer the create did not name are rejected', () {
      final facts = _gate([
        createEvent(commandId: 'cmd-1'),
        receiptEvent(commandId: 'cmd-1', status: 'created'),
        metadataEvent(status: 'running'),
        metadataEvent(
          status: 'failed',
          pubkey: otherProviderPubkey,
          createdAt: 5000,
        ),
      ]);
      expect(facts.metadata, hasLength(1));
      expect(facts.metadata.single.status, CodingSessionStatus.running);
      expect(facts.counts.rejectedAuthor, 1);
    });

    // D5 makes the first-seen-metadata-signer fallback the answer only "when
    // no create is readable". Two receipt-joined creates naming one target for
    // different providers are a dispute, not an absence: falling back would
    // let whoever published the earliest metadata own a victim's execution.
    test(
      'a disputed target never falls back to the earliest metadata signer',
      () {
        final events = [
          // The genuine provider's execution.
          createEvent(commandId: 'cmd-1'),
          receiptEvent(commandId: 'cmd-1', status: 'created'),
          metadataEvent(status: 'running', createdAt: 2000),
          // A channel member claiming the same execution for themselves, with
          // metadata timed to win any first-seen race.
          createEvent(
            commandId: 'cmd-2',
            pubkey: otherFounderPubkey,
            authority: otherProviderPubkey,
          ),
          receiptEvent(
            commandId: 'cmd-2',
            status: 'created',
            pubkey: otherProviderPubkey,
          ),
          metadataEvent(
            status: 'failed',
            pubkey: otherProviderPubkey,
            createdAt: 500,
          ),
        ];
        final facts = _gate(events);
        expect(facts.authorityByTarget[target().key], isNull);
        expect(facts.metadata, isEmpty);
        expect(facts.counts.conflicts, greaterThan(0));

        final view = readCodingSessionChannel(
          channelId: channelId,
          verifier: null,
          events: events,
        );
        expect(view.sessions, isEmpty);
      },
    );

    // A 44221 carries only h / csl-v / csl-command, so a commandId is a
    // claim, not a credential: anyone in the channel can publish a second
    // create reusing a live one. The command is then disputed and binds
    // nothing — but the execution its receipt named must not quietly drop
    // into the first-seen-metadata-signer fallback, or the forger's backdated
    // 44223 owns the victim's execution.
    test('a forged create reusing a live commandId cannot take the target', () {
      final events = [
        // The genuine command, its provider's receipt, and its facts.
        createEvent(commandId: 'cmd-1'),
        receiptEvent(commandId: 'cmd-1', status: 'created'),
        metadataEvent(status: 'running', createdAt: 2000),
        // A channel member republishing the same commandId for themselves,
        // with metadata timed to win any first-seen race.
        createEvent(
          commandId: 'cmd-1',
          pubkey: otherFounderPubkey,
          authority: otherProviderPubkey,
        ),
        metadataEvent(
          status: 'failed',
          pubkey: otherProviderPubkey,
          createdAt: 500,
        ),
      ];
      final facts = _gate(events);
      expect(facts.authorityByTarget[target().key], isNull);
      expect(facts.metadata, isEmpty);
      expect(facts.counts.conflicts, greaterThan(0));

      final view = readCodingSessionChannel(
        channelId: channelId,
        verifier: null,
        events: events,
      );
      expect(view.sessions, isEmpty);
    });

    // One command cannot have minted two executions. Which one it really
    // minted is unreadable, so neither may fall back to a metadata signer.
    test('a command answered with two executions disputes both', () {
      final second = target(sessionId: 'session-2');
      final facts = _gate([
        createEvent(commandId: 'cmd-1'),
        receiptEvent(commandId: 'cmd-1', status: 'created'),
        receiptEvent(commandId: 'cmd-1', status: 'resumed', forTarget: second),
        metadataEvent(status: 'running'),
        metadataEvent(forTarget: second, status: 'running'),
      ]);
      expect(facts.authorityByTarget[target().key], isNull);
      expect(facts.authorityByTarget[second.key], isNull);
      expect(facts.metadata, isEmpty);
      expect(facts.counts.conflicts, greaterThan(0));
    });

    test('a transcript for a target nobody vouches for is not rendered', () {
      final facts = _gate([
        transcriptEvent(
          eventSeq: 1,
          item: {'kind': 'assistant_text', 'text': 'unattributable'},
        ),
      ]);
      expect(facts.transcripts, isEmpty);
      expect(facts.counts.rejectedAuthor, 1);
    });

    test('facts from different signers never merge', () {
      final facts = _gate([
        createEvent(commandId: 'cmd-1'),
        receiptEvent(commandId: 'cmd-1', status: 'created'),
        transcriptEvent(
          eventSeq: 1,
          item: {'kind': 'assistant_text', 'text': 'mine'},
        ),
        transcriptEvent(
          eventSeq: 2,
          pubkey: otherProviderPubkey,
          item: {'kind': 'assistant_text', 'text': 'theirs'},
        ),
      ]);
      expect(facts.transcripts, hasLength(1));
      expect(facts.transcripts.single.ref.signerPubkey, providerPubkey);
      expect(facts.counts.rejectedAuthor, 1);
    });

    test('a turn receipt alone never joins a create to a target', () {
      final facts = _gate([
        createEvent(commandId: 'cmd-1'),
        receiptEvent(commandId: 'cmd-1', status: 'turn_queued'),
        metadataEvent(status: 'running'),
      ]);
      // The metadata still renders — the fallback authority covers it — but
      // nobody has *verified* who may speak for the target.
      expect(facts.authorityByTarget[target().key]!.verified, isFalse);
    });

    test(
      'with no readable create the first-seen metadata signer stands in',
      () {
        final facts = _gate([
          metadataEvent(status: 'running', createdAt: 1000),
          metadataEvent(
            status: 'failed',
            pubkey: otherProviderPubkey,
            createdAt: 2000,
          ),
        ]);
        final authority = facts.authorityByTarget[target().key]!;
        expect(authority.pubkey, providerPubkey);
        expect(authority.verified, isFalse);
        expect(facts.counts.rejectedAuthor, 1);
      },
    );

    test('a resume vouches for the generation it mints', () {
      final resumed = target(generation: 2);
      final facts = _gate([
        resumeEvent(commandId: 'cmd-2'),
        receiptEvent(
          commandId: 'cmd-2',
          status: 'resumed',
          forTarget: resumed,
          createdAt: 900,
        ),
        metadataEvent(forTarget: resumed, status: 'running', createdAt: 1000),
      ]);
      final authority = facts.authorityByTarget[resumed.key]!;
      expect(authority.pubkey, providerPubkey);
      expect(
        authority.verified,
        isTrue,
        reason: 'the resume named the provider that answered it',
      );
      expect(facts.targetKeyByCommandId['cmd-2'], resumed.key);
    });

    test('a stranger cannot speak for a resumed generation', () {
      final resumed = target(generation: 2);
      final facts = _gate([
        resumeEvent(commandId: 'cmd-2'),
        // The stranger's metadata lands first, so before the resume was read
        // it became the fallback authority for the whole generation.
        metadataEvent(
          forTarget: resumed,
          status: 'running',
          pubkey: otherProviderPubkey,
          createdAt: 800,
        ),
        receiptEvent(
          commandId: 'cmd-2',
          status: 'resumed',
          forTarget: resumed,
          createdAt: 900,
        ),
        metadataEvent(forTarget: resumed, status: 'idle', createdAt: 1000),
      ]);
      expect(facts.authorityByTarget[resumed.key]!.pubkey, providerPubkey);
      expect(facts.metadata, hasLength(1));
      expect(facts.metadata.single.ref.signerPubkey, providerPubkey);
      expect(facts.counts.rejectedAuthor, 1);
    });

    test('two creates disagreeing about the provider bind nothing', () {
      final facts = _gate([
        createEvent(commandId: 'cmd-1', authority: providerPubkey),
        createEvent(commandId: 'cmd-1', authority: otherProviderPubkey),
        receiptEvent(commandId: 'cmd-1', status: 'created'),
      ]);
      expect(facts.counts.conflicts, greaterThan(0));
      expect(facts.authorityByTarget[target().key], isNull);
    });

    // A commandId is a claim, not a credential, and a receipt naming one is
    // no better. A disputed command must not reach past its own execution and
    // strip a target that a different, undisputed, receipt-joined create
    // already bound: that would let any channel member erase any session in
    // the channel using only events they can sign themselves.
    test('a forged disputed commandId cannot revoke a target bound by its own '
        'undisputed create', () {
      final events = [
        // The victim's session, bound end to end by its own command.
        createEvent(
          commandId: 'cmd-victim',
          sessionRef: sessionRefA,
          genesisRef: genesisEventIdA,
        ),
        genesisEvent(eventId: genesisEventIdA, sessionRef: sessionRefA),
        receiptEvent(commandId: 'cmd-victim', status: 'created'),
        metadataEvent(
          status: 'running',
          sessionRef: sessionRefA,
          createdAt: 2000,
        ),
        transcriptEvent(
          eventSeq: 1,
          item: {'kind': 'assistant_text', 'text': 'hi'},
        ),
        leaseEvent(commandId: 'cmd-victim', createdAt: 2000),
        // An attacker disputes a commandId of their own, colliding with
        // nothing, then points a receipt under it at the victim's target.
        createEvent(
          commandId: 'cmd-atk',
          pubkey: otherFounderPubkey,
          authority: otherProviderPubkey,
        ),
        createEvent(
          commandId: 'cmd-atk',
          pubkey: otherFounderPubkey,
          authority: founderPubkey,
        ),
        receiptEvent(
          commandId: 'cmd-atk',
          status: 'created',
          pubkey: otherProviderPubkey,
        ),
      ];
      final facts = _gate(events);
      final authority = facts.authorityByTarget[target().key];
      expect(authority, isNotNull);
      expect(authority!.pubkey, providerPubkey);
      expect(authority.verified, isTrue);
      expect(facts.metadata, hasLength(1));

      final view = readCodingSessionChannel(
        channelId: channelId,
        verifier: null,
        events: events,
      );
      expect(view.sessions, hasLength(1));
    });

    // A `failed` receipt names no execution, so it is attributed by the
    // command it answers. Refusing one is still a refusal, and the read
    // counts are meant to be a complete account of what was refused.
    test(
      'a failed receipt from a signer the command did not name is counted',
      () {
        final facts = _gate([
          createEvent(commandId: 'cmd-1'),
          receiptEvent(
            commandId: 'cmd-1',
            status: 'failed',
            pubkey: otherProviderPubkey,
            error: {'code': 'NO_CAPACITY', 'message': 'busy'},
          ),
        ]);
        expect(facts.receipts, isEmpty);
        expect(facts.counts.malformed, 0);
        expect(facts.counts.rejectedAuthor, 1);
      },
    );
  });

  // The disclosure is only as useful as it is specific: a hundred refused
  // 44225 transcript envelopes and a hundred refused 44229 names cost the same
  // total and mean entirely different things, and "invalid signature" and
  // "unauthorized signer" are accusations against different parties.
  group('read counts per kind', () {
    NostrEvent brokenOfKind(int kind, List<List<String>> tags) => NostrEvent(
      id: nextEventId(),
      pubkey: providerPubkey,
      createdAt: 1000,
      kind: kind,
      tags: [
        ['h', channelId],
        ...tags,
      ],
      content: '{}',
      sig: '0' * 128,
    );

    test('names the kind behind every malformed fact', () {
      final facts = _gate([
        brokenOfKind(EventKind.codingSessionMetadata, [
          ['csm-v', 'csm1-1'],
        ]),
        brokenOfKind(EventKind.codingSessionTranscript, [
          ['cst-v', 'cst1-1'],
          ['cs-target', target().key],
          ['cst-seq', '1'],
          ['cst-key', target().transcriptSemanticKey(1)],
        ]),
        brokenOfKind(EventKind.codingSessionTranscript, [
          ['cst-v', 'cst1-1'],
          ['cs-target', target().key],
          ['cst-seq', '2'],
          ['cst-key', target().transcriptSemanticKey(2)],
        ]),
      ]);

      expect(facts.counts.malformed, 3);
      expect(facts.counts.malformedByKind, {
        EventKind.codingSessionMetadata: 1,
        EventKind.codingSessionTranscript: 2,
      });
      expect(facts.counts.rejectedAuthorByKind, isEmpty);
      expect(facts.counts.invalidSignatureByKind, isEmpty);
    });

    test('names the kind behind every unauthorized signer', () {
      final facts = _gate([
        createEvent(commandId: 'cmd-1'),
        receiptEvent(commandId: 'cmd-1', status: 'created'),
        metadataEvent(status: 'running'),
        // A stranger's facts for the same execution: correctly shaped, signed
        // by nobody the create vouched for.
        metadataEvent(
          status: 'failed',
          pubkey: otherProviderPubkey,
          createdAt: 5000,
        ),
        transcriptEvent(
          eventSeq: 1,
          item: {'kind': 'assistant_text', 'text': 'not theirs to say'},
          pubkey: otherProviderPubkey,
        ),
        receiptEvent(
          commandId: 'cmd-1',
          status: 'failed',
          pubkey: otherProviderPubkey,
          error: {'code': 'NO_CAPACITY', 'message': 'busy'},
        ),
      ]);

      expect(facts.counts.rejectedAuthor, 3);
      expect(facts.counts.rejectedAuthorByKind, {
        EventKind.codingSessionMetadata: 1,
        EventKind.codingSessionLifecycleReceipt: 1,
        EventKind.codingSessionTranscript: 1,
      });
      expect(facts.counts.malformedByKind, isEmpty);
    });

    test('names the kind behind every invalid signature', () {
      // The fixtures carry a placeholder signature, so a real verifier refuses
      // all of them — which is exactly the "signed by nobody" case.
      final facts = applyCodingSessionTrustGate(
        channelId: channelId,
        verifier: const NostrPackageSignatureVerifier(),
        events: [
          metadataEvent(status: 'running'),
          nameEvent(content: 'Ship it'),
          leaseEvent(),
        ],
      );

      expect(facts.counts.invalidSignature, 3);
      expect(facts.counts.invalidSignatureByKind, {
        EventKind.codingSessionMetadata: 1,
        EventKind.codingSessionName: 1,
        EventKind.codingSessionLease: 1,
      });
      expect(facts.counts.malformedByKind, isEmpty);
    });

    test('a clean read names no kind at all', () {
      final facts = _gate([
        createEvent(commandId: 'cmd-1'),
        receiptEvent(commandId: 'cmd-1', status: 'created'),
        metadataEvent(status: 'running'),
      ]);

      expect(facts.counts.isClean, isTrue);
      expect(facts.counts.malformedByKind, isEmpty);
      expect(facts.counts.rejectedAuthorByKind, isEmpty);
      expect(facts.counts.invalidSignatureByKind, isEmpty);
    });
  });

  group('duplicates and conflicts', () {
    test('a byte-identical duplicate collapses by event id', () {
      final metadata = metadataEvent(status: 'running');
      final facts = _gate([metadata, metadata]);
      expect(facts.metadata, hasLength(1));
      expect(facts.counts.duplicates, 1);
    });

    test(
      'two distinct payloads at one (target, signer, seq) render neither',
      () {
        final facts = _gate([
          // Metadata first: without it the target has no authority at all and
          // every transcript would be dropped for a different reason.
          metadataEvent(status: 'running'),
          transcriptEvent(
            eventSeq: 1,
            item: {'kind': 'assistant_text', 'text': 'one story'},
            id: '${'0' * 63}a',
          ),
          transcriptEvent(
            eventSeq: 1,
            item: {'kind': 'assistant_text', 'text': 'another story'},
            id: '${'0' * 63}b',
          ),
          transcriptEvent(
            eventSeq: 2,
            item: {'kind': 'assistant_text', 'text': 'undisputed'},
          ),
        ]);
        expect(facts.transcripts, hasLength(1));
        expect(facts.transcripts.single.eventSeq, 2);
        expect(facts.counts.conflicts, 1);
      },
    );

    test('the same payload republished under a new id renders once', () {
      final facts = _gate([
        metadataEvent(status: 'running'),
        transcriptEvent(
          eventSeq: 1,
          item: {'kind': 'assistant_text', 'text': 'same'},
          id: '${'0' * 63}a',
        ),
        transcriptEvent(
          eventSeq: 1,
          item: {'kind': 'assistant_text', 'text': 'same'},
          id: '${'0' * 63}b',
        ),
      ]);
      expect(facts.transcripts, hasLength(1));
      expect(facts.counts.conflicts, 0);
    });

    test('a malformed event is counted, never guessed at', () {
      final broken = NostrEvent(
        id: nextEventId(),
        pubkey: providerPubkey,
        createdAt: 1000,
        kind: EventKind.codingSessionMetadata,
        tags: [
          ['h', channelId],
          ['csm-v', 'csm1-1'],
        ],
        content: '{}',
        sig: '0' * 128,
      );
      final facts = _gate([broken]);
      expect(facts.metadata, isEmpty);
      expect(facts.counts.malformed, 1);
    });

    test('events from another channel are ignored entirely', () {
      final foreign = NostrEvent(
        id: nextEventId(),
        pubkey: providerPubkey,
        createdAt: 1000,
        kind: EventKind.codingSessionMetadata,
        tags: [
          ['h', 'another-channel'],
          ['csm-v', 'csm1-1'],
          ['cs-target', target().key],
          ['csm-key', target().metadataSemanticKey],
        ],
        content: metadataEvent().content,
        sig: '0' * 128,
      );
      final facts = _gate([foreign]);
      expect(facts.metadata, isEmpty);
      expect(facts.counts.malformed, 0);
    });
  });

  group('readCodingSessionChannel', () {
    test('folds a whole channel into sessions with a transcript', () {
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
          nameEvent(content: 'Nightly refactor'),
          transcriptEvent(
            eventSeq: 1,
            turnId: 'turn-1',
            item: {'kind': 'user_prompt', 'content': 'go'},
          ),
          transcriptEvent(
            eventSeq: 2,
            turnId: 'turn-1',
            item: {'kind': 'assistant_text', 'text': 'working'},
          ),
          leaseEvent(createdAt: 1500),
        ],
      );
      expect(view.sessions, hasLength(1));
      final session = view.sessions.single;
      expect(session.displayName, 'Nightly refactor');
      expect(
        session.founder.resolution,
        CodingSessionFounderResolution.genesis,
      );
      expect(session.status.kind, CodingSessionFoldedStatusKind.working);
      expect(session.hasUnverifiedAuthority, isFalse);

      final blocks = view.transcriptFor(session);
      expect(blocks.single.items, hasLength(2));
      expect(blocks.single.label, 'claude-agent-acp · opus');

      final reachable = view.reachabilityFor(
        session,
        now: DateTime.fromMillisecondsSinceEpoch(1600 * 1000, isUtc: true),
      );
      expect(reachable.kind, CodingSessionReachabilityKind.reachable);

      final stale = view.reachabilityFor(
        session,
        now: DateTime.fromMillisecondsSinceEpoch(9000 * 1000, isUtc: true),
      );
      expect(stale.kind, CodingSessionReachabilityKind.noProviderAnswering);
    });

    // D6: a same-second distinct-payload metadata collision is a conflict, so
    // it has to reach the counter `isClean` is read from. Carrying it only on
    // the execution let the disclosure line call the read clean while a live
    // conflict was on screen.
    test('a same-second metadata conflict reaches the read counts', () {
      final view = readCodingSessionChannel(
        channelId: channelId,
        verifier: null,
        events: [
          createEvent(commandId: 'cmd-1'),
          receiptEvent(commandId: 'cmd-1', status: 'created'),
          metadataEvent(status: 'running', createdAt: 2000, title: 'one'),
          metadataEvent(status: 'running', createdAt: 2000, title: 'two'),
        ],
      );
      expect(view.sessions.single.executions.single.statusConflict, isTrue);
      expect(view.counts.conflicts, 1);
      expect(view.counts.isClean, isFalse);
    });

    test('an unread lease query never reads as nobody answering', () {
      final view = readCodingSessionChannel(
        channelId: channelId,
        verifier: null,
        leasesRead: false,
        events: [
          createEvent(commandId: 'cmd-1', sessionRef: sessionRefA),
          receiptEvent(commandId: 'cmd-1', status: 'created'),
          metadataEvent(status: 'running', sessionRef: sessionRefA),
        ],
      );
      expect(
        view.reachabilityFor(view.sessions.single, now: DateTime.now()).kind,
        CodingSessionReachabilityKind.unknown,
      );
    });

    test('signaturesVerified is false when no verifier is reachable', () {
      final view = readCodingSessionChannel(
        channelId: channelId,
        verifier: const UnavailableSignatureVerifier(),
        events: [metadataEvent()],
      );
      expect(view.signaturesVerified, isFalse);
    });

    test('retention keeps the newest events and evicts the oldest', () {
      final events = [
        for (var index = 0; index < 10; index += 1)
          metadataEvent(createdAt: 1000 + index),
      ];
      final retained = retainNewestCodingSessionEvents(events, cap: 4);
      expect(retained, hasLength(4));
      expect(retained.first.createdAt, 1006);
      expect(retained.last.createdAt, 1009);
    });
  });
}
