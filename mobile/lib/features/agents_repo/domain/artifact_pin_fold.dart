import 'package:beekeeper/shared/relay/nostr_models.dart';
import 'package:beekeeper/shared/relay/project_coordinate.dart';
import 'package:flutter/foundation.dart';

import 'agents_repo_draft_fold.dart' show canonicalRepositoryCoordinate;
import 'artifact_pin_op.dart';

/// The project artifact pin fold — the Dart twin of
/// `crates/beekeeper-core/src/project_artifact_pin_fold.rs`, bound to the same
/// vectors (`conformance/project-artifact-pin-fold/`).
///
/// Rules, normatively, are that directory's `CONTRACT.md`.
const projectArtifactPinDigestSchema = 'buzz-project-artifact-pin-digest/v1';

/// One target the digest reports.
@immutable
class PinRow {
  /// The file path or folder prefix.
  final String target;
  final PinTargetKind targetKind;

  /// Whether it shows in every member's sidebar now.
  final bool pinned;
  final String rank;

  /// Who pinned it: the author of the winning `pin.set`.
  final String by;
  final int updatedAt;

  const PinRow({
    required this.target,
    required this.targetKind,
    required this.pinned,
    required this.rank,
    required this.by,
    required this.updatedAt,
  });

  Map<String, Object?> toJson() => {
    'target': target,
    'targetKind': targetKind.wire,
    'pinned': pinned,
    'rank': rank,
    'by': by,
    'updatedAt': updatedAt,
  };
}

/// What every reader gets from one bag of ops.
@immutable
class ProjectArtifactPinDigest {
  final String project;
  final String repo;

  /// Events dropped as malformed or mis-scoped.
  final int ignored;

  /// Well-formed ops for another repository.
  final int otherRepo;

  /// `pin.rank` ops naming a target no `pin.set` ever introduced.
  final int ranksWithoutPin;
  final List<PinRow> pins;

  /// The digest of a project nothing has been read for yet.
  const ProjectArtifactPinDigest.empty(this.project, this.repo)
    : ignored = 0,
      otherRepo = 0,
      ranksWithoutPin = 0,
      pins = const [];

  const ProjectArtifactPinDigest({
    required this.project,
    required this.repo,
    required this.ignored,
    required this.otherRepo,
    required this.ranksWithoutPin,
    required this.pins,
  });

  /// The pinned rows, in order — what a sidebar draws.
  List<PinRow> get pinnedOnly => [
    for (final row in pins)
      if (row.pinned) row,
  ];

  Map<String, Object?> toJson() => {
    'schema': projectArtifactPinDigestSchema,
    'project': project,
    'repo': repo,
    'ignored': ignored,
    'otherRepo': otherRepo,
    'ranksWithoutPin': ranksWithoutPin,
    'pins': [for (final row in pins) row.toJson()],
  };
}

class _Key implements Comparable<_Key> {
  const _Key(this.createdAt, this.id);
  final int createdAt;
  final String id;

  @override
  int compareTo(_Key other) {
    if (createdAt != other.createdAt) {
      return createdAt.compareTo(other.createdAt);
    }
    return id.compareTo(other.id);
  }
}

class _Decoded {
  const _Decoded(this.key, this.pubkey, this.op);
  final _Key key;
  final String pubkey;
  final ArtifactPinOp op;
}

/// One field's winner: the value and the key that set it.
class _Slot<T> {
  T? value;
  _Key? key;

  void offer(_Key candidate, T next) {
    final held = key;
    if (held == null || candidate.compareTo(held) > 0) {
      key = candidate;
      value = next;
    }
  }
}

class _TargetState {
  final pinned = _Slot<bool>();
  final targetKind = _Slot<PinTargetKind>();
  final rank = _Slot<String>();

  /// The author of the winning `pin.set` — who pinned it.
  final by = _Slot<String>();
  int updatedAt = 0;
}

String? _singleTag(NostrEvent event, String key) {
  String? found;
  for (final tag in event.tags) {
    if (tag.length == 2 && tag[0] == key) {
      if (found != null) return null;
      found = tag[1];
    }
  }
  return found;
}

