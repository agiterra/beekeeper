import 'dart:convert';

import 'package:buzz/shared/utils/fractional_rank.dart';
import 'package:flutter/foundation.dart';

import 'agents_repo_draft_op.dart';

/// NIP-AR kind:44251 — the Dart twin of
/// `crates/beekeeper-core/src/project_artifact_pin.rs`.
///
/// Which documents, plans and folders of a project's agents repository show in
/// every member's sidebar, and in what order. A pin is shared: every member
/// sees it. Hiding the pinned rows is a per-device viewing preference that
/// never reaches the wire.
const projectArtifactPinKind = 44251;
const projectArtifactPinSchema = 'buzz-project-artifact-pin/v1';
const projectArtifactPinTagVersion = 'ar1-1';
const maxProjectArtifactPinContentBytes = 2 * 1024;

/// What one op does.
enum PinOpKind {
  pinSet('pin.set'),
  pinRank('pin.rank');

  const PinOpKind(this.wire);
  final String wire;

  static PinOpKind? fromWire(String value) {
    for (final kind in values) {
      if (kind.wire == value) return kind;
    }
    return null;
  }

  List<String> get contentKeys => switch (this) {
    pinSet => const ['schema', 'op', 'target', 'targetKind', 'pinned', 'rank'],
    pinRank => const ['schema', 'op', 'target', 'rank'],
  };
}

/// What a pin points at.
enum PinTargetKind {
  file('file'),
  folder('folder');

  const PinTargetKind(this.wire);
  final String wire;

  static PinTargetKind? fromWire(String value) {
    for (final kind in values) {
      if (kind.wire == value) return kind;
    }
    return null;
  }
}

/// One op: the repository it is about, the target, and what it sets.
@immutable
class ArtifactPinOp {
  /// `30617:<hex>:<id>`, as the project's kind:30624 pins it.
  final String repo;
  final PinOpKind kind;
  final String target;

  /// A `pin.set`'s target kind.
  final PinTargetKind? targetKind;

  /// A `pin.set`'s flag.
  final bool? pinned;

  /// The order key. Both ops carry one.
  final String rank;

  const ArtifactPinOp._({
    required this.repo,
    required this.kind,
    required this.target,
    required this.rank,
    this.targetKind,
    this.pinned,
  });

  const ArtifactPinOp.pinSet({
    required String repo,
    required String target,
    required PinTargetKind targetKind,
    required bool pinned,
    required String rank,
  }) : this._(
         repo: repo,
         kind: PinOpKind.pinSet,
         target: target,
         rank: rank,
         targetKind: targetKind,
         pinned: pinned,
       );

  const ArtifactPinOp.pinRank({
    required String repo,
    required String target,
    required String rank,
  }) : this._(repo: repo, kind: PinOpKind.pinRank, target: target, rank: rank);

  /// Canonical content JSON: the exact key set, in canonical order.
  String toContent() {
    final object = <String, Object?>{
      'schema': projectArtifactPinSchema,
      'op': kind.wire,
      'target': target,
    };
    if (kind == PinOpKind.pinSet) {
      object['targetKind'] = targetKind?.wire;
      object['pinned'] = pinned;
    }
    object['rank'] = rank;
    return jsonEncode(object);
  }

  /// The tags this op carries, in canonical order: `a`, `ar-v`, `ar-op`,
  /// `ar-repo`, `ar-target`.
  List<List<String>> tags(String coordinate) => [
    ['a', coordinate],
    ['ar-v', projectArtifactPinTagVersion],
    ['ar-op', kind.wire],
    ['ar-repo', repo],
    ['ar-target', target],
  ];

  @override
  bool operator ==(Object other) =>
      other is ArtifactPinOp &&
      toContent() == other.toContent() &&
      repo == other.repo;

  @override
  int get hashCode => Object.hash(repo, toContent());
}

