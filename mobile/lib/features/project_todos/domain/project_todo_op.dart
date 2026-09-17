import 'dart:convert';

import 'package:flutter/foundation.dart';

import '../../../shared/relay/nostr_models.dart';
import 'fractional_rank.dart';

/// Project to-do operations (kind 44248, NIP-TD): one field-level edit to a
/// shared, project-scoped to-do list.
///
/// A port of `crates/buzz-core/src/project_todo.rs`, the single wire
/// validator: the op vocabulary, the closed content key set, the tag
/// grammar. The fold that turns a bag of ops into lists lives in
/// `project_todo_fold.dart`. Every op sets exactly one thing, so two people
/// editing different fields of one item never race.

/// Exact `schema` value carried by kind 44248 content.
const projectTodoSchema = 'buzz-project-todo/v1';

/// Exact version carried by the `td-v` tag.
const projectTodoTagVersion = 'td1-1';

/// Maximum UTF-8 byte length of a complete op payload.
const maxProjectTodoContentBytes = 4 * 1024;

/// Maximum UTF-8 byte length of a list title or an item's text.
const maxProjectTodoTextBytes = 1024;

/// What one op does. [wire] is the `td-op` tag and the content `op` field.
enum ProjectTodoOpKind {
  /// Bring a list into existence with a title.
  listCreate('list.create', ['schema', 'op', 'listId', 'title']),

  /// Retitle a list.
  listTitle('list.title', ['schema', 'op', 'listId', 'title']),

  /// Archive or unarchive a list.
  listArchived('list.archived', ['schema', 'op', 'listId', 'archived']),

  /// Bring an item into existence with its text and its rank.
  itemAdd('item.add', ['schema', 'op', 'listId', 'itemId', 'text', 'rank']),

  /// Rewrite an item's text.
  itemText('item.text', ['schema', 'op', 'listId', 'itemId', 'text']),

  /// Mark an item done or not done.
  itemDone('item.done', ['schema', 'op', 'listId', 'itemId', 'done']),

  /// Assign an item to a pubkey, or clear the assignee.
  itemAssignee('item.assignee', [
    'schema',
    'op',
    'listId',
    'itemId',
    'assignee',
  ]),

  /// Set or clear an item's due date.
  itemDue('item.due', ['schema', 'op', 'listId', 'itemId', 'due']),

  /// Move an item by giving it a new rank.
  itemRank('item.rank', ['schema', 'op', 'listId', 'itemId', 'rank']),

  /// Remove an item for good.
  itemRemove('item.remove', ['schema', 'op', 'listId', 'itemId']);

  const ProjectTodoOpKind(this.wire, this.contentKeys);

  /// The wire spelling, identical in the `td-op` tag and in content.
  final String wire;

  /// The exact content key set for this op, in canonical order. Absent is
  /// not null: a nullable field (`assignee`, `due`) must be present.
  final List<String> contentKeys;

  /// `true` for the ops that name an item and therefore carry `itemId` in
  /// content and a `td-item` tag.
  bool get isItemOp => contentKeys.contains('itemId');

  /// The kind spelled [wire], or `null` for an unknown op.
  static ProjectTodoOpKind? fromWire(String wire) {
    for (final kind in values) {
      if (kind.wire == wire) return kind;
    }
    return null;
  }
}

/// One decoded op: the list it names and what it sets. Only the fields the
/// op's [kind] carries are non-null, except `assignee` and `due`, which are
/// legitimately `null` on their own ops when cleared.
@immutable
class ProjectTodoOp {
  final ProjectTodoOpKind kind;

  /// The list (32 lowercase hex).
  final String listId;

  /// The item (32 lowercase hex), on item ops.
  final String? itemId;
  final String? title;
  final bool? archived;
  final String? text;
  final String? rank;
  final bool? done;
  final String? assignee;
  final String? due;

  const ProjectTodoOp._({
    required this.kind,
    required this.listId,
    this.itemId,
    this.title,
    this.archived,
    this.text,
    this.rank,
    this.done,
    this.assignee,
    this.due,
  });

