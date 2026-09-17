import 'dart:convert';

import 'package:buzz/features/project_todos/domain/project_todo_op.dart';
import 'package:buzz/features/project_todos/state/project_todo_actions.dart';
import 'package:buzz/features/project_todos/state/project_todos_provider.dart';
import 'package:buzz/shared/relay/relay.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:nostr/nostr.dart' as nostr;

import '../../../helpers/recording_relay_session.dart';
import '../fake_project_todos.dart';

const _item1 = '11111111111111111111111111111111';
const _item2 = '22222222222222222222222222222222';

int _counter = 0;

/// A signed-looking op event with a unique id.
NostrEvent _op(
  ProjectTodoOp op, {
  required int createdAt,
  String pubkey = todoOwner,
  String? id,
}) {
  _counter++;
  return NostrEvent(
    id: id ?? _counter.toRadixString(16).padLeft(64, '0'),
    pubkey: pubkey,
    createdAt: createdAt,
    kind: EventKind.projectTodoOp,
    tags: op.tags(todoAddress),
    content: op.toContent(),
    sig: '',
  );
}

class _FakeConfig extends RelayConfigNotifier {
  final String nsec;
  _FakeConfig(this.nsec);

  @override
  RelayConfig build() =>
      RelayConfig(baseUrl: 'https://relay.example', nsec: nsec);
}

Future<void> _settle([int rounds = 8]) async {
  for (var i = 0; i < rounds; i++) {
    await Future<void>.delayed(Duration.zero);
  }
}

({ProviderContainer container, RecordingRelaySessionNotifier relay}) _harness({
  List<Object> queryResults = const [],
  List<Object> publishResults = const [],
  bool failSubscribe = false,
}) {
  final relay = RecordingRelaySessionNotifier(
    queryResults: queryResults,
    publishResults: publishResults,
    failSubscribe: failSubscribe,
  );
  final container = ProviderContainer(
    overrides: [
      relaySessionProvider.overrideWith(() => relay),
      relayConfigProvider.overrideWith(
        () => _FakeConfig(nostr.Keys.generate().nsec),
      ),
    ],
  );
  addTearDown(container.dispose);
  return (container: container, relay: relay);
}

