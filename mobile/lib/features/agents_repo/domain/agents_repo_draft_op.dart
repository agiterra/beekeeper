import 'dart:convert';

import 'package:flutter/foundation.dart';

import '../../../shared/relay/nostr_models.dart';

/// The kind 44250 agents-repository draft op — Dart twin of
/// `crates/buzz-core/src/agents_repo_draft.rs` and `docs/nips/NIP-AD.md`.
///
/// A draft is one proposed change to one file of the project's agents
/// repository: the whole new text (`file.put`), an archive move
/// (`file.move`), a removal (`file.delete`), or the committer's record that
/// named drafts landed on `main` (`commit.record`). The content key set is
/// exact per op — absent is not null — and the `ad-*` tags repeat the
/// content's op, repository and paths so the relay can gate without parsing.

const agentsRepoDraftSchema = 'buzz-agents-repo-draft/v1';
const agentsRepoDraftTagVersion = 'ad1-1';
const maxAgentsRepoDraftContentBytes = 65536;
const maxAgentsRepoDraftTextBytes = 60000;
const maxAgentsRepoDraftMessageBytes = 512;
const maxCommitRecordEntries = 256;

/// The reserved directory; equal to `buzz_persona::team::ARCHIVE_DIR`.
const archiveSegment = 'archive';

/// Root files a draft may put but never move or delete.
const rootFiles = ['README.md', 'team.yml', 'actions.yml'];

/// The one tree in the layout with folders: a project's document artifacts.
/// `plans/`, `roles/` and `skills/` stay flat, because a plan's path is cited
/// by every adopted `planRef` and a role's stem is a `team.yml` key.
const docsRoot = 'docs';

/// Components allowed under `docs/`, the last of them the file.
const maxDocumentComponents = 8;

/// A document's formats, lowercase so one path names one file.
const documentExtensions = ['.md', '.html'];

/// An image a document embeds, committed beside it.
const documentAssetExtensions = [
  '.png',
  '.jpg',
  '.jpeg',
  '.gif',
  '.webp',
  '.svg',
];

/// The only dotfile the documents tree admits — how an empty folder exists.
const gitkeep = '.gitkeep';

enum DraftOpKind {
  filePut('file.put'),
  fileMove('file.move'),
  fileDelete('file.delete'),
  assetPut('asset.put'),
  commitRecord('commit.record');

  const DraftOpKind(this.wire);
  final String wire;

  static DraftOpKind? fromWire(String value) {
    for (final kind in values) {
      if (kind.wire == value) return kind;
    }
    return null;
  }

  List<String> get contentKeys => switch (this) {
    filePut => const [
      'schema',
      'op',
      'path',
      'text',
      'base',
      'baseCommit',
      'prev',
      'message',
    ],
    fileMove => const [
      'schema',
      'op',
      'path',
      'to',
      'base',
      'baseCommit',
      'prev',
      'message',
    ],
    fileDelete => const [
      'schema',
      'op',
      'path',
      'base',
      'baseCommit',
      'prev',
      'message',
    ],
    assetPut => const [
      'schema',
      'op',
      'path',
      'sha256',
      'mime',
      'size',
      'base',
      'baseCommit',
      'prev',
      'message',
    ],
    commitRecord => const [
      'schema',
      'op',
      'commit',
      'paths',
      'drafts',
      'message',
    ],
  };
}

/// Where a path sits in the agents repository's layout.
enum DraftPathClass {
  rootFile,
  role,
  archivedRole,
  roleSkill,
  sharedSkill,
  plan,
  archivedPlan,

  /// `docs/<folder>/…/<stem>.md` or `.html` — a document artifact.
  document,

  /// `docs/<folder>/…/<file>.png` and the other image formats.
  documentAsset,

  /// `docs/<folder>/…/.gitkeep` — how an empty folder exists in git.
  documentFolder;

  /// Whether this path sits in the documents tree, where folders nest and a
  /// move may name any path of the same class.
  bool get isDocument =>
      this == DraftPathClass.document ||
      this == DraftPathClass.documentAsset ||
      this == DraftPathClass.documentFolder;
}