  const ProjectTodoOp.listCreate({
    required String listId,
    required String title,
  }) : this._(kind: ProjectTodoOpKind.listCreate, listId: listId, title: title);

  const ProjectTodoOp.listTitle({required String listId, required String title})
    : this._(kind: ProjectTodoOpKind.listTitle, listId: listId, title: title);

  const ProjectTodoOp.listArchived({
    required String listId,
    required bool archived,
  }) : this._(
         kind: ProjectTodoOpKind.listArchived,
         listId: listId,
         archived: archived,
       );

  const ProjectTodoOp.itemAdd({
    required String listId,
    required String itemId,
    required String text,
    required String rank,
  }) : this._(
         kind: ProjectTodoOpKind.itemAdd,
         listId: listId,
         itemId: itemId,
         text: text,
         rank: rank,
       );

  const ProjectTodoOp.itemText({
    required String listId,
    required String itemId,
    required String text,
  }) : this._(
         kind: ProjectTodoOpKind.itemText,
         listId: listId,
         itemId: itemId,
         text: text,
       );

  const ProjectTodoOp.itemDone({
    required String listId,
    required String itemId,
    required bool done,
  }) : this._(
         kind: ProjectTodoOpKind.itemDone,
         listId: listId,
         itemId: itemId,
         done: done,
       );

  const ProjectTodoOp.itemAssignee({
    required String listId,
    required String itemId,
    required String? assignee,
  }) : this._(
         kind: ProjectTodoOpKind.itemAssignee,
         listId: listId,
         itemId: itemId,
         assignee: assignee,
       );

  const ProjectTodoOp.itemDue({
    required String listId,
    required String itemId,
    required String? due,
  }) : this._(
         kind: ProjectTodoOpKind.itemDue,
         listId: listId,
         itemId: itemId,
         due: due,
       );

  const ProjectTodoOp.itemRank({
    required String listId,
    required String itemId,
    required String rank,
  }) : this._(
         kind: ProjectTodoOpKind.itemRank,
         listId: listId,
         itemId: itemId,
         rank: rank,
       );

  const ProjectTodoOp.itemRemove({
    required String listId,
    required String itemId,
  }) : this._(
         kind: ProjectTodoOpKind.itemRemove,
         listId: listId,
         itemId: itemId,
       );

  /// Encode as canonical content JSON: keys in [ProjectTodoOpKind.contentKeys]
  /// order, no whitespace. The inverse of [decodeProjectTodoOp].
  String toContent() {
    final object = <String, Object?>{
      'schema': projectTodoSchema,
      'op': kind.wire,
      'listId': listId,
    };
    if (kind.isItemOp) object['itemId'] = itemId;
    switch (kind) {
      case ProjectTodoOpKind.listCreate:
      case ProjectTodoOpKind.listTitle:
        object['title'] = title;
      case ProjectTodoOpKind.listArchived:
        object['archived'] = archived;
      case ProjectTodoOpKind.itemAdd:
        object['text'] = text;
        object['rank'] = rank;
      case ProjectTodoOpKind.itemText:
        object['text'] = text;
      case ProjectTodoOpKind.itemDone:
        object['done'] = done;
      case ProjectTodoOpKind.itemAssignee:
        object['assignee'] = assignee;
      case ProjectTodoOpKind.itemDue:
        object['due'] = due;
      case ProjectTodoOpKind.itemRank:
        object['rank'] = rank;
      case ProjectTodoOpKind.itemRemove:
        break;
    }
    return jsonEncode(object);
  }

  /// The tags this op carries, in canonical order: `a`, `td-v`, `td-op`,
  /// `td-list`, and `td-item` for item ops.
  List<List<String>> tags(String coordinate) => [
    ['a', coordinate],
    ['td-v', projectTodoTagVersion],
    ['td-op', kind.wire],
    ['td-list', listId],
    if (kind.isItemOp) ['td-item', itemId!],
  ];

