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
