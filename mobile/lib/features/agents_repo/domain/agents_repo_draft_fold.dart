import 'dart:convert';

import 'package:flutter/foundation.dart';

import '../../../shared/relay/nostr_models.dart';
import '../../../shared/relay/project_coordinate.dart';
import 'agents_repo_draft_op.dart';

/// The agents-repository draft fold: from a bag of kind 44249 ops to the
/// open draft per path a reader sees.
///
/// A port of `crates/buzz-core/src/agents_repo_draft_fold.rs`, bound to
/// `conformance/agents-repo-draft-fold/CONTRACT.md` and pinned by
/// `fixtures/fold-vectors.json`. Pure and total: any set of events in, one
/// digest out, the same digest from every client.

const agentsRepoDraftDigestSchema = 'buzz-agents-repo-draft-digest/v1';

/// One file op as the digest reports it, head or superseded.
@immutable
class DraftRow {
  final String id;
  final String author;
  final int createdAt;
  final DraftOpKind op;
  final String path;
  final String? to;
  final String? text;
  final String? base;
  final String? baseCommit;
  final String? prev;
  final String? message;

  const DraftRow({
    required this.id,
    required this.author,
    required this.createdAt,
    required this.op,
    required this.path,
    required this.to,
    required this.text,
    required this.base,
    required this.baseCommit,
    required this.prev,
    required this.message,
  });

  Map<String, Object?> toJson() => {
    'id': id,
    'author': author,
    'createdAt': createdAt,
    'op': op.wire,
    'path': path,
    'to': to,
    'text': text,
    'base': base,
    'baseCommit': baseCommit,
    'prev': prev,
    'message': message,
  };
}

/// One path with an open draft.
@immutable
class DraftPath {
  final String path;
  final DraftRow head;

  /// Every other open op naming the path, oldest first.
  final List<DraftRow> superseded;

  /// The head did not build on the newest superseded op.
  final bool diverged;
  final int updatedAt;

  const DraftPath({
    required this.path,
    required this.head,
    required this.superseded,
    required this.diverged,
    required this.updatedAt,
  });

  Map<String, Object?> toJson() => {
    'path': path,
    'head': head.toJson(),
    'superseded': [for (final row in superseded) row.toJson()],
    'diverged': diverged,
    'updatedAt': updatedAt,
  };
}

@immutable
class CommitRecordRow {
  final String id;
  final String commit;
  final String by;
  final int createdAt;
  final List<String> paths;
  final List<String> drafts;
  final String? message;

  const CommitRecordRow({
    required this.id,
    required this.commit,
    required this.by,
    required this.createdAt,
    required this.paths,
    required this.drafts,
    required this.message,
  });

  Map<String, Object?> toJson() => {
    'id': id,
    'commit': commit,
    'by': by,
    'createdAt': createdAt,
    'paths': paths,
    'drafts': drafts,
    'message': message,
  };
}

@immutable
class AgentsRepoDraftDigest {
  final String project;
  final String repo;
  final int ignored;
  final int otherRepo;
  final List<DraftPath> paths;
  final List<CommitRecordRow> commits;

  const AgentsRepoDraftDigest({
    required this.project,
    required this.repo,
    required this.ignored,
    required this.otherRepo,
    required this.paths,
    required this.commits,
  });

  const AgentsRepoDraftDigest.empty(this.project, this.repo)
    : ignored = 0,
      otherRepo = 0,
      paths = const [],
      commits = const [];

  DraftPath? byPath(String path) {
    for (final entry in paths) {
      if (entry.path == path) return entry;
    }
    return null;
  }

  Map<String, Object?> toJson() => {
    'schema': agentsRepoDraftDigestSchema,
    'project': project,
    'repo': repo,
    'ignored': ignored,
    'otherRepo': otherRepo,
    'paths': [for (final entry in paths) entry.toJson()],
    'commits': [for (final record in commits) record.toJson()],
  };
}

final _anyHex64 = RegExp(r'^[0-9a-fA-F]{64}$');

/// `30617:<lowercase hex>:<id>` exactly; `null` otherwise.
String? canonicalRepositoryCoordinate(String value) {
  final first = value.indexOf(':');
  if (first < 0) return null;
  final second = value.indexOf(':', first + 1);
  if (second < 0) return null;
  final kind = value.substring(0, first);
  final hex = value.substring(first + 1, second);
  final id = value.substring(second + 1);
  if (kind != '30617' || !_anyHex64.hasMatch(hex) || id.isEmpty) return null;
  final canonical = '30617:${hex.toLowerCase()}:$id';
  return canonical == value ? canonical : null;
}