/// One op: the repository it belongs to, an optional reason, and the edit.
@immutable
class AgentsRepoDraftOp {
  /// `30617:<hex>:<id>`, as the project's kind:30624 pins it.
  final String repo;
  final String? message;
  final DraftOpKind kind;

  /// The file (file ops).
  final String? path;

  /// A move's destination.
  final String? to;

  /// A put's whole text.
  final String? text;

  /// An `asset.put`'s media blob id.
  final String? sha256;

  /// An `asset.put`'s MIME.
  final String? mime;

  /// An `asset.put`'s size in bytes.
  final int? size;

  /// Blob sha of the file on `main` the author started from; null = new.
  final String? base;

  /// Advisory: the `main` commit the author read.
  final String? baseCommit;

  /// The draft head the author edited from; null when there was none.
  final String? prev;

  /// A record's commit.
  final String? commit;

  /// A record's paths.
  final List<String> paths;

  /// A record's closed draft ids.
  final List<String> drafts;

  const AgentsRepoDraftOp._({
    required this.repo,
    required this.message,
    required this.kind,
    this.path,
    this.to,
    this.text,
    this.sha256,
    this.mime,
    this.size,
    this.base,
    this.baseCommit,
    this.prev,
    this.commit,
    this.paths = const [],
    this.drafts = const [],
  });

  const AgentsRepoDraftOp.filePut({
    required String repo,
    required String path,
    required String text,
    required String? base,
    required String? baseCommit,
    required String? prev,
    String? message,
  }) : this._(
         repo: repo,
         message: message,
         kind: DraftOpKind.filePut,
         path: path,
         text: text,
         base: base,
         baseCommit: baseCommit,
         prev: prev,
       );

  const AgentsRepoDraftOp.fileMove({
    required String repo,
    required String path,
    required String to,
    required String? base,
    required String? baseCommit,
    required String? prev,
    String? message,
  }) : this._(
         repo: repo,
         message: message,
         kind: DraftOpKind.fileMove,
         path: path,
         to: to,
         base: base,
         baseCommit: baseCommit,
         prev: prev,
       );

  const AgentsRepoDraftOp.fileDelete({
    required String repo,
    required String path,
    required String? base,
    required String? baseCommit,
    required String? prev,
    String? message,
  }) : this._(
         repo: repo,
         message: message,
         kind: DraftOpKind.fileDelete,
         path: path,
         base: base,
         baseCommit: baseCommit,
         prev: prev,
       );

  const AgentsRepoDraftOp.assetPut({
    required String repo,
    required String path,
    required String sha256,
    required String mime,
    required int size,
    required String? base,
    required String? baseCommit,
    required String? prev,
    String? message,
  }) : this._(
         repo: repo,
         message: message,
         kind: DraftOpKind.assetPut,
         path: path,
         sha256: sha256,
         mime: mime,
         size: size,
         base: base,
         baseCommit: baseCommit,
         prev: prev,
       );

  const AgentsRepoDraftOp.commitRecord({
    required String repo,
    required String commit,
    required List<String> paths,
    required List<String> drafts,
    String? message,
  }) : this._(
         repo: repo,
         message: message,
         kind: DraftOpKind.commitRecord,
         commit: commit,
         paths: paths,
         drafts: drafts,
       );

  /// Every path the op names: the file (and a move's destination), or a
  /// record's paths.
  List<String> get namedPaths => switch (kind) {
    DraftOpKind.filePut ||
    DraftOpKind.fileDelete ||
    DraftOpKind.assetPut => [path!],
    DraftOpKind.fileMove => [path!, to!],
    DraftOpKind.commitRecord => List.of(paths),
  };

