import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

import 'coding_session_fixtures.dart';

final _checkpoint = 'c1' * 32;

Map<String, Object?> _rewind({
  String files = 'restored',
  int cutGeneration = 1,
  int previousGeneration = 1,
}) => {
  'checkpoint': _checkpoint,
  'cutGeneration': cutGeneration,
  'cutAfterSeq': 4,
  'previousGeneration': previousGeneration,
  'files': files,
  'preRewindCheckpoint': 'c2' * 32,
  'head': 'e' * 40,
};

void main() {
  group('SV-29 receipt rewind', () {
    test('a resumed receipt naming N+1 carries its seven-key rewind', () {
      final decoded = decodeCodingSessionReceipt(
        receiptEvent(
          commandId: 'rewind-1',
          status: 'resumed',
          forTarget: target(generation: 2),
          rewind: _rewind(),
        ),
      );
      expect(decoded.value, isNotNull);
      expect(decoded.value!.status, CodingSessionReceiptStatus.resumed);
    });

    test('failed REWIND_NOT_RESTARTED may carry any files outcome', () {
      final decoded = decodeCodingSessionReceipt(
        receiptEvent(
          commandId: 'rewind-1',
          status: 'failed',
          error: {'code': 'REWIND_NOT_RESTARTED', 'message': 'needs a Restart'},
          rewind: _rewind(files: 'restore_failed'),
        ),
      );
      expect(decoded.value, isNotNull);
    });

    test('a rewind off its rule is refused, never partially read', () {
      final refused = <String, NostrEventFactory>{
        'unknown key': () => receiptEvent(
          commandId: 'r',
          status: 'resumed',
          forTarget: target(generation: 2),
          rewind: {..._rewind(), 'extra': 1},
        ),
        'missing key': () => receiptEvent(
          commandId: 'r',
          status: 'resumed',
          forTarget: target(generation: 2),
          rewind: {..._rewind()}..remove('head'),
        ),
        'wrong generation': () => receiptEvent(
          commandId: 'r',
          status: 'resumed',
          forTarget: target(generation: 3),
          rewind: _rewind(),
        ),
        'restore_failed on success': () => receiptEvent(
          commandId: 'r',
          status: 'resumed',
          forTarget: target(generation: 2),
          rewind: _rewind(files: 'restore_failed'),
        ),
        'cut past previous': () => receiptEvent(
          commandId: 'r',
          status: 'resumed',
          forTarget: target(generation: 2),
          rewind: _rewind(cutGeneration: 2),
        ),
        'another failure code': () => receiptEvent(
          commandId: 'r',
          status: 'failed',
          error: {'code': 'SESSION_BUSY', 'message': 'busy'},
          rewind: _rewind(),
        ),
        'another status': () =>
            receiptEvent(commandId: 'r', status: 'stopped', rewind: _rewind()),
      };
      for (final entry in refused.entries) {
        expect(
          decodeCodingSessionReceipt(entry.value()).reason,
          CodingSessionDecodeReason.malformedPayload,
          reason: entry.key,
        );
      }
    });
  });

  group('SV-29 session.rewind', () {
    test('decodes as a next-generation step, exactly five keys', () {
      final decoded = decodeCodingSessionResume(
        resumeEvent(
          commandId: 'rewind-1',
          type: 'session.rewind',
          extraAction: {'checkpoint': _checkpoint, 'files': 'keep'},
        ),
      );
      expect(decoded.value, isNotNull);
      for (final bad in [
        <String, Object?>{'checkpoint': _checkpoint},
        {'checkpoint': 'AB', 'files': 'keep'},
        {'checkpoint': _checkpoint, 'files': 'all'},
        {'checkpoint': _checkpoint, 'files': 'keep', 'extra': 1},
      ]) {
        expect(
          decodeCodingSessionResume(
            resumeEvent(
              commandId: 'rewind-1',
              type: 'session.rewind',
              extraAction: bad,
            ),
          ).value,
          isNull,
          reason: '$bad',
        );
      }
    });

    test('a rewind vouches for the generation it mints, like a resume', () {
      final rewound = target(generation: 2);
      final facts = applyCodingSessionTrustGate(
        channelId: channelId,
        events: [
          resumeEvent(
            commandId: 'rewind-1',
            type: 'session.rewind',
            extraAction: {'checkpoint': _checkpoint, 'files': 'restore'},
          ),
          metadataEvent(
            forTarget: rewound,
            status: 'running',
            pubkey: otherProviderPubkey,
            createdAt: 800,
          ),
          receiptEvent(
            commandId: 'rewind-1',
            status: 'resumed',
            forTarget: rewound,
            createdAt: 900,
            rewind: _rewind(),
          ),
          metadataEvent(forTarget: rewound, status: 'idle', createdAt: 1000),
        ],
      );
      final authority = facts.authorityByTarget[rewound.key]!;
      expect(authority.pubkey, providerPubkey);
      expect(authority.verified, isTrue);
      expect(facts.targetKeyByCommandId['rewind-1'], rewound.key);
    });
  });

  test('the session_rewound item renders the provider’s own words', () {
    final item = <String, Object?>{
      'kind': 'status',
      'status': 'session_rewound',
      'commandId': 'rewind-1',
      'checkpoint': _checkpoint,
      'cutGeneration': 1,
      'cutAfterSeq': 4,
      'previousGeneration': 1,
      'files': 'restored',
      'memory': 'seeded',
    };
    CodingSessionTranscriptItem row(Map<String, Object?> item) =>
        projectCodingSessionTranscript([
          decodeCodingSessionTranscript(
            transcriptEvent(eventSeq: 1, item: item),
          ).value!,
        ]).single.items.single;
    expect(row(item).title, 'Rewound');
    expect(
      row(item).text,
      'Rewound to before this turn · files restored · '
      'new conversation seeded from the record',
    );
    expect(
      row({...item, 'files': 'kept', 'memory': 'none'}).text,
      'Rewound to before this turn · files kept · restarted with no memory',
    );
  });
}

typedef NostrEventFactory = NostrEvent Function();
