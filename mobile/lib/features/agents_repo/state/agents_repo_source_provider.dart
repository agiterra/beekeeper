import 'package:flutter/foundation.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/relay/relay.dart';

/// The project's agents repository as its newest kind:30624 pins it.
@immutable
class AgentsRepoSource {
  /// `30617:<owner>:<id>`.
  final String repo;
  final String owner;
  final String id;

  /// `refs/heads/<branch>` when the source follows a branch.
  final String? ref;

  /// A pinned commit when it does not.
  final String? sha;

  /// `.` for an agents repository; a sub-path for a pack layout.
  final String path;

  const AgentsRepoSource({
    required this.repo,
    required this.owner,
    required this.id,
    required this.ref,
    required this.sha,
    required this.path,
  });

  /// The shape the Files page edits: the repository root, following a branch.
  bool get isAgentsRepo => path == '.' && ref != null;
}

String? _tag(NostrEvent event, String key) {
  for (final tag in event.tags) {
    if (tag.length >= 2 && tag[0] == key) return tag[1];
  }
  return null;
}

/// Decode a kind:30624, or `null` for a shape this reader does not fully
/// recognise (strict on purpose: a guessed source stages nothing honest).
AgentsRepoSource? decodeAgentsRepoSource(NostrEvent event) {
  if (event.kind != EventKind.projectPackSource) return null;
  final repo = _tag(event, 'repo');
  if (repo == null) return null;
  final parts = repo.split(':');
  if (parts.length != 3 || parts[0] != '30617') return null;
  final ref = _tag(event, 'ref');
  final sha = _tag(event, 'sha');
  if ((ref == null) == (sha == null)) return null;
  return AgentsRepoSource(
    repo: repo,
    owner: parts[1].toLowerCase(),
    id: parts[2],
    ref: ref,
    sha: sha,
    path: _tag(event, 'path') ?? 'personas/roles',
  );
}

/// The project's newest source, or `null` when it publishes none.
final agentsRepoSourceProvider =
    FutureProvider.family<AgentsRepoSource?, String>((ref, address) async {
      final session = ref.read(relaySessionProvider.notifier);
      final events = await session.query(
        NostrFilters.projectPackSource(address),
      );
      AgentsRepoSource? newest;
      int? newestAt;
      for (final event in events) {
        final source = decodeAgentsRepoSource(event);
        if (source == null) continue;
        if (newestAt == null || event.createdAt > newestAt) {
          newest = source;
          newestAt = event.createdAt;
        }
      }
      return newest;
    });