  /// Canonical content JSON: the exact key set, nullables as null.
  String toContent() {
    final object = <String, Object?>{
      'schema': agentsRepoDraftSchema,
      'op': kind.wire,
    };
    switch (kind) {
      case DraftOpKind.filePut:
        object['path'] = path;
        object['text'] = text;
        object['base'] = base;
        object['baseCommit'] = baseCommit;
        object['prev'] = prev;
      case DraftOpKind.fileMove:
        object['path'] = path;
        object['to'] = to;
        object['base'] = base;
        object['baseCommit'] = baseCommit;
        object['prev'] = prev;
      case DraftOpKind.fileDelete:
        object['path'] = path;
        object['base'] = base;
        object['baseCommit'] = baseCommit;
        object['prev'] = prev;
      case DraftOpKind.assetPut:
        object['path'] = path;
        object['sha256'] = sha256;
        object['mime'] = mime;
        object['size'] = size;
        object['base'] = base;
        object['baseCommit'] = baseCommit;
        object['prev'] = prev;
      case DraftOpKind.commitRecord:
        object['commit'] = commit;
        object['paths'] = paths;
        object['drafts'] = drafts;
    }
    object['message'] = message;
    return jsonEncode(object);
  }

  /// The tags: `a`, `ad-v`, `ad-op`, `ad-repo`, then one `ad-path` per path.
  List<List<String>> tags(String coordinate) => [
    ['a', coordinate],
    ['ad-v', agentsRepoDraftTagVersion],
    ['ad-op', kind.wire],
    ['ad-repo', repo],
    for (final named in namedPaths) ['ad-path', named],
  ];

  @override
  bool operator ==(Object other) =>
      other is AgentsRepoDraftOp &&
      toContent() == other.toContent() &&
      repo == other.repo;

  @override
  int get hashCode => Object.hash(repo, toContent());
}

final _lowerHex40 = RegExp(r'^[0-9a-f]{40}$');
final _lowerHex64 = RegExp(r'^[0-9a-f]{64}$');
final _slug = RegExp(r'^[a-z0-9-]{1,64}$');
final _fileSegment = RegExp(r'^[A-Za-z0-9._-]+$');
final _docStem = RegExp(r'^[A-Za-z0-9._-]+$');

bool isGitSha(String? value) => value != null && _lowerHex40.hasMatch(value);
bool isEventId(String? value) => value != null && _lowerHex64.hasMatch(value);

bool _isSlug(String value) => _slug.hasMatch(value) && value != archiveSegment;

bool _isFileSegment(String value) =>
    value.isNotEmpty &&
    value != '.' &&
    value != '..' &&
    _fileSegment.hasMatch(value);

/// A **plan's** filename stem: the name of a document, not a manifest key. It
/// carries uppercase, `_` and interior dots, because the documents that move
/// into an agents repository are called `CURRENT_STATE.md`, `SESSION_STATE.md`
/// and `README.md`. Requiring [_isSlug] here was a carry-over from the role
/// rule and it cost something real — Beekeeper's own map and ledger moved in on
/// 2026-09-22 at paths the Files tab refuses to open.
///
/// Still bounded, never `archive` in any case (that names the sibling
/// directory), and never leading with `.` or `-`. Pinned by
/// `conformance/agents-repo-draft-path/`.
bool _isDocStem(String value) =>
    value.isNotEmpty &&
    value.length <= 96 &&
    !value.startsWith('.') &&
    !value.startsWith('-') &&
    _docStem.hasMatch(value) &&
    value.toLowerCase() != archiveSegment;

/// A role or skill file: named for its manifest key, so a slug.
bool _isRoleFile(String segment) =>
    segment.endsWith('.md') &&
    _isSlug(segment.substring(0, segment.length - 3));

/// A plan file: named for the document it holds.
bool _isPlanFile(String segment) =>
    segment.endsWith('.md') &&
    _isDocStem(segment.substring(0, segment.length - 3));

/// A segment of the documents tree: a folder name, or a document's or asset's
/// stem. The same shape as [_isDocStem] — these are document names too —
/// except that `archive` is an ordinary name here, because the documents tree
/// has no archive rule. A document is moved or deleted.
bool _isDocSegment(String value) =>
    value.isNotEmpty &&
    value.length <= 96 &&
    !value.startsWith('.') &&
    !value.startsWith('-') &&
    _docStem.hasMatch(value);

