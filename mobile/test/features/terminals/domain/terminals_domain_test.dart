import 'dart:convert';
import 'dart:typed_data';

import 'package:beekeeper/features/terminals/domain/terminals_domain.dart';
import 'package:beekeeper/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

const owner =
    'aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd';
const alice =
    '11111111222222223333333344444444555555556666666677777777dddddddd';
const bob = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
const project = '30621:$owner:beekeeper';

NostrEvent _announce({
  String sessionId = 's1',
  String status = 'open',
  String? title = 'build shell',
  String? dims = '34x120',
  String pubkey = owner,
  int createdAt = 100,
  String id = 'a1',
  List<List<String>> roster = const [],
  String projectRef = project,
}) => NostrEvent(
  id: id,
  pubkey: pubkey,
  createdAt: createdAt,
  kind: 30623,
  tags: [
    ['d', sessionId],
    ['a', projectRef],
    if (title != null) ['title', title],
    ['status', status],
    if (dims != null) ['dims', dims],
    ...roster,
  ],
  content: '',
  sig: '',
);

NostrEvent _frame({
  required String type,
  required int seq,
  String epoch = 'e1',
  String? dims,
  List<int> bytes = const [0x68, 0x69],
  String pubkey = owner,
  String sessionId = 's1',
  String id = 'f',
}) => NostrEvent(
  id: '$id-$seq',
  pubkey: pubkey,
  createdAt: 100,
  kind: 24311,
  tags: [
    ['d', sessionId],
    ['a', project],
    ['t', type],
    ['seq', '$seq'],
    ['epoch', epoch],
    if (dims != null) ['dims', dims],
  ],
  content: base64.encode(bytes),
  sig: '',
);

ShellFrame _parsed(NostrEvent event) =>
    parseShellFrame(event, ownerPubkey: owner, sessionId: 's1')!;