/// Why a folder prefix is refused, or `null`.
///
/// A folder is `docs/<segment>/…` with one to [maxDocumentComponents] − 1
/// segments, each a document name. It is deliberately not a path the grammar
/// admits: git has no directory object, so a folder is only the prefix its
/// files share. One fewer segment than a path, so a file under the deepest
/// pinnable folder still fits.
String? pinFolderError(String target) {
  if (target.isEmpty || target.length > 512) {
    return 'a pinned folder is 1–512 bytes';
  }
  if (target.startsWith('/') || target.endsWith('/')) {
    return '"$target" must be relative with no trailing slash';
  }
  final segments = target.split('/');
  final folders = segments.sublist(1);
  if (segments.first != docsRoot) {
    return '"$target" is outside the documents tree, the only part of the '
        'layout with folders';
  }
  if (folders.isEmpty) {
    return '"$target" is the documents tree itself, which is not pinnable';
  }
  if (folders.length >= maxDocumentComponents) {
    return '"$target" is ${folders.length} segments under $docsRoot/; the '
        'deepest pinnable folder is ${maxDocumentComponents - 1} so a file '
        'under it still fits';
  }
  // One rule for both: a folder name is a document name, so a path built
  // under a pinnable folder is a path the grammar admits.
  for (final segment in folders) {
    if (draftPathClass('$docsRoot/$segment/x.md') == null) {
      return '"$target" has a segment "$segment" that is not a document name';
    }
  }
  return null;
}

/// Why [target] is refused for [kind], or `null`.
///
/// A file target is any path the layout admits except a folder keep: the keep
/// is how an empty directory exists in git, and pinning it instead of the
/// folder it holds open would put a row called `.gitkeep` in the sidebar.
String? pinTargetError(String target, PinTargetKind kind) {
  if (kind == PinTargetKind.folder) return pinFolderError(target);
  final classified = draftPathClass(target);
  if (classified == null) {
    return '"$target" is outside the agents repository layout';
  }
  if (classified == DraftPathClass.documentFolder) {
    return '"$target" is a folder\'s keep; pin the folder it holds open, not '
        'the file';
  }
  return null;
}

/// Whether [target] is a legal target of either kind.
bool isPinTarget(String target) =>
    pinTargetError(target, PinTargetKind.file) == null ||
    pinTargetError(target, PinTargetKind.folder) == null;

/// Decode and validate content JSON, or `null` when it fails any rule.
ArtifactPinOp? decodeArtifactPinOp(String content, String repo) {
  if (utf8.encode(content).length > maxProjectArtifactPinContentBytes) {
    return null;
  }
  Object? parsed;
  try {
    parsed = jsonDecode(content);
  } catch (_) {
    return null;
  }
  if (parsed is! Map<String, Object?>) return null;
  if (parsed['schema'] != projectArtifactPinSchema) return null;
  final wire = parsed['op'];
  if (wire is! String) return null;
  final kind = PinOpKind.fromWire(wire);
  if (kind == null) return null;
  final object = parsed;
  final expected = kind.contentKeys;
  if (object.keys.any((key) => !expected.contains(key))) return null;
  if (expected.any((key) => !object.containsKey(key))) return null;
  final target = object['target'];
  if (target is! String) return null;
  final rank = object['rank'];
  if (rank is! String || !isValidRank(rank)) return null;
  switch (kind) {
    case PinOpKind.pinSet:
      final rawKind = object['targetKind'];
      if (rawKind is! String) return null;
      final targetKind = PinTargetKind.fromWire(rawKind);
      if (targetKind == null) return null;
      if (pinTargetError(target, targetKind) != null) return null;
      final pinned = object['pinned'];
      if (pinned is! bool) return null;
      return ArtifactPinOp.pinSet(
        repo: repo,
        target: target,
        targetKind: targetKind,
        pinned: pinned,
        rank: rank,
      );
    case PinOpKind.pinRank:
      // A `pin.rank` does not repeat what the target is, so it cannot be
      // checked against a kind. Either shape is legal here and the fold keeps
      // it only when a `pin.set` established the target.
      if (!isPinTarget(target)) return null;
      return ArtifactPinOp.pinRank(repo: repo, target: target, rank: rank);
  }
}