/// The stem of [file] once a known extension is taken off, or `null` when the
/// name ends in none of them. The *trailing* extension is the one read, so
/// `notes.md.txt` is not a document.
String? _stemWithExtension(String file, List<String> extensions) {
  for (final extension in extensions) {
    if (file.endsWith(extension)) {
      return file.substring(0, file.length - extension.length);
    }
  }
  return null;
}

/// Classify the components under `docs/`. Folders nest, bounded by
/// [maxDocumentComponents]; every folder name is a document name; and the file
/// is a document, an asset, or the keep that holds an empty folder open.
DraftPathClass? _classifyDocumentPath(List<String> components) {
  if (components.length > maxDocumentComponents) return null;
  final file = components.last;
  final folders = components.sublist(0, components.length - 1);
  if (!folders.every(_isDocSegment)) return null;
  if (file == gitkeep) return DraftPathClass.documentFolder;
  final documentStem = _stemWithExtension(file, documentExtensions);
  if (documentStem != null && _isDocSegment(documentStem)) {
    return DraftPathClass.document;
  }
  final assetStem = _stemWithExtension(file, documentAssetExtensions);
  if (assetStem != null && _isDocSegment(assetStem)) {
    return DraftPathClass.documentAsset;
  }
  return null;
}

/// Classify a path against the agents repository layout, or `null` when it
/// is outside it.
DraftPathClass? draftPathClass(String path) {
  if (path.isEmpty || path.length > 512) return null;
  if (path.startsWith('/') || path.endsWith('/')) return null;
  if (rootFiles.contains(path)) return DraftPathClass.rootFile;
  final segments = path.split('/');
  if (!segments.every(_isFileSegment)) return null;
  if (segments.length == 3 &&
      segments[0] == 'roles' &&
      segments[1] == archiveSegment &&
      _isRoleFile(segments[2])) {
    return DraftPathClass.archivedRole;
  }
  if (segments.length == 3 &&
      segments[0] == 'plans' &&
      segments[1] == archiveSegment &&
      _isPlanFile(segments[2])) {
    return DraftPathClass.archivedPlan;
  }
  if (segments.length == 2 &&
      segments[0] == 'roles' &&
      _isRoleFile(segments[1])) {
    return DraftPathClass.role;
  }
  if (segments.length == 2 &&
      segments[0] == 'plans' &&
      _isPlanFile(segments[1])) {
    return DraftPathClass.plan;
  }
  if (segments.length >= 5 &&
      segments[0] == 'roles' &&
      _isSlug(segments[1]) &&
      segments[2] == 'skills' &&
      _isSlug(segments[3])) {
    return DraftPathClass.roleSkill;
  }
  if (segments.length >= 3 && segments[0] == 'skills' && _isSlug(segments[1])) {
    return DraftPathClass.sharedSkill;
  }
  if (segments.length >= 2 && segments[0] == docsRoot) {
    return _classifyDocumentPath(segments.sublist(1));
  }
  return null;
}

/// The MIME the relay's media store admits for each asset extension, which is
/// also what an `asset.put` must name. `.svg` is deliberately absent: the
/// media store refuses `image/svg+xml` as active web content, so an SVG in the
/// tree is committed with git rather than uploaded.
const documentAssetMimes = <String, String>{
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
};

/// A sanity bound on an `asset.put`'s `size`; the media store's cap is real.
const maxDocumentAssetBytes = 100 * 1024 * 1024;

/// The MIME an `asset.put` at [path] must name, or `null` when the path cannot
/// carry an uploaded asset at all. The extension decides, not the uploader.
String? documentAssetMime(String path) {
  if (draftPathClass(path) != DraftPathClass.documentAsset) return null;
  for (final entry in documentAssetMimes.entries) {
    if (path.endsWith(entry.key)) return entry.value;
  }
  return null;
}