void main() {
  group('announce', () {
    test('parses an open announce with its roster', () {
      final terminal = remoteTerminalFromEvent(
        _announce(
          roster: const [
            ['p', alice, '', 'collaborator'],
            ['p', bob, '', 'viewer'],
          ],
        ),
      )!;
      expect(terminal.title, 'build shell');
      expect(terminal.ownerPubkey, owner);
      expect(terminal.projectRef, project);
      expect(terminal.dims, const ShellDims(rows: 34, cols: 120));
      expect(terminal.roleOf(alice), ShellRosterRole.collaborator);
      expect(terminal.roleOf(bob.toUpperCase()), ShellRosterRole.viewer);
      expect(terminal.mayType(alice), isTrue);
      expect(terminal.mayType(bob), isFalse);
      expect(terminal.mayType(owner), isTrue);
      expect(terminal.mayType(null), isFalse);
    });

    test('skips roster tags the relay would have refused', () {
      final roster = rosterFromAnnounce(
        _announce(
          roster: [
            ['p', alice], // arity 2: no role, not defaulted
            ['p', alice.toUpperCase(), '', 'viewer'], // not lowercase hex
            ['p', bob, '', 'admin'], // unknown role
            ['p', bob, '', 'viewer'],
            ['p', bob, '', 'collaborator'], // duplicate: first wins
          ],
        ),
      );
      expect(roster, [
        const ShellRosterEntry(pubkey: bob, role: ShellRosterRole.viewer),
      ]);
    });

    test(
      'a closed head, a bad address or a missing d reads as no terminal',
      () {
        expect(remoteTerminalFromEvent(_announce(status: 'closed')), isNull);
        expect(
          remoteTerminalFromEvent(_announce(projectRef: '30617:$owner:repo')),
          isNull,
        );
        expect(remoteTerminalFromEvent(_announce(sessionId: '')), isNull);
        expect(
          remoteTerminalFromEvent(_announce(title: null))!.title,
          'terminal',
        );
        expect(remoteTerminalFromEvent(_announce(dims: '0x0'))!.dims, isNull);
      },
    );

    test(
      'the newest head per (owner, d) wins, so a close hides a terminal',
      () {
        final terminals = remoteTerminalsFromEvents([
          _announce(createdAt: 100, id: 'old'),
          _announce(createdAt: 200, id: 'new', status: 'closed'),
          _announce(sessionId: 's2', title: 'alpha', createdAt: 100, id: 'x'),
          _announce(
            sessionId: 's2',
            title: 'alpha',
            pubkey: alice,
            createdAt: 100,
            id: 'y',
          ),
        ]);
        expect(terminals.map((t) => t.key), ['$alice s2', '$owner s2']);
        expect(
          remoteTerminalsFromEvents([
            _announce(sessionId: 's2', pubkey: alice),
          ], excludeOwner: alice.toUpperCase()),
          isEmpty,
        );
        final grouped = terminalsByProject(terminals);
        expect(grouped.keys, [project]);
        expect(grouped[project]!.length, 2);
      },
    );
  });

  group('frame', () {
    test('parses a well-formed frame and refuses the rest', () {
      final frame = _parsed(_frame(type: 'snap', seq: 3, dims: '24x80'));
      expect(frame.type, ShellFrameType.snap);
      expect(frame.seq, 3);
      expect(frame.epoch, 'e1');
      expect(frame.dims, const ShellDims(rows: 24, cols: 80));
      expect(frame.bytes, [0x68, 0x69]);

      expect(
        parseShellFrame(
          _frame(type: 'snap', seq: 1, pubkey: alice),
          ownerPubkey: owner,
          sessionId: 's1',
        ),
        isNull,
        reason: 'another member cannot spoof frames',
      );
      expect(
        parseShellFrame(
          _frame(type: 'snap', seq: 1, sessionId: 's9'),
          ownerPubkey: owner,
          sessionId: 's1',
        ),
        isNull,
      );
      expect(
        parseShellFrame(
          _frame(type: 'blit', seq: 1),
          ownerPubkey: owner,
          sessionId: 's1',
        ),
        isNull,
      );
      final badBase64 = NostrEvent(
        id: 'b',
        pubkey: owner,
        createdAt: 1,
        kind: 24311,
        tags: const [
          ['d', 's1'],
          ['a', project],
          ['t', 'diff'],
          ['seq', '1'],
          ['epoch', 'e1'],
        ],
        content: '!!!',
        sig: '',
      );
      expect(
        parseShellFrame(badBase64, ownerPubkey: owner, sessionId: 's1'),
        isNull,
      );
    });
  });

  group('ObserveStream', () {
    // Mirrors desktop/src/features/builtin-shell/observe/shellObserveProtocol.
    test('attach: tails write, the first snap paints without a clear', () {
      final stream = ObserveStream();
      final tail = stream.apply(_parsed(_frame(type: 'tail', seq: 1)));
      expect(tail.write, [0x68, 0x69]);
      final snap = stream.apply(
        _parsed(_frame(type: 'snap', seq: 2, dims: '24x80')),
      );
      expect(snap.write, [0x68, 0x69]);
      expect(snap.resize, const ShellDims(rows: 24, cols: 80));
      final diff = stream.apply(_parsed(_frame(type: 'diff', seq: 3)));
      expect(diff.write, [0x68, 0x69]);
      expect(diff.needsResync, isFalse);
    });

    test('a seq gap suppresses diffs and asks for a resync; the next snap '
        'clears first', () {
      final stream = ObserveStream();
      stream.apply(_parsed(_frame(type: 'snap', seq: 1)));
      final gapped = stream.apply(_parsed(_frame(type: 'diff', seq: 5)));
      expect(gapped.write, isNull);
      expect(gapped.needsResync, isTrue);
      // A stale tail mid-stream is refused too.
      expect(
        stream.apply(_parsed(_frame(type: 'tail', seq: 6))).needsResync,
        isTrue,
      );
      final repaint = stream.apply(_parsed(_frame(type: 'snap', seq: 7)));
      expect(
        repaint.write!.sublist(0, shellClearSequence.length),
        shellClearSequence,
      );
      expect(repaint.write!.sublist(shellClearSequence.length), [0x68, 0x69]);
      expect(
        stream.apply(_parsed(_frame(type: 'diff', seq: 8))).write,
        isNotNull,
      );
    });

    test('a stale replay is dropped silently', () {
      final stream = ObserveStream();
      stream.apply(_parsed(_frame(type: 'snap', seq: 5)));
      final replay = stream.apply(_parsed(_frame(type: 'diff', seq: 3)));
      expect(replay, same(ObserveAction.none));
    });

    test('an epoch change means a restarted owner: wait for its snap', () {
      final stream = ObserveStream();
      stream.apply(_parsed(_frame(type: 'snap', seq: 1, epoch: 'e1')));
      final restarted = stream.apply(
        _parsed(_frame(type: 'diff', seq: 1, epoch: 'e2')),
      );
      expect(restarted.needsResync, isTrue);
      final snap = stream.apply(
        _parsed(_frame(type: 'snap', seq: 2, epoch: 'e2')),
      );
      expect(snap.write!.length, shellClearSequence.length + 2);
    });

    test('resize invalidates the diff chain; end ends', () {
      final stream = ObserveStream();
      stream.apply(_parsed(_frame(type: 'snap', seq: 1)));
      final resized = stream.apply(
        _parsed(_frame(type: 'resize', seq: 2, dims: '40x100')),
      );
      expect(resized.resize, const ShellDims(rows: 40, cols: 100));
      expect(
        stream.apply(_parsed(_frame(type: 'diff', seq: 3))).needsResync,
        isTrue,
      );
      expect(stream.apply(_parsed(_frame(type: 'end', seq: 4))).ended, isTrue);
    });
  });

  group('ShellByteDecoder', () {
    test('carries a multibyte glyph split across two frames', () {
      final decoder = ShellByteDecoder();
      final euro = utf8.encode('€'); // e2 82 ac
      final first = decoder.decode(Uint8List.fromList([0x61, euro[0]]));
      final second = decoder.decode(Uint8List.fromList([euro[1], euro[2]]));
      expect(first, 'a');
      expect(second, '€');
    });

    test('a malformed run is not held back forever', () {
      expect(incompleteUtf8Tail(Uint8List.fromList([0x61, 0x80, 0x80])), 0);
      expect(incompleteUtf8Tail(Uint8List.fromList([0xf0, 0x9f])), 2);
      expect(incompleteUtf8Tail(Uint8List.fromList([0xe2, 0x82, 0xac])), 0);
      final decoder = ShellByteDecoder();
      expect(decoder.decode(Uint8List.fromList([0x61, 0x80])), 'a\uFFFD');
    });

    test('reset drops a partial sequence', () {
      final decoder = ShellByteDecoder();
      decoder.decode(Uint8List.fromList([0xe2]));
      decoder.reset();
      expect(decoder.decode(Uint8List.fromList([0x62])), 'b');
    });
  });

  group('events this device signs', () {
    test('a watch names the owner, session and project', () {
      final event = buildShellWatchEvent(
        ownerPubkey: owner.toUpperCase(),
        sessionId: 's1',
        projectRef: project,
        action: ShellWatchAction.resync,
      );
      expect(event.kind, 24310);
      expect(event.content, '{"action":"resync"}');
      expect(event.tags, [
        ['p', owner],
        ['d', 's1'],
        ['a', project],
      ]);
    });

    test('input is base64, chunked under the 8 KiB cap, in order', () {
      final bytes = Uint8List.fromList(
        List<int>.generate(13000, (i) => i % 251),
      );
      final chunks = chunkShellInput(bytes);
      expect(chunks.map((c) => c.length), [6000, 6000, 1000]);
      expect(chunks[2].first, bytes[12000]);
      for (final chunk in chunks) {
        final event = buildShellInputEvent(
          ownerPubkey: owner,
          sessionId: 's1',
          projectRef: project,
          bytes: chunk,
        );
        expect(event.kind, 24312);
        expect(event.content.length, lessThanOrEqualTo(8 * 1024));
        expect(base64.decode(event.content), chunk);
      }
      expect(chunkShellInput(Uint8List(0)), isEmpty);
      expect(
        () => buildShellInputEvent(
          ownerPubkey: owner,
          sessionId: 's1',
          projectRef: project,
          bytes: Uint8List(7000),
        ),
        throwsArgumentError,
      );
    });
  });
}
