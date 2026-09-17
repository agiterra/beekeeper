import 'dart:convert';

import 'package:buzz/features/project_todos/domain/project_todo_op.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

const owner =
    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const coord = '30621:$owner:tank-loop';
const list = '0123456789abcdef0123456789abcdef';
const item = 'fedcba9876543210fedcba9876543210';
const pubkey =
    '1111111111111111111111111111111111111111111111111111111111111111';

NostrEvent _event(
  ProjectTodoOp op, {
  List<List<String>>? tags,
  int kind = 44248,
}) => NostrEvent(
  id: '0' * 64,
  pubkey: pubkey,
  createdAt: 1,
  kind: kind,
  tags: tags ?? op.tags(coord),
  content: op.toContent(),
  sig: '',
);

Matcher _refuses(String fragment) => throwsA(
  isA<FormatException>().having(
    (e) => e.message,
    'message',
    contains(fragment),
  ),
);

void main() {
  final everyOp = <ProjectTodoOp>[
    const ProjectTodoOp.listCreate(listId: list, title: 'Launch'),
    const ProjectTodoOp.listTitle(listId: list, title: 'Launch v2'),
    const ProjectTodoOp.listArchived(listId: list, archived: true),
    const ProjectTodoOp.itemAdd(
      listId: list,
      itemId: item,
      text: 'Write the NIP',
      rank: 'a0',
    ),
    const ProjectTodoOp.itemText(listId: list, itemId: item, text: 'Rewrite'),
    const ProjectTodoOp.itemDone(listId: list, itemId: item, done: true),
    const ProjectTodoOp.itemAssignee(
      listId: list,
      itemId: item,
      assignee: pubkey,
    ),
    const ProjectTodoOp.itemAssignee(
      listId: list,
      itemId: item,
      assignee: null,
    ),
    const ProjectTodoOp.itemDue(listId: list, itemId: item, due: '2026-02-28'),
    const ProjectTodoOp.itemDue(listId: list, itemId: item, due: null),
    const ProjectTodoOp.itemRank(listId: list, itemId: item, rank: 'a0V'),
    const ProjectTodoOp.itemRemove(listId: list, itemId: item),
  ];

  test('every op round-trips through content and passes the envelope', () {
    for (final op in everyOp) {
      final content = op.toContent();
      final keys = (jsonDecode(content) as Map<String, dynamic>).keys.toList();
      expect(keys, op.kind.contentKeys, reason: content);
      expect(decodeProjectTodoOp(content), op, reason: content);
      expect(validateProjectTodoEnvelope(_event(op)), op, reason: content);
      expect(decodeProjectTodoEvent(_event(op), coord), op, reason: content);
    }
  });

  test('tags are a, td-v, td-op, td-list and td-item on item ops only', () {
    expect(everyOp.first.tags(coord), [
      ['a', coord],
      ['td-v', 'td1-1'],
      ['td-op', 'list.create'],
      ['td-list', list],
    ]);
    expect(everyOp[3].tags(coord), [
      ['a', coord],
      ['td-v', 'td1-1'],
      ['td-op', 'item.add'],
      ['td-list', list],
      ['td-item', item],
    ]);
  });

  test('nullable fields are present as null, never absent', () {
    expect(
      everyOp[7].toContent(),
      '{"schema":"buzz-project-todo/v1","op":"item.assignee",'
      '"listId":"$list","itemId":"$item","assignee":null}',
    );
    expect(
      () => decodeProjectTodoOp(
        '{"schema":"buzz-project-todo/v1","op":"item.due",'
        '"listId":"$list","itemId":"$item"}',
      ),
      _refuses('missing field "due"'),
    );
  });

  test('content grammar refusals name the problem', () {
    String content(Map<String, Object?> extra) => jsonEncode({
      'schema': 'buzz-project-todo/v1',
      'op': 'list.create',
      'listId': list,
      'title': 'x',
      ...extra,
    });
    expect(() => decodeProjectTodoOp('{not json'), _refuses('malformed'));
    expect(() => decodeProjectTodoOp('[]'), _refuses('must be an object'));
    expect(
      () => decodeProjectTodoOp(content({'extra': 1})),
      _refuses('unsupported field "extra"'),
    );
    expect(
      () => decodeProjectTodoOp(content({'schema': 'nope'})),
      _refuses('schema'),
    );
    expect(
      () => decodeProjectTodoOp(content({'op': 'list.zap'})),
      _refuses('unknown project todo op'),
    );
    expect(
      () => decodeProjectTodoOp(content({'title': '   '})),
      _refuses('must not be blank'),
    );
    expect(
      () => decodeProjectTodoOp(content({'title': 'a\u0007b'})),
      _refuses('control characters'),
    );
    expect(
      () => decodeProjectTodoOp(content({'title': 'a\tb\nc'})),
      returnsNormally,
    );
    expect(
      () => decodeProjectTodoOp(content({'title': '\u00e9' * 513})),
      _refuses('exceeds 1024 bytes'),
    );
    expect(
      () => decodeProjectTodoOp(content({'title': '\u00e9' * 512})),
      returnsNormally,
    );
    expect(
      () => decodeProjectTodoOp(content({'listId': 'ABCDEF'})),
      _refuses('32 lowercase hex'),
    );
    expect(
      () => decodeProjectTodoOp(content({'title': 'x' * 5000})),
      _refuses('exceeds 4096 bytes'),
    );
  });

  test('item field values are validated', () {
    String content(String op, Map<String, Object?> fields) => jsonEncode({
      'schema': 'buzz-project-todo/v1',
      'op': op,
      'listId': list,
      'itemId': item,
      ...fields,
    });
    expect(
      () => decodeProjectTodoOp(content('item.assignee', {'assignee': 'AB'})),
      _refuses('64-character lowercase hex'),
    );
    expect(
      () =>
          decodeProjectTodoOp(content('item.assignee', {'assignee': 'A' * 64})),
      _refuses('64-character lowercase hex'),
    );
    expect(
      () => decodeProjectTodoOp(content('item.due', {'due': '2026-02-30'})),
      _refuses('not a calendar day'),
    );
    expect(
      () => decodeProjectTodoOp(content('item.due', {'due': '2024-02-29'})),
      returnsNormally,
    );
    expect(
      () => decodeProjectTodoOp(content('item.due', {'due': '2023-02-29'})),
      _refuses('not a calendar day'),
    );
    expect(
      () => decodeProjectTodoOp(content('item.due', {'due': '1969-12-31'})),
      _refuses('1970'),
    );
    expect(
      () => decodeProjectTodoOp(content('item.due', {'due': '26-1-1'})),
      _refuses('YYYY-MM-DD'),
    );
    expect(
      () => decodeProjectTodoOp(content('item.due', {'due': '2026-13-01'})),
      _refuses('month'),
    );
    expect(
      () => decodeProjectTodoOp(content('item.rank', {'rank': 'a0V0'})),
      _refuses('must not end in 0'),
    );
    expect(
      () => decodeProjectTodoOp(content('item.done', {'done': 'yes'})),
      _refuses('must be a boolean'),
    );
  });

  test('the envelope refuses bad tags: h, duplicates, unknown, mismatch', () {
    final op = everyOp[3];
    expect(
      () => validateProjectTodoEnvelope(
        _event(
          op,
          tags: [
            ...op.tags(coord),
            ['h', 'chan'],
          ],
        ),
      ),
      _refuses('must not carry an h tag'),
    );
    expect(
      () => validateProjectTodoEnvelope(
        _event(
          op,
          tags: [
            ...op.tags(coord),
            ['a', coord],
          ],
        ),
      ),
      _refuses('more than one a tag'),
    );
    expect(
      () => validateProjectTodoEnvelope(
        _event(
          op,
          tags: [
            ...op.tags(coord),
            ['x', '1'],
          ],
        ),
      ),
      _refuses('unsupported tag key'),
    );
    expect(
      () => validateProjectTodoEnvelope(
        _event(op, tags: op.tags('30621:${'A' * 64}:tank-loop')),
      ),
      _refuses('canonical'),
    );
    expect(
      () => validateProjectTodoEnvelope(
        _event(
          op,
          tags: op.tags(coord).where((t) => t[0] != 'td-item').toList(),
        ),
      ),
      _refuses('requires one td-item tag'),
    );
    expect(
      () => validateProjectTodoEnvelope(
        _event(
          everyOp.first,
          tags: [
            ...everyOp.first.tags(coord),
            ['td-item', item],
          ],
        ),
      ),
      _refuses('must not carry a td-item tag'),
    );
    expect(
      () => validateProjectTodoEnvelope(
        _event(
          op,
          tags: [
            for (final t in op.tags(coord))
              if (t[0] == 'td-op') ['td-op', 'item.text'] else t,
          ],
        ),
      ),
      _refuses('does not match content op'),
    );
    expect(
      () => validateProjectTodoEnvelope(
        _event(
          op,
          tags: [
            for (final t in op.tags(coord))
              if (t[0] == 'td-v') ['td-v', 'td1-0'] else t,
          ],
        ),
      ),
      _refuses('tag version'),
    );
    expect(
      () => validateProjectTodoEnvelope(_event(op, kind: 1)),
      _refuses('kind 44248'),
    );
  });

  test('the fold decode folds hex case on the coordinate, drops the rest', () {
    final op = everyOp.first;
    expect(
      decodeProjectTodoEvent(
        _event(op, tags: op.tags('30621:${'A' * 64}:tank-loop')),
        coord,
      ),
      op,
    );
    expect(decodeProjectTodoEvent(_event(op, kind: 44240), coord), isNull);
    expect(
      decodeProjectTodoEvent(_event(op), '30621:${'b' * 64}:tank-loop'),
      isNull,
    );
    expect(
      decodeProjectTodoEvent(
        _event(
          op,
          tags: [
            ...op.tags(coord),
            ['a', coord],
          ],
        ),
        coord,
      ),
      isNull,
    );
    expect(
      decodeProjectTodoEvent(
        _event(op, tags: op.tags(coord).where((t) => t[0] != 'a').toList()),
        coord,
      ),
      isNull,
    );
  });

  test('normalizeProjectCoordinate', () {
    expect(normalizeProjectCoordinate(coord), coord);
    expect(
      normalizeProjectCoordinate('30621:${'A' * 64}:x:y'),
      '30621:${'a' * 64}:x:y',
    );
    expect(normalizeProjectCoordinate('30622:${'a' * 64}:x'), isNull);
    expect(normalizeProjectCoordinate('30621:${'a' * 63}:x'), isNull);
    expect(normalizeProjectCoordinate('30621:${'a' * 64}:'), isNull);
    expect(normalizeProjectCoordinate('30621:${'a' * 64}:${'d' * 65}'), isNull);
    expect(normalizeProjectCoordinate('30621:${'a' * 64}:a\nb'), isNull);
    expect(normalizeProjectCoordinate('nope'), isNull);
  });

  test('isTodoId', () {
    expect(isTodoId(list), isTrue);
    expect(isTodoId(list.toUpperCase()), isFalse);
    expect(isTodoId(list.substring(1)), isFalse);
  });
}