void main() {
  final create = const ProjectTodoOp.listCreate(listId: listA, title: 'L');
  final add1 = const ProjectTodoOp.itemAdd(
    listId: listA,
    itemId: _item1,
    text: 'first',
    rank: 'a0',
  );
  final add2 = const ProjectTodoOp.itemAdd(
    listId: listA,
    itemId: _item2,
    text: 'second',
    rank: 'a1',
  );

  group('read', () {
    test(
      'subscribes live first (since now-900), then reads one page',
      () async {
        final h = _harness(
          queryResults: [
            [_op(add1, createdAt: 101), _op(create, createdAt: 100)],
          ],
        );
        final before = DateTime.now().millisecondsSinceEpoch ~/ 1000;
        final sub = h.container.listen(
          projectTodosProvider(todoAddress),
          (_, _) {},
        );
        addTearDown(sub.close);
        expect(
          h.container.read(projectTodosProvider(todoAddress)).loading,
          isTrue,
        );
        await _settle();

        expect(h.relay.operations.first, 'subscribe');
        expect(h.relay.operations, ['subscribe', 'query1']);
        final live = h.relay.liveFilters.single;
        expect(live.kinds, [44248]);
        expect(live.tags, {
          '#a': [todoAddress],
        });
        expect(live.since, isNotNull);
        expect(live.since! <= before - 900, isTrue);
        expect(live.since! >= before - 900 - 5, isTrue);
        expect(live.until, isNull);

        final page = h.relay.coalescedQueryFilters.single;
        expect(page.kinds, [44248]);
        expect(page.tags, {
          '#a': [todoAddress],
        });
        expect(page.limit, projectTodoHistoryPageLimit);
        expect(page.until, isNull);
        expect(page.since, isNull);

        final read = h.container.read(projectTodosProvider(todoAddress));
        expect(read.loading, isFalse);
        expect(read.hasRead, isTrue);
        expect(read.truncated, isFalse);
        expect(read.error, isNull);
        expect(read.digest.lists.single.title, 'L');
        expect(read.digest.lists.single.open.single.text, 'first');
      },
    );

    test('walks older pages with until until a short page', () async {
      // Page one is full: 500 ops stamped 1000..1499 (newest first, as the
      // relay sends them). Page two is short.
      final full = [
        for (var i = 0; i < projectTodoHistoryPageLimit; i++)
          _op(
            ProjectTodoOp.itemText(
              listId: listA,
              itemId: _item1,
              text: 'edit $i',
            ),
            createdAt: 1499 - i,
          ),
      ];
      final h = _harness(
        queryResults: [
          full,
          [
            // The boundary second again (deduped by id) plus the creates.
            full.last,
            _op(add1, createdAt: 10),
            _op(create, createdAt: 9),
          ],
        ],
      );
      final sub = h.container.listen(
        projectTodosProvider(todoAddress),
        (_, _) {},
      );
      addTearDown(sub.close);
      await _settle(12);

      expect(h.relay.coalescedQueryFilters.length, 2);
      expect(h.relay.coalescedQueryFilters[0].until, isNull);
      expect(h.relay.coalescedQueryFilters[1].until, 1000);
      final read = h.container.read(projectTodosProvider(todoAddress));
      expect(read.truncated, isFalse);
      expect(read.digest.ignored, 0);
      expect(read.digest.lists.single.open.single.text, 'edit 0');
      expect(
        h.container
            .read(projectTodosProvider(todoAddress).notifier)
            .latestSeenFor(listA, _item1),
        1499,
      );
      expect(
        h.container
            .read(projectTodosProvider(todoAddress).notifier)
            .latestSeenFor(listA),
        9,
      );
    });

    test('stops after the page cap and says the read is truncated', () async {
      List<NostrEvent> page(int n) => [
        for (var i = 0; i < projectTodoHistoryPageLimit; i++)
          _op(
            ProjectTodoOp.itemText(listId: listA, itemId: _item1, text: 'e'),
            createdAt: 100000 - n * 1000 - i,
          ),
      ];
      final h = _harness(
        queryResults: [
          for (var n = 0; n < projectTodoHistoryMaxPages + 2; n++) page(n),
        ],
      );
      final sub = h.container.listen(
        projectTodosProvider(todoAddress),
        (_, _) {},
      );
      addTearDown(sub.close);
      await _settle(40);

      expect(h.relay.coalescedQueryFilters.length, projectTodoHistoryMaxPages);
      final read = h.container.read(projectTodosProvider(todoAddress));
      expect(read.truncated, isTrue);
      expect(read.loading, isFalse);
      expect(read.hasRead, isTrue);
    });

    test(
      'a full page that adds nothing new is disclosed, not looped',
      () async {
        final same = [
          for (var i = 0; i < projectTodoHistoryPageLimit; i++)
            _op(
              ProjectTodoOp.itemText(listId: listA, itemId: _item1, text: 'e'),
              createdAt: 500,
            ),
        ];
        final h = _harness(queryResults: [same, same, same]);
        final sub = h.container.listen(
          projectTodosProvider(todoAddress),
          (_, _) {},
        );
        addTearDown(sub.close);
        await _settle(12);
        expect(h.relay.coalescedQueryFilters.length, 2);
        expect(
          h.container.read(projectTodosProvider(todoAddress)).truncated,
          isTrue,
        );
      },
    );

    test('live ops fold in, deduped against history and each other', () async {
      final createEvent = _op(create, createdAt: 100);
      final h = _harness(
        queryResults: [
          [createEvent],
        ],
      );
      final states = <ProjectTodosRead>[];
      final sub = h.container.listen(
        projectTodosProvider(todoAddress),
        (_, next) => states.add(next),
      );
      addTearDown(sub.close);
      await _settle();
      expect(
        h.container
            .read(projectTodosProvider(todoAddress))
            .digest
            .lists
            .single
            .open,
        isEmpty,
      );

      final folds = states.length;
      h.relay.emit(createEvent); // duplicate of history: no fold
      h.relay.emit(_op(add1, createdAt: 101));
      h.relay.emit(_op(add2, createdAt: 102));
      await _settle();
      // One microtask-coalesced fold for the burst.
      expect(states.length, folds + 1);
      final open = h.container
          .read(projectTodosProvider(todoAddress))
          .digest
          .lists
          .single
          .open;
      expect(open.map((i) => i.text), ['first', 'second']);
      expect(
        h.container
            .read(projectTodosProvider(todoAddress).notifier)
            .latestSeenFor(listA, _item2),
        102,
      );
    });

    test('a failed subscribe is disclosed and history still reads', () async {
      final h = _harness(
        failSubscribe: true,
        queryResults: [
          [_op(create, createdAt: 100)],
        ],
      );
      final sub = h.container.listen(
        projectTodosProvider(todoAddress),
        (_, _) {},
      );
      addTearDown(sub.close);
      await _settle();
      final read = h.container.read(projectTodosProvider(todoAddress));
      expect(read.error, contains('subscription failed'));
      expect(read.digest.lists.single.title, 'L');
      expect(read.hasRead, isTrue);
    });

    test('a dropped socket is disclosed over what was read', () async {
      final h = _harness(
        queryResults: [
          [_op(create, createdAt: 100)],
        ],
      );
      final sub = h.container.listen(
        projectTodosProvider(todoAddress),
        (_, _) {},
      );
      addTearDown(sub.close);
      await _settle();
      h.relay.setConnected(false);
      await _settle();
      final read = h.container.read(projectTodosProvider(todoAddress));
      expect(read.error, projectTodosDisconnectedError);
      expect(read.loading, isFalse);
      expect(read.digest.lists.single.title, 'L');
      expect(read.hasRead, isTrue);

      // Back up: the read restarts, the disclosure clears.
      h.relay.setConnected(true);
      await _settle();
      expect(h.relay.liveFilters.length, 2);
    });

    test('a failed history read is disclosed verbatim', () async {
      final h = _harness(queryResults: [Exception('rate limited: slow down')]);
      final sub = h.container.listen(
        projectTodosProvider(todoAddress),
        (_, _) {},
      );
      addTearDown(sub.close);
      await _settle();
      final read = h.container.read(projectTodosProvider(todoAddress));
      expect(
        read.error,
        'To-do history read failed: Exception: rate limited: slow down',
      );
      expect(read.loading, isFalse);
    });
  });

  group('actions', () {
    test('publishes exactly the contract tags and content, signed', () async {
      final h = _harness(
        queryResults: [
          [_op(create, createdAt: 100)],
        ],
      );
      final sub = h.container.listen(
        projectTodosProvider(todoAddress),
        (_, _) {},
      );
      addTearDown(sub.close);
      await _settle();

      final actions = h.container.read(projectTodoActionsProvider(todoAddress));
      final list = h.container
          .read(projectTodosProvider(todoAddress))
          .digest
          .lists
          .single;
      final before = DateTime.now().millisecondsSinceEpoch ~/ 1000;
      final itemId = await actions.addItem(list, 'Write the NIP');
      expect(isTodoId(itemId), isTrue);

      final event = h.relay.published.single;
      expect(event.kind, 44248);
      expect(event.pubkey.length, 64);
      expect(event.sig.isNotEmpty, isTrue);
      expect(event.tags, [
        ['a', todoAddress],
        ['td-v', 'td1-1'],
        ['td-op', 'item.add'],
        ['td-list', listA],
        ['td-item', itemId],
      ]);
      expect(
        event.content,
        '{"schema":"buzz-project-todo/v1","op":"item.add","listId":"$listA",'
        '"itemId":"$itemId","text":"Write the NIP","rank":"a0"}',
      );
      expect(event.createdAt >= before, isTrue);
      expect(event.createdAt <= before + 2, isTrue);
      // What we send is what the relay's validator accepts.
      expect(
        validateProjectTodoEnvelope(event).kind,
        ProjectTodoOpKind.itemAdd,
      );
      expect(jsonDecode(event.content), isA<Map<String, dynamic>>());
    });

    test('an add lands after the last open item; a move mints between '
        'its new neighbours', () async {
      final h = _harness(
        queryResults: [
          [
            _op(create, createdAt: 100),
            _op(add1, createdAt: 101),
            _op(add2, createdAt: 102),
          ],
        ],
      );
      final sub = h.container.listen(
        projectTodosProvider(todoAddress),
        (_, _) {},
      );
      addTearDown(sub.close);
      await _settle();
      final actions = h.container.read(projectTodoActionsProvider(todoAddress));
      snapshot() => h.container
          .read(projectTodosProvider(todoAddress))
          .digest
          .lists
          .single;

      await actions.addItem(snapshot(), 'third');
      expect(decodeProjectTodoOp(h.relay.published.last.content).rank, 'a2');

      // Move the second item (index 1) to the top (index 0).
      await actions.moveItem(snapshot(), 1, 0);
      final move = decodeProjectTodoOp(h.relay.published.last.content);
      expect(move.kind, ProjectTodoOpKind.itemRank);
      expect(move.itemId, _item2);
      expect(move.rank, 'Zz');
      expect(h.relay.published.last.tags[4], ['td-item', _item2]);

      // Move the first item (index 0) below the second, in
      // ReorderableListView terms (newIndex counts the old slot).
      await actions.moveItem(snapshot(), 0, 2);
      final down = decodeProjectTodoOp(h.relay.published.last.content);
      expect(down.itemId, _item1);
      expect(down.rank, 'a2');

      // A no-op move publishes nothing.
      final count = h.relay.published.length;
      await actions.moveItem(snapshot(), 1, 2);
      expect(h.relay.published.length, count);
    });

    test(
      'stamps created_at past the latest seen op on the same target',
      () async {
        final h = _harness(
          queryResults: [
            [_op(create, createdAt: 100), _op(add1, createdAt: 101)],
          ],
        );
        final sub = h.container.listen(
          projectTodosProvider(todoAddress),
          (_, _) {},
        );
        addTearDown(sub.close);
        await _settle();
        final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
        // A peer stamped a done op 300 s ahead of this clock (legal: the relay
        // allows 900 s).
        h.relay.emit(
          _op(
            const ProjectTodoOp.itemDone(
              listId: listA,
              itemId: _item1,
              done: true,
            ),
            createdAt: now + 300,
            pubkey: todoViewer,
          ),
        );
        await _settle();
        final actions = h.container.read(
          projectTodoActionsProvider(todoAddress),
        );
        await actions.setDone(listA, _item1, false);
        expect(h.relay.published.last.createdAt, now + 301);
        expect(
          decodeProjectTodoOp(h.relay.published.last.content).done,
          isFalse,
        );

        // A list op is a different target: it is stamped now.
        await actions.retitleList(listA, 'Renamed');
        expect(h.relay.published.last.createdAt >= now, isTrue);
        expect(h.relay.published.last.createdAt <= now + 2, isTrue);

        // Past the window, the bump is refused rather than silently dropped.
        h.relay.emit(
          _op(
            const ProjectTodoOp.itemText(
              listId: listA,
              itemId: _item1,
              text: 'x',
            ),
            createdAt: now + 901,
            pubkey: todoViewer,
          ),
        );
        await _settle();
        final count = h.relay.published.length;
        await expectLater(
          actions.setText(listA, _item1, 'y'),
          throwsA(isA<ProjectTodoClockError>()),
        );
        expect(h.relay.published.length, count);
      },
    );

    test(
      'a relay refusal surfaces verbatim; a bad value is refused locally',
      () async {
        final h = _harness(
          queryResults: [
            [_op(create, createdAt: 100)],
          ],
          publishResults: [
            Exception('restricted: viewers may not write to-dos'),
          ],
        );
        final sub = h.container.listen(
          projectTodosProvider(todoAddress),
          (_, _) {},
        );
        addTearDown(sub.close);
        await _settle();
        final actions = h.container.read(
          projectTodoActionsProvider(todoAddress),
        );
        await expectLater(
          actions.retitleList(listA, 'Nope'),
          throwsA(
            predicate<Object>(
              (e) =>
                  e.toString() ==
                  'Exception: restricted: viewers may not write to-dos',
            ),
          ),
        );
        expect(h.relay.published.length, 1);

        await expectLater(
          actions.retitleList(listA, '   '),
          throwsA(
            isA<FormatException>().having(
              (e) => e.message,
              'message',
              'todo title must not be blank',
            ),
          ),
        );
        await expectLater(
          actions.setDue(listA, _item1, '2026-02-30'),
          throwsFormatException,
        );
        await expectLater(
          actions.setAssignee(listA, _item1, 'ABC'),
          throwsFormatException,
        );
        // Nothing malformed reached the relay.
        expect(h.relay.published.length, 1);

        // An assignee's hex case is folded to the wire's lowercase.
        await actions.setAssignee(listA, _item1, todoViewer.toUpperCase());
        expect(
          decodeProjectTodoOp(h.relay.published.last.content).assignee,
          todoViewer,
        );
        // Cleared fields are null on the wire.
        await actions.setDue(listA, _item1, null);
        expect(h.relay.published.last.content, contains('"due":null'));
      },
    );
  });
}