  @override
  bool operator ==(Object other) =>
      other is ProjectTodoOp &&
      other.kind == kind &&
      other.listId == listId &&
      other.itemId == itemId &&
      other.title == title &&
      other.archived == archived &&
      other.text == text &&
      other.rank == rank &&
      other.done == done &&
      other.assignee == assignee &&
      other.due == due;

  @override
  int get hashCode => Object.hash(
    kind,
    listId,
    itemId,
    title,
    archived,
    text,
    rank,
    done,
    assignee,
    due,
  );

  @override
  String toString() => 'ProjectTodoOp(${toContent()})';
}

final _lowerHex = RegExp(r'^[0-9a-f]*$');

/// `true` for a 32-character lowercase hex id (a list or item id).
bool isTodoId(String value) => value.length == 32 && _lowerHex.hasMatch(value);

bool _isPubkeyHex(String value) =>
    value.length == 64 && _lowerHex.hasMatch(value);

void _validateId(String field, String value) {
  if (!isTodoId(value)) {
    throw FormatException('todo $field must be 32 lowercase hex characters');
  }
}

bool _isControl(int rune) => rune < 0x20 || (rune >= 0x7f && rune <= 0x9f);

/// Validate a list title or an item's text: non-blank, within
/// [maxProjectTodoTextBytes], no control characters other than newline and
/// tab.
void validateTodoText(String field, String value) {
  if (value.trim().isEmpty) {
    throw FormatException('todo $field must not be blank');
  }
  if (utf8.encode(value).length > maxProjectTodoTextBytes) {
    throw FormatException('todo $field exceeds $maxProjectTodoTextBytes bytes');
  }
  for (final rune in value.runes) {
    if (_isControl(rune) && rune != 0x0a && rune != 0x09) {
      throw FormatException('todo $field must not contain control characters');
    }
  }
}

/// Validate a due date: `YYYY-MM-DD`, a real calendar date, year 1970–9999.
void validateDueDate(String value) {
  final units = value.codeUnits;
  bool digit(int i) => units[i] >= 0x30 && units[i] <= 0x39;
  final wellFormed =
      units.length == 10 &&
      units[4] == 0x2d &&
      units[7] == 0x2d &&
      [0, 1, 2, 3, 5, 6, 8, 9].every(digit);
  if (!wellFormed) {
    throw const FormatException('todo due date must be YYYY-MM-DD');
  }
  final year = int.parse(value.substring(0, 4));
  final month = int.parse(value.substring(5, 7));
  final day = int.parse(value.substring(8, 10));
  if (year < 1970 || year > 9999) {
    throw const FormatException('todo due year must be 1970–9999');
  }
  final leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
  final daysInMonth = switch (month) {
    1 || 3 || 5 || 7 || 8 || 10 || 12 => 31,
    4 || 6 || 9 || 11 => 30,
    2 => leap ? 29 : 28,
    _ => throw const FormatException('todo due month must be 01–12'),
  };
  if (day == 0 || day > daysInMonth) {
    throw const FormatException(
      'todo due day is not a calendar day of that month',
    );
  }
}

String _takeString(Map<String, Object?> object, String key) {
  final value = object[key];
  if (value is String) return value;
  throw FormatException('todo op $key must be a string');
}

bool _takeBool(Map<String, Object?> object, String key) {
  final value = object[key];
  if (value is bool) return value;
  throw FormatException('todo op $key must be a boolean');
}

String? _takeNullableString(Map<String, Object?> object, String key) {
  final value = object[key];
  if (value == null) return null;
  if (value is String) return value;
  throw FormatException('todo op $key must be a string or null');
}