class _Decoded {
  final int createdAt;
  final String id;
  final String pubkey;
  final AgentsRepoDraftOp op;
  _Decoded(this.createdAt, this.id, this.pubkey, this.op);
}

int _compareKey(int aCreated, String aId, int bCreated, String bId) {
  if (aCreated != bCreated) return aCreated.compareTo(bCreated);
  return aId.compareTo(bId);
}

int _bytewise(String a, String b) {
  final ea = utf8.encode(a);
  final eb = utf8.encode(b);
  final n = ea.length < eb.length ? ea.length : eb.length;
  for (var i = 0; i < n; i++) {
    if (ea[i] != eb[i]) return ea[i].compareTo(eb[i]);
  }
  return ea.length.compareTo(eb.length);
}

DraftRow _row(_Decoded d) => DraftRow(
  id: d.id,
  author: d.pubkey,
  createdAt: d.createdAt,
  op: d.op.kind,
  path: d.op.path!,
  to: d.op.kind == DraftOpKind.fileMove ? d.op.to : null,
  text: d.op.kind == DraftOpKind.filePut ? d.op.text : null,
  base: d.op.base,
  baseCommit: d.op.baseCommit,
  prev: d.op.prev,
  message: d.op.message,
);

/// Fold [events] for [project] and its agents repository [repo].
AgentsRepoDraftDigest foldAgentsRepoDrafts(
  String project,
  String repo,
  Iterable<NostrEvent> events,
) {
  var ignored = 0;
  var otherRepo = 0;
  final seen = <String>{};
  final ops = <_Decoded>[];
  for (final event in events) {
    if (!seen.add(event.id)) continue;
    if (event.kind != EventKind.agentsRepoDraftOp) {
      ignored++;
      continue;
    }
    final coordinate = singleTag(event, 'a');
    if (coordinate == null ||
        normalizeProjectCoordinate(coordinate) != project) {
      ignored++;
      continue;
    }
    final tagRepo = singleTag(event, 'ad-repo');
    final canonical = tagRepo == null
        ? null
        : canonicalRepositoryCoordinate(tagRepo);
    if (canonical == null) {
      ignored++;
      continue;
    }
    final op = decodeAgentsRepoDraftOp(event.content, canonical);
    if (op == null) {
      ignored++;
      continue;
    }
    if (canonical != repo) {
      otherRepo++;
      continue;
    }
    ops.add(_Decoded(event.createdAt, event.id, event.pubkey, op));
  }
  ops.sort((a, b) => _compareKey(a.createdAt, a.id, b.createdAt, b.id));

  final closed = <String>{};
  final commits = <CommitRecordRow>[];
  for (final d in ops) {
    if (d.op.kind != DraftOpKind.commitRecord) continue;
    closed.addAll(d.op.drafts);
    commits.add(
      CommitRecordRow(
        id: d.id,
        commit: d.op.commit!,
        by: d.pubkey,
        createdAt: d.createdAt,
        paths: List.of(d.op.paths),
        drafts: List.of(d.op.drafts),
        message: d.op.message,
      ),
    );
  }
  final commitsNewestFirst = commits.reversed.toList();

  final byPath = <String, List<DraftRow>>{};
  for (final d in ops) {
    if (closed.contains(d.id)) continue;
    if (d.op.kind == DraftOpKind.commitRecord) continue;
    final row = _row(d);
    for (final path in d.op.namedPaths) {
      byPath.putIfAbsent(path, () => []).add(row);
    }
  }
  final paths = <DraftPath>[];
  for (final entry in byPath.entries) {
    final open = entry.value;
    final head = open.removeLast();
    final last = open.isEmpty ? null : open.last;
    final diverged = last != null && last.id != head.prev;
    paths.add(
      DraftPath(
        path: entry.key,
        head: head,
        superseded: open,
        diverged: diverged,
        updatedAt: head.createdAt,
      ),
    );
  }
  paths.sort((a, b) => _bytewise(a.path, b.path));

  return AgentsRepoDraftDigest(
    project: project,
    repo: repo,
    ignored: ignored,
    otherRepo: otherRepo,
    paths: paths,
    commits: commitsNewestFirst,
  );
}
