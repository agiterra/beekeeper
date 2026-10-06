import 'dart:convert';

import 'package:beekeeper/features/project_todos/domain/project_todo_op.dart';
import 'package:beekeeper/features/project_todos/state/project_todo_actions.dart';
import 'package:beekeeper/features/project_todos/state/project_todos_provider.dart';
import 'package:beekeeper/shared/relay/relay.dart';
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

/// The instant every test here runs at.
///
/// The provider used to read `DateTime.now()` directly, so the live
/// filter's `since` floor and every stamped `created_at` could only be
/// asserted within a tolerance — and the five-second window on `since` is
/// what a starved isolate in the full mobile suite broke (item 217).
/// `projectTodoClockProvider` is pinned here instead, so all of them are
/// exact values.
final _now = DateTime.utc(2026, 9, 21, 12, 34, 56);
final _nowSeconds = _now.millisecondsSinceEpoch ~/ 1000;

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
      projectTodoClockProvider.overrideWithValue(() => _now),
    ],
  );
  addTearDown(container.dispose);
  return (container: container, relay: relay);
}

void main() {
  final create = const ProjectTodoOp.listCreate(
    listId: listA,
    visibility: TodoVisibility.project,
    title: 'L',
  );
  final add1 = const ProjectTodoOp.itemAdd(
    listId: listA,
    visibility: TodoVisibility.project,
    itemId: _item1,
    text: 'first',
    rank: 'a0',
  );
  final add2 = const ProjectTodoOp.itemAdd(
    listId: listA,
    visibility: TodoVisibility.project,
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
        expect(live.since, _nowSeconds - 900);
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
              visibility: TodoVisibility.project,
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
            ProjectTodoOp.itemText(
              listId: listA,
              visibility: TodoVisibility.project,
              itemId: _item1,
              text: 'e',
            ),
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
              ProjectTodoOp.itemText(
                listId: listA,
                visibility: TodoVisibility.project,
                itemId: _item1,
                text: 'e',
              ),
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
        ['td-vis', 'project'],
        ['td-item', itemId],
      ]);
      expect(
        event.content,
        '{"schema":"buzz-project-todo/v1","op":"item.add","listId":"$listA",'
        '"itemId":"$itemId","text":"Write the NIP","rank":"a0"}',
      );
      expect(event.createdAt, _nowSeconds);
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
      expect(
        decodeProjectTodoOp(
          h.relay.published.last.content,
          TodoVisibility.project,
        ).rank,
        'a2',
      );

      // Move the second item (index 1) to the top (index 0).
      await actions.moveItem(snapshot(), 1, 0);
      final move = decodeProjectTodoOp(
        h.relay.published.last.content,
        TodoVisibility.project,
      );
      expect(move.kind, ProjectTodoOpKind.itemRank);
      expect(move.itemId, _item2);
      expect(move.rank, 'Zz');
      expect(h.relay.published.last.tags[5], ['td-item', _item2]);

      // Move the first item (index 0) below the second, in
      // ReorderableListView terms (newIndex counts the old slot).
      await actions.moveItem(snapshot(), 0, 2);
      final down = decodeProjectTodoOp(
        h.relay.published.last.content,
        TodoVisibility.project,
      );
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
        final now = _nowSeconds;
        // A peer stamped a done op 300 s ahead of this clock (legal: the relay
        // allows 900 s).
        h.relay.emit(
          _op(
            const ProjectTodoOp.itemDone(
              listId: listA,
              visibility: TodoVisibility.project,
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
          decodeProjectTodoOp(
            h.relay.published.last.content,
            TodoVisibility.project,
          ).done,
          isFalse,
        );

        // A list op is a different target: it is stamped now.
        await actions.retitleList(listA, 'Renamed');
        expect(h.relay.published.last.createdAt, now);

        // Past the window, the bump is refused rather than silently dropped.
        h.relay.emit(
          _op(
            const ProjectTodoOp.itemText(
              listId: listA,
              visibility: TodoVisibility.project,
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
          decodeProjectTodoOp(
            h.relay.published.last.content,
            TodoVisibility.project,
          ).assignee,
          todoViewer,
        );
        // Cleared fields are null on the wire.
        await actions.setDue(listA, _item1, null);
        expect(h.relay.published.last.content, contains('"due":null'));
      },
    );

    test('createList publishes list.create with the chosen visibility, then '
        'list.pinned stamped after it when asked to pin', () async {
      final h = _harness(queryResults: [<NostrEvent>[]]);
      final sub = h.container.listen(
        projectTodosProvider(todoAddress),
        (_, _) {},
      );
      addTearDown(sub.close);
      await _settle();
      final actions = h.container.read(projectTodoActionsProvider(todoAddress));

      final listId = await actions.createList(
        'Mine',
        visibility: TodoVisibility.personal,
        pinned: true,
      );
      expect(isTodoId(listId), isTrue);
      expect(h.relay.published.length, 2);

      final create = h.relay.published[0];
      expect(create.tags, [
        ['a', todoAddress],
        ['td-v', 'td1-1'],
        ['td-op', 'list.create'],
        ['td-list', listId],
        ['td-vis', 'personal'],
      ]);
      expect(
        create.content,
        '{"schema":"buzz-project-todo/v1","op":"list.create",'
        '"listId":"$listId","title":"Mine","visibility":"personal"}',
      );
      expect(create.createdAt, _nowSeconds);
      expect(
        validateProjectTodoEnvelope(create).visibility,
        TodoVisibility.personal,
      );

      final pin = h.relay.published[1];
      expect(pin.tags, [
        ['a', todoAddress],
        ['td-v', 'td1-1'],
        ['td-op', 'list.pinned'],
        ['td-list', listId],
        ['td-vis', 'personal'],
      ]);
      expect(
        pin.content,
        '{"schema":"buzz-project-todo/v1","op":"list.pinned",'
        '"listId":"$listId","pinned":true}',
      );
      // The pin is a field write on the list the create made; it must sort
      // after the create even before the relay echoes either back, or the
      // create's own pinned=false would win the tie on id.
      expect(pin.createdAt, create.createdAt + 1);
      expect(
        validateProjectTodoEnvelope(pin).kind,
        ProjectTodoOpKind.listPinned,
      );

      // Without pinning: one op, project visibility.
      final plain = await actions.createList(
        'Shared',
        visibility: TodoVisibility.project,
      );
      expect(h.relay.published.length, 3);
      expect(h.relay.published.last.tags[3], ['td-list', plain]);
      expect(h.relay.published.last.tags[4], ['td-vis', 'project']);
      expect(
        h.relay.published.last.content,
        contains('"visibility":"project"'),
      );
    });

    test('every op on an existing list repeats that list\'s visibility; '
        'setListPinned publishes list.pinned', () async {
      final personal = const ProjectTodoOp.listCreate(
        listId: listB,
        visibility: TodoVisibility.personal,
        title: 'Mine',
      );
      final h = _harness(
        queryResults: [
          [_op(create, createdAt: 100), _op(personal, createdAt: 101)],
        ],
      );
      final sub = h.container.listen(
        projectTodosProvider(todoAddress),
        (_, _) {},
      );
      addTearDown(sub.close);
      await _settle();
      final actions = h.container.read(projectTodoActionsProvider(todoAddress));

      await actions.setListPinned(listB, true);
      expect(h.relay.published.last.tags[2], ['td-op', 'list.pinned']);
      expect(h.relay.published.last.tags[4], ['td-vis', 'personal']);
      expect(h.relay.published.last.content, contains('"pinned":true'));

      await actions.setListPinned(listA, false);
      expect(h.relay.published.last.tags[4], ['td-vis', 'project']);
      expect(h.relay.published.last.content, contains('"pinned":false'));

      await actions.retitleList(listB, 'Still mine');
      expect(h.relay.published.last.tags[4], ['td-vis', 'personal']);
      final personalList = h.container
          .read(projectTodosProvider(todoAddress))
          .digest
          .listById(listB)!;
      await actions.addItem(personalList, 'secret');
      expect(h.relay.published.last.tags[4], ['td-vis', 'personal']);
      expect(h.relay.published.last.tags[2], ['td-op', 'item.add']);
    });

    test(
      'a write on a list the read does not know is refused, not guessed',
      () async {
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
        final actions = h.container.read(
          projectTodoActionsProvider(todoAddress),
        );
        await expectLater(
          actions.retitleList(listB, 'Nope'),
          throwsA(
            isA<ProjectTodoUnknownListError>().having(
              (e) => e.toString(),
              'message',
              'This list is not in the current read; refresh and try again.',
            ),
          ),
        );
        await expectLater(
          actions.setListPinned(listB, true),
          throwsA(isA<ProjectTodoUnknownListError>()),
        );
        expect(h.relay.published, isEmpty);
      },
    );
  });
}