/// Fold [events] for [project] and its agents repository [repo].
ProjectArtifactPinDigest foldProjectArtifactPins(
  String project,
  String repo,
  Iterable<NostrEvent> events,
) {
  var ignored = 0;
  var otherRepo = 0;
  var ranksWithoutPin = 0;
  final seen = <String>{};
  final ops = <_Decoded>[];
  for (final event in events) {
    if (!seen.add(event.id)) continue;
    if (event.kind != projectArtifactPinKind) {
      ignored++;
      continue;
    }
    final coordinate = _singleTag(event, 'a');
    if (coordinate == null ||
        normalizeProjectCoordinate(coordinate) != project) {
      ignored++;
      continue;
    }
    final tagRepo = _singleTag(event, 'ar-repo');
    // `canonicalRepositoryCoordinate` answers null unless the tag is already
    // canonical, which is the rule: ingest never stores a variant.
    final canonical = tagRepo == null
        ? null
        : canonicalRepositoryCoordinate(tagRepo);
    if (canonical == null) {
      ignored++;
      continue;
    }
    final op = decodeArtifactPinOp(event.content, canonical);
    if (op == null) {
      ignored++;
      continue;
    }
    if (canonical != repo) {
      otherRepo++;
      continue;
    }
    ops.add(_Decoded(_Key(event.createdAt, event.id), event.pubkey, op));
  }
  ops.sort((a, b) => a.key.compareTo(b.key));

  // Introduce every target first, so a `pin.rank` that arrived before its
  // `pin.set` in the log still counts — the order of the bag is not the order
  // of the clock, and a reader must not depend on it.
  final targets = <String, _TargetState>{};
  for (final decoded in ops) {
    if (decoded.op.kind == PinOpKind.pinSet) {
      targets.putIfAbsent(decoded.op.target, _TargetState.new);
    }
  }
  for (final decoded in ops) {
    final state = targets[decoded.op.target];
    if (state == null) {
      ranksWithoutPin++;
      continue;
    }
    if (decoded.op.kind == PinOpKind.pinSet) {
      state.pinned.offer(decoded.key, decoded.op.pinned!);
      state.targetKind.offer(decoded.key, decoded.op.targetKind!);
      state.rank.offer(decoded.key, decoded.op.rank);
      state.by.offer(decoded.key, decoded.pubkey);
    } else {
      state.rank.offer(decoded.key, decoded.op.rank);
    }
    state.updatedAt = state.updatedAt > decoded.key.createdAt
        ? state.updatedAt
        : decoded.key.createdAt;
  }

  final pins = <PinRow>[];
  for (final entry in targets.entries) {
    final state = entry.value;
    final targetKind = state.targetKind.value;
    final pinned = state.pinned.value;
    final rank = state.rank.value;
    final by = state.by.value;
    if (targetKind == null || pinned == null || rank == null || by == null) {
      continue;
    }
    pins.add(
      PinRow(
        target: entry.key,
        targetKind: targetKind,
        pinned: pinned,
        rank: rank,
        by: by,
        updatedAt: state.updatedAt,
      ),
    );
  }
  pins.sort((a, b) {
    final byRank = a.rank.compareTo(b.rank);
    return byRank != 0 ? byRank : a.target.compareTo(b.target);
  });

  return ProjectArtifactPinDigest(
    project: project,
    repo: repo,
    ignored: ignored,
    otherRepo: otherRepo,
    ranksWithoutPin: ranksWithoutPin,
    pins: pins,
  );
}

/// What a pinned artifact's row reads.
///
/// The tail of the path with `.md` taken off and every other extension kept:
/// a document's format is part of what it is, so `login.html` stays itself
/// while `api-shape.md` reads as its name. A folder reads as its own last
/// segment.
String artifactPinLabel(PinRow pin) {
  final tail = pin.target.substring(pin.target.lastIndexOf('/') + 1);
  return tail.endsWith('.md') ? tail.substring(0, tail.length - 3) : tail;
}