/// Why a `file.move` from [path] to [to] is refused, or `null` when it names a
/// legal destination.
///
/// The rule differs by class, deliberately: a role or plan has exactly one
/// destination, its [archiveCounterpart] (a plan is never renamed — every
/// adopted `planRef` names it by path); a document, asset or folder keep may
/// move to any path of its own class, which is what rename and
/// move-between-folders are, and Markdown and HTML are one class so changing a
/// document's format is a move; a root file is put-only and a skill file has
/// no move at all.
///
/// Renaming a folder is one move per file under it, issued by the caller.
/// This admits each one; it does not make a directory move atomic.
/// Pinned by `conformance/agents-repo-draft-path/` (`moves`).
String? moveDestinationError(String path, String to) {
  final from = draftPathClass(path);
  if (from == null) return '"$path" is outside the agents repository layout.';
  if (from.isDocument) {
    if (to == path) return '"$path" must move to a different path.';
    final target = draftPathClass(to);
    if (target == null) return '"$to" is outside the agents repository layout.';
    if (target != from) {
      return '"$path" may only name another path of its own class, not "$to".';
    }
    return null;
  }
  final counterpart = archiveCounterpart(path);
  if (counterpart == null) {
    return '"$path" is not a role or plan that can move to or from archive/.';
  }
  if (counterpart != to) {
    return '"$path" may only go to "$counterpart", not "$to".';
  }
  return null;
}

/// The only legal `to` of a `file.move`, or `null`.
String? archiveCounterpart(String path) {
  final segments = path.split('/');
  return switch (draftPathClass(path)) {
    DraftPathClass.role ||
    DraftPathClass.plan => '${segments[0]}/$archiveSegment/${segments[1]}',
    DraftPathClass.archivedRole ||
    DraftPathClass.archivedPlan => '${segments[0]}/${segments[2]}',
    _ => null,
  };
}

bool _hasControl(String value, {required bool allowWhitespace}) {
  for (final code in value.codeUnits) {
    if (code == 0x7f) return true;
    if (code >= 0x20) continue;
    if (allowWhitespace && (code == 0x0a || code == 0x0d || code == 0x09)) {
      continue;
    }
    return true;
  }
  return false;
}

/// Why a `file.put` text is refused, or `null`.
String? draftTextError(String text) {
  final bytes = utf8.encode(text).length;
  if (bytes > maxAgentsRepoDraftTextBytes) {
    return 'The text is $bytes bytes; a draft carries at most '
        '$maxAgentsRepoDraftTextBytes. A file this size is edited with git.';
  }
  if (_hasControl(text, allowWhitespace: true)) {
    return 'The text must not contain control characters.';
  }
  return null;
}

/// Why a message is refused, or `null`.
String? draftMessageError(String message) {
  if (utf8.encode(message).length > maxAgentsRepoDraftMessageBytes) {
    return 'A note is at most $maxAgentsRepoDraftMessageBytes bytes.';
  }
  if (_hasControl(message, allowWhitespace: false)) {
    return 'A note is one line.';
  }
  return null;
}

String? _nullableString(Map<String, Object?> object, String key) {
  final value = object[key];
  if (value == null) return null;
  if (value is String) return value;
  throw const FormatException('not a string');
}

({String? base, String? baseCommit, String? prev}) _decodeBase(
  Map<String, Object?> object,
) {
  final base = _nullableString(object, 'base');
  final baseCommit = _nullableString(object, 'baseCommit');
  final prev = _nullableString(object, 'prev');
  if (base != null && !isGitSha(base)) throw const FormatException('base');
  if (baseCommit != null && !isGitSha(baseCommit)) {
    throw const FormatException('baseCommit');
  }
  if (prev != null && !isEventId(prev)) throw const FormatException('prev');
  return (base: base, baseCommit: baseCommit, prev: prev);
}

List<String> _decodeStringList(Object? value, bool Function(String) check) {
  if (value is! List) throw const FormatException('list');
  if (value.isEmpty || value.length > maxCommitRecordEntries) {
    throw const FormatException('list size');
  }
  final out = <String>[];
  for (final item in value) {
    if (item is! String || !check(item) || out.contains(item)) {
      throw const FormatException('list item');
    }
    out.add(item);
  }
  return out;
}

