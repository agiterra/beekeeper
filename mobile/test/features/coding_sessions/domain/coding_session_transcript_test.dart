import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:flutter_test/flutter_test.dart';

import 'coding_session_fixtures.dart';

CodingSessionTranscriptEnvelope _envelope({
  required int eventSeq,
  required Map<String, Object?> item,
  CodingSessionTarget? forTarget,
  String pubkey = providerPubkey,
  String? turnId,
  int createdAt = 1100,
  String? id,
}) => decodeCodingSessionTranscript(
  transcriptEvent(
    eventSeq: eventSeq,
    item: item,
    forTarget: forTarget,
    pubkey: pubkey,
    turnId: turnId,
    createdAt: createdAt,
    id: id,
  ),
).value!;

void main() {
  group('ordering', () {
    test('eventSeq orders numerically: 10 comes after 9', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 10,
          item: {'kind': 'assistant_text', 'text': 'ten'},
        ),
        _envelope(
          eventSeq: 9,
          item: {'kind': 'assistant_text', 'text': 'nine'},
        ),
        _envelope(eventSeq: 2, item: {'kind': 'assistant_text', 'text': 'two'}),
      ]);
      expect(blocks.single.items.map((item) => item.eventSeq).toList(), [
        2,
        9,
        10,
      ]);
      expect(blocks.single.items.last.text, 'ten');
    });

    test('a tie on eventSeq breaks by event id, not by arrival', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          item: {'kind': 'assistant_text', 'text': 'second'},
          id: '${'0' * 63}b',
          forTarget: target(),
        ),
        _envelope(
          eventSeq: 1,
          item: {'kind': 'assistant_text', 'text': 'first'},
          id: '${'0' * 63}a',
          forTarget: target(),
        ),
      ]);
      expect(blocks.single.items.first.text, 'first');
    });
  });

  group('blocks', () {
    test('two executions interleave as blocks, never as items', () {
      final blocks = projectCodingSessionTranscript(
        [
          _envelope(
            eventSeq: 1,
            item: {'kind': 'assistant_text', 'text': 'a1'},
            createdAt: 100,
          ),
          _envelope(
            eventSeq: 1,
            item: {'kind': 'assistant_text', 'text': 'b1'},
            forTarget: target(sessionId: 'session-2'),
            pubkey: otherProviderPubkey,
            createdAt: 200,
          ),
          _envelope(
            eventSeq: 2,
            item: {'kind': 'assistant_text', 'text': 'a2'},
            createdAt: 300,
          ),
        ],
        labelsByTargetKey: {target().key: 'Claude Code · opus'},
      );
      expect(blocks, hasLength(2));
      final first = blocks.firstWhere(
        (block) => block.signerPubkey == providerPubkey,
      );
      expect(first.items.map((item) => item.text).toList(), ['a1', 'a2']);
      expect(first.label, 'Claude Code · opus');
      final second = blocks.firstWhere(
        (block) => block.signerPubkey == otherProviderPubkey,
      );
      expect(second.items.single.text, 'b1');
    });

    test('one signer with two generations gets one block each', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(eventSeq: 1, item: {'kind': 'assistant_text', 'text': 'g1'}),
        _envelope(
          eventSeq: 1,
          item: {'kind': 'assistant_text', 'text': 'g2'},
          forTarget: target(generation: 2),
          createdAt: 1200,
        ),
      ]);
      expect(blocks, hasLength(2));
    });
  });

  group('turns', () {
    test('the declared turnId groups contiguous rows', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          turnId: 'turn-1',
          item: {'kind': 'user_prompt', 'content': 'go'},
        ),
        _envelope(
          eventSeq: 2,
          turnId: 'turn-1',
          item: {'kind': 'assistant_text', 'text': 'ok'},
        ),
        _envelope(
          eventSeq: 3,
          turnId: 'turn-2',
          item: {'kind': 'user_prompt', 'content': 'again'},
        ),
      ]);
      final turns = blocks.single.turns;
      expect(turns.map((turn) => turn.turnId).toList(), ['turn-1', 'turn-2']);
      expect(turns.first.items, hasLength(2));
    });

    test('a null turnId means the row belongs to no turn', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(eventSeq: 1, item: {'kind': 'system_init'}),
        _envelope(
          eventSeq: 2,
          turnId: 'turn-1',
          item: {'kind': 'user_prompt', 'content': 'go'},
        ),
      ]);
      expect(blocks.single.turns.first.turnId, isNull);
      expect(blocks.single.items.first.turnId, isNull);
    });
  });

  group('item kinds', () {
    test('a prompt carries its steer flag, operator and command id', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          item: {
            'kind': 'user_prompt',
            'content': 'do the thing',
            'steered': true,
            'operatorPubkey': founderPubkey,
            'commandId': 'cmd-77',
          },
        ),
      ]);
      final item = blocks.single.items.single;
      expect(item.title, 'Steered prompt');
      expect(item.role, CodingSessionItemRole.user);
      expect(item.steered, isTrue);
      expect(item.operatorPubkey, founderPubkey);
      expect(item.commandId, 'cmd-77');
    });

    // A turn that carried files must not read exactly like one that carried
    // none. The count is signed; the files are not on this device and the row
    // never implies they are.
    test('a prompt says how many attachments came with it', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          item: {
            'kind': 'user_prompt',
            'content': 'review these',
            'attachmentCount': 3,
          },
        ),
        _envelope(
          eventSeq: 2,
          item: {
            'kind': 'user_prompt',
            'content': 'and this',
            'attachmentCount': 1,
          },
        ),
        _envelope(
          eventSeq: 3,
          item: {
            'kind': 'user_prompt',
            'content': 'nothing attached',
            'attachmentCount': 0,
          },
        ),
      ]);
      final items = blocks.single.items;
      expect(items[0].text, 'review these\n\n(3 attachments)');
      expect(items[1].text, 'and this\n\n(1 attachment)');
      expect(items[2].text, 'nothing attached');
    });

    test(
      'a result keeps duration and cost structured, not baked into text',
      () {
        final blocks = projectCodingSessionTranscript([
          _envelope(
            eventSeq: 1,
            item: {
              'kind': 'result',
              'subtype': 'success',
              'durationMs': 4200,
              'costUsd': 0.12,
              'result': 'done',
              'isError': false,
            },
          ),
        ]);
        final item = blocks.single.items.single;
        expect(item.title, 'Turn result');
        expect(item.text, 'done');
        expect(item.result!.durationMs, 4200);
        expect(item.result!.costUsd, 0.12);
        expect(item.result!.outcome, 'success');
        expect(item.text, isNot(contains('4200')));
      },
    );

    test('a known continuity slug gets its own title', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          item: {'kind': 'status', 'status': 'session_rehydrated'},
        ),
        _envelope(
          eventSeq: 2,
          item: {'kind': 'status', 'status': 'something_new'},
        ),
      ]);
      expect(blocks.single.items.first.title, codingSessionContinuityTitle);
      expect(blocks.single.items.last.title, 'Status');
      expect(blocks.single.items.last.text, 'something_new');
    });

    test('boundary statuses name the protection and its reason', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          item: {
            'kind': 'status',
            'status': 'execution_boundary_enforced',
            'reason': 'macos-seatbelt',
          },
        ),
        _envelope(
          eventSeq: 2,
          item: {
            'kind': 'status',
            'status': 'execution_boundary_not_enforced',
            'reason': 'no-backend-for-platform',
          },
        ),
      ]);
      final items = blocks.single.items;
      expect(items.first.title, codingSessionBoundaryTitle);
      expect(items.first.text, contains('Enforced'));
      expect(items.first.text, contains('(macos-seatbelt)'));
      expect(items.last.title, codingSessionBoundaryTitle);
      expect(items.last.text, startsWith('Not enforced'));
      expect(items.last.text, contains('(no-backend-for-platform)'));
    });

    test('reasoning is folded by default', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(eventSeq: 1, item: {'kind': 'reasoning', 'text': 'thinking'}),
      ]);
      expect(blocks.single.items.single.type, CodingSessionItemType.thought);
      expect(blocks.single.items.single.foldedByDefault, isTrue);
    });

    test('an elided item reports its reason and size only', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          item: {
            'kind': 'elided',
            'reason': 'too_large',
            'byteCount': 99999,
            'contentDigest': 'abc',
          },
        ),
      ]);
      final item = blocks.single.items.single;
      expect(item.title, 'Content elided');
      expect(item.text, contains('too_large'));
      expect(item.text, contains('99999'));
    });

    test('an unknown kind names the kind and surfaces no payload', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          item: {
            'kind': 'wire_transfer',
            'secret': 'do-not-render-me',
            'amount': 1000,
          },
        ),
      ]);
      final item = blocks.single.items.single;
      expect(item.unknownKind, 'wire_transfer');
      expect(item.title, contains('wire_transfer'));
      expect(item.text, isEmpty);
      expect(item.text, isNot(contains('do-not-render-me')));
    });
  });

  group('tool pairing', () {
    test('a result pairs into its pending call within the same stream', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          item: {
            'kind': 'tool_call',
            'tool': {
              'toolName': 'read_file',
              'toolId': 'tool-1',
              'input': {'path': '/tmp/x'},
            },
          },
        ),
        _envelope(
          eventSeq: 2,
          item: {
            'kind': 'tool_result',
            'toolId': 'tool-1',
            'content': 'file body',
            'isError': false,
          },
        ),
      ]);
      final items = blocks.single.items;
      expect(items, hasLength(1));
      expect(items.single.tool!.toolName, 'read_file');
      expect(items.single.tool!.status, CodingSessionToolStatus.completed);
      expect(items.single.tool!.result, 'file body');
      expect(items.single.tool!.argsSummary, contains('path=/tmp/x'));
      expect(items.single.foldedByDefault, isTrue);
    });

    test('an error result marks the paired call failed', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          item: {
            'kind': 'tool_call',
            'tool': {'toolName': 'bash', 'toolId': 'tool-2'},
          },
        ),
        _envelope(
          eventSeq: 2,
          item: {
            'kind': 'tool_result',
            'toolId': 'tool-2',
            'content': 'boom',
            'isError': true,
          },
        ),
      ]);
      expect(
        blocks.single.items.single.tool!.status,
        CodingSessionToolStatus.failed,
      );
    });

    test('an unpaired result renders standalone rather than vanishing', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          item: {
            'kind': 'tool_result',
            'toolId': 'tool-9',
            'toolName': 'grep',
            'content': 'orphan',
          },
        ),
      ]);
      final item = blocks.single.items.single;
      expect(item.type, CodingSessionItemType.tool);
      expect(item.tool!.toolName, 'grep');
      expect(item.tool!.result, 'orphan');
    });

    test('a result never pairs across executions', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          item: {
            'kind': 'tool_call',
            'tool': {'toolName': 'read_file', 'toolId': 'shared'},
          },
        ),
        _envelope(
          eventSeq: 1,
          forTarget: target(sessionId: 'session-2'),
          item: {
            'kind': 'tool_result',
            'toolId': 'shared',
            'content': 'other stream',
          },
        ),
      ]);
      expect(blocks, hasLength(2));
      for (final block in blocks) {
        expect(block.items, hasLength(1));
      }
      final call = blocks
          .firstWhere((block) => block.target.sessionId == 'session-1')
          .items
          .single;
      expect(call.tool!.status, CodingSessionToolStatus.executing);
    });
  });

  group('per-turn usage', () {
    // The `usage` block is additive on the `result` item. This observer must
    // accept a result item carrying it and keep projecting the turn — a strict
    // reader that rejected the item would blank the end of every turn the
    // moment the provider started measuring context.
    test('a result item carrying a usage block still projects', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          turnId: 'turn-1',
          item: {
            'kind': 'result',
            'subtype': 'success',
            'isError': false,
            'durationMs': 1000,
            'result': 'completed',
            'usage': {
              'inputTokens': 1200,
              'outputTokens': 340,
              'cacheReadTokens': 96000,
              'cacheWriteTokens': 4000,
              'toolCalls': 7,
              'contextWindow': 1000000,
            },
          },
        ),
      ]);
      final item = blocks.single.items.single;
      expect(item.type, CodingSessionItemType.lifecycle);
      expect(item.text, 'completed');
      expect(item.result!.durationMs, 1000);
      expect(item.result!.isError, isFalse);
    });

    // A driver's own occupancy item is what the context percentage is built
    // from upstream; this observer must not reject it either.
    test('a context_window_updated item carrying occupancy still projects', () {
      final blocks = projectCodingSessionTranscript([
        _envelope(
          eventSeq: 1,
          turnId: 'turn-1',
          item: {
            'kind': 'context_window_updated',
            'usage': {'size': 1000000, 'used': 137498},
          },
        ),
      ]);
      expect(blocks.single.items.single.title, 'Context window updated');
    });
  });
}