/// Strictly decode and validate op content. Throws a [FormatException]
/// naming the first problem.
///
/// The whole-content cap is checked before any parse. The key set is
/// **exact** per op — every key in [ProjectTodoOpKind.contentKeys] must be
/// present and no other may be — so absent and `null` are different things.
ProjectTodoOp decodeProjectTodoOp(String content) {
  if (utf8.encode(content).length > maxProjectTodoContentBytes) {
    throw const FormatException(
      'todo op content exceeds $maxProjectTodoContentBytes bytes',
    );
  }
  final Object? decoded;
  try {
    decoded = jsonDecode(content);
  } on FormatException {
    throw const FormatException('malformed todo op payload');
  }
  if (decoded is! Map<String, Object?>) {
    throw const FormatException('todo op payload must be an object');
  }
  final object = decoded;
  if (object['schema'] != projectTodoSchema) {
    throw const FormatException('todo op schema must be "$projectTodoSchema"');
  }
  final wire = _takeString(object, 'op');
  final kind = ProjectTodoOpKind.fromWire(wire);
  if (kind == null) throw FormatException('unknown project todo op "$wire"');
  for (final key in object.keys) {
    if (!kind.contentKeys.contains(key)) {
      throw FormatException(
        'todo op ${kind.wire} has unsupported field "$key"',
      );
    }
  }
  for (final key in kind.contentKeys) {
    if (!object.containsKey(key)) {
      throw FormatException('todo op ${kind.wire} is missing field "$key"');
    }
  }
  final listId = _takeString(object, 'listId');
  _validateId('listId', listId);
  String? itemId;
  if (kind.isItemOp) {
    itemId = _takeString(object, 'itemId');
    _validateId('itemId', itemId);
  }
  switch (kind) {
    case ProjectTodoOpKind.listCreate:
      final title = _takeString(object, 'title');
      validateTodoText('title', title);
      return ProjectTodoOp.listCreate(listId: listId, title: title);
    case ProjectTodoOpKind.listTitle:
      final title = _takeString(object, 'title');
      validateTodoText('title', title);
      return ProjectTodoOp.listTitle(listId: listId, title: title);
    case ProjectTodoOpKind.listArchived:
      return ProjectTodoOp.listArchived(
        listId: listId,
        archived: _takeBool(object, 'archived'),
      );
    case ProjectTodoOpKind.itemAdd:
      final text = _takeString(object, 'text');
      validateTodoText('text', text);
      final rank = _takeString(object, 'rank');
      validateRank(rank);
      return ProjectTodoOp.itemAdd(
        listId: listId,
        itemId: itemId!,
        text: text,
        rank: rank,
      );
    case ProjectTodoOpKind.itemText:
      final text = _takeString(object, 'text');
      validateTodoText('text', text);
      return ProjectTodoOp.itemText(
        listId: listId,
        itemId: itemId!,
        text: text,
      );
    case ProjectTodoOpKind.itemDone:
      return ProjectTodoOp.itemDone(
        listId: listId,
        itemId: itemId!,
        done: _takeBool(object, 'done'),
      );
    case ProjectTodoOpKind.itemAssignee:
      final assignee = _takeNullableString(object, 'assignee');
      if (assignee != null && !_isPubkeyHex(assignee)) {
        throw const FormatException(
          'todo assignee must be a 64-character lowercase hex pubkey',
        );
      }
      return ProjectTodoOp.itemAssignee(
        listId: listId,
        itemId: itemId!,
        assignee: assignee,
      );
    case ProjectTodoOpKind.itemDue:
      final due = _takeNullableString(object, 'due');
      if (due != null) validateDueDate(due);
      return ProjectTodoOp.itemDue(listId: listId, itemId: itemId!, due: due);
    case ProjectTodoOpKind.itemRank:
      final rank = _takeString(object, 'rank');
      validateRank(rank);
      return ProjectTodoOp.itemRank(
        listId: listId,
        itemId: itemId!,
        rank: rank,
      );
    case ProjectTodoOpKind.itemRemove:
      return ProjectTodoOp.itemRemove(listId: listId, itemId: itemId!);
  }
}

final _anyHex64 = RegExp(r'^[0-9a-fA-F]{64}$');

/// The canonical `30621:<lowercase-hex>:<dtag>` spelling of a project
/// coordinate, or `null` when [value] is not one (port of
/// `normalize_project_coordinate`). Hex case is folded; the dtag is kept as
/// written, must be non-empty, at most 64 characters, and free of control
/// characters.
String? normalizeProjectCoordinate(String value) {
  final first = value.indexOf(':');
  if (first < 0) return null;
  final second = value.indexOf(':', first + 1);
  if (second < 0) return null;
  final kind = value.substring(0, first);
  final pubkey = value.substring(first + 1, second);
  final dtag = value.substring(second + 1);
  if (kind != '30621') return null;
  if (!_anyHex64.hasMatch(pubkey)) return null;
  if (dtag.isEmpty || dtag.runes.length > 64 || dtag.runes.any(_isControl)) {
    return null;
  }
  return '30621:${pubkey.toLowerCase()}:$dtag';
}