/// Decode content JSON for the repository [repo]; `null` for anything the
/// Rust validator refuses (the fold counts those as `ignored`).
AgentsRepoDraftOp? decodeAgentsRepoDraftOp(String content, String repo) {
  if (utf8.encode(content).length > maxAgentsRepoDraftContentBytes) return null;
  Object? parsed;
  try {
    parsed = jsonDecode(content);
  } on FormatException {
    return null;
  }
  if (parsed is! Map<String, Object?>) return null;
  final object = parsed;
  if (object['schema'] != agentsRepoDraftSchema) return null;
  final op = object['op'];
  if (op is! String) return null;
  final kind = DraftOpKind.fromWire(op);
  if (kind == null) return null;
  final expected = kind.contentKeys;
  if (object.keys.any((key) => !expected.contains(key))) return null;
  if (expected.any((key) => !object.containsKey(key))) return null;
  try {
    final message = _nullableString(object, 'message');
    if (message != null && draftMessageError(message) != null) return null;
    switch (kind) {
      case DraftOpKind.filePut:
        final path = object['path'];
        final text = object['text'];
        if (path is! String || draftPathClass(path) == null) return null;
        if (text is! String || draftTextError(text) != null) return null;
        final base = _decodeBase(object);
        return AgentsRepoDraftOp.filePut(
          repo: repo,
          path: path,
          text: text,
          base: base.base,
          baseCommit: base.baseCommit,
          prev: base.prev,
          message: message,
        );
      case DraftOpKind.fileMove:
        final path = object['path'];
        final to = object['to'];
        if (path is! String || to is! String) return null;
        if (moveDestinationError(path, to) != null) return null;
        final base = _decodeBase(object);
        return AgentsRepoDraftOp.fileMove(
          repo: repo,
          path: path,
          to: to,
          base: base.base,
          baseCommit: base.baseCommit,
          prev: base.prev,
          message: message,
        );
      case DraftOpKind.fileDelete:
        final path = object['path'];
        if (path is! String) return null;
        final klass = draftPathClass(path);
        if (klass == null || klass == DraftPathClass.rootFile) return null;
        final base = _decodeBase(object);
        return AgentsRepoDraftOp.fileDelete(
          repo: repo,
          path: path,
          base: base.base,
          baseCommit: base.baseCommit,
          prev: base.prev,
          message: message,
        );
      case DraftOpKind.assetPut:
        final path = object['path'];
        if (path is! String) return null;
        final mime = documentAssetMime(path);
        if (mime == null || object['mime'] != mime) return null;
        final sha256 = object['sha256'];
        if (sha256 is! String || !isEventId(sha256)) return null;
        final size = object['size'];
        if (size is! int || size <= 0 || size > maxDocumentAssetBytes) {
          return null;
        }
        final base = _decodeBase(object);
        return AgentsRepoDraftOp.assetPut(
          repo: repo,
          path: path,
          sha256: sha256,
          mime: mime,
          size: size,
          base: base.base,
          baseCommit: base.baseCommit,
          prev: base.prev,
          message: message,
        );
      case DraftOpKind.commitRecord:
        final commit = object['commit'];
        if (commit is! String || !isGitSha(commit)) return null;
        final paths = _decodeStringList(
          object['paths'],
          (p) => draftPathClass(p) != null,
        );
        final drafts = _decodeStringList(object['drafts'], isEventId);
        return AgentsRepoDraftOp.commitRecord(
          repo: repo,
          commit: commit,
          paths: paths,
          drafts: drafts,
          message: message,
        );
    }
  } on FormatException {
    return null;
  }
}

/// Exactly one tag with [key], or `null`.
String? singleTag(NostrEvent event, String key) {
  String? found;
  for (final tag in event.tags) {
    if (tag.isEmpty || tag[0] != key) continue;
    if (found != null || tag.length < 2) return null;
    found = tag[1];
  }
  return found;
}