/// The fold's decode rule (CONTRACT rule 1): the event is a 44248, carries
/// exactly one `a` tag that normalizes to [project], and its content passes
/// the op grammar. Returns `null` for anything else — the fold counts it in
/// `ignored`.
ProjectTodoOp? decodeProjectTodoEvent(NostrEvent event, String project) {
  if (event.kind != EventKind.projectTodoOp) return null;
  String? coordinate;
  for (final tag in event.tags) {
    if (tag.isEmpty || tag[0] != 'a') continue;
    if (coordinate != null) return null;
    if (tag.length < 2) return null;
    coordinate = tag[1];
  }
  if (coordinate == null) return null;
  if (normalizeProjectCoordinate(coordinate) != project) return null;
  try {
    return decodeProjectTodoOp(event.content);
  } on FormatException {
    return null;
  }
}

/// Validate a signed op end to end: kind, tag grammar, canonical project
/// coordinate, content envelope, and tag/content agreement — the relay's
/// rule, so a client can check what it is about to send.
///
/// Tag grammar: position-independent, closed key set; exactly one each of
/// `a`, `td-v`, `td-op`, `td-list`; exactly one `td-item` on an item op and
/// none on a list op; every tag exactly two fields; any other key —
/// including `h` — is a rejection.
ProjectTodoOp validateProjectTodoEnvelope(NostrEvent event) {
  if (event.kind != EventKind.projectTodoOp) {
    throw const FormatException('event is not a project todo op (kind 44248)');
  }
  final op = decodeProjectTodoOp(event.content);
  final seen = <String, String>{};
  for (final tag in event.tags) {
    if (tag.length != 2) {
      throw const FormatException('todo op tags must have exactly two fields');
    }
    final key = tag[0];
    switch (key) {
      case 'a' || 'td-v' || 'td-op' || 'td-list' || 'td-item':
        if (seen.containsKey(key)) {
          throw FormatException('todo op has more than one $key tag');
        }
        seen[key] = tag[1];
      case 'h':
        throw const FormatException(
          'todo op must not carry an h tag; it is project-scoped',
        );
      default:
        throw FormatException('todo op has unsupported tag key "$key"');
    }
  }
  final coordinate = seen['a'];
  if (coordinate == null) {
    throw const FormatException('todo op requires one a tag');
  }
  if (normalizeProjectCoordinate(coordinate) != coordinate) {
    throw const FormatException(
      '44248 `a` tag must be a canonical 30621:<lowercase-hex>:<dtag> '
      'coordinate',
    );
  }
  if (seen['td-v'] != projectTodoTagVersion) {
    throw const FormatException('unsupported todo op tag version');
  }
  final tagOp = seen['td-op'];
  if (tagOp == null) {
    throw const FormatException('todo op requires one td-op tag');
  }
  if (tagOp != op.kind.wire) {
    throw FormatException(
      'todo op td-op tag "$tagOp" does not match content op "${op.kind.wire}"',
    );
  }
  final tagList = seen['td-list'];
  if (tagList == null) {
    throw const FormatException('todo op requires one td-list tag');
  }
  if (tagList != op.listId) {
    throw const FormatException(
      'todo op td-list tag does not match content listId',
    );
  }
  final tagItem = seen['td-item'];
  if (op.itemId != null) {
    if (tagItem == null) {
      throw const FormatException('todo item op requires one td-item tag');
    }
    if (tagItem != op.itemId) {
      throw const FormatException(
        'todo op td-item tag does not match content itemId',
      );
    }
  } else if (tagItem != null) {
    throw const FormatException('todo list op must not carry a td-item tag');
  }
  return op;
}
