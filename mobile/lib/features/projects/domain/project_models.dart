import 'package:flutter/foundation.dart';

import '../../../shared/relay/nostr_models.dart';

/// A project head (kind:30621, NIP-MP), as the phone reads it.
///
/// Mirrors the desktop's `eventToProjectContainer`
/// (`projectContainerModel.ts`): the address is `30621:<owner>:<d>`, the
/// name falls back to the slug, and `channel` tags are the project's own
/// forward references to its channels. Membership on the head is advisory —
/// the relay's roster projection (kind:39010) is the grant.
@immutable
class Project {
  final String address;

  /// Lowercase 64-hex owner pubkey — the head's signer.
  final String owner;
  final String dtag;
  final String name;
  final String description;

  /// `private` when the head says so; anything else reads public, matching
  /// the relay's fail-open default for untagged heads.
  final bool isPrivate;

  /// Channel ids the head claims (`channel` tags), deduplicated in order.
  final List<String> channelIds;

  /// Member pubkeys from `p` tags (owner excluded), lowercase.
  final List<String> memberPubkeys;

  /// The head's `created_at`.
  final int createdAt;

  const Project({
    required this.address,
    required this.owner,
    required this.dtag,
    required this.name,
    required this.description,
    required this.isPrivate,
    required this.channelIds,
    required this.memberPubkeys,
    required this.createdAt,
  });
}

final _hex64 = RegExp(r'^[0-9a-f]{64}$');

String? _singleTag(NostrEvent event, String name) {
  for (final tag in event.tags) {
    if (tag.length >= 2 && tag[0] == name) return tag[1];
  }
  return null;
}

/// Read one head as a [Project], or `null` for a malformed one.
Project? projectFromEvent(NostrEvent event) {
  if (event.kind != EventKind.project) return null;
  final dtag = _singleTag(event, 'd');
  if (dtag == null || dtag.isEmpty) return null;
  final owner = event.pubkey.toLowerCase();
  if (!_hex64.hasMatch(owner)) return null;
  final channelIds = <String>[];
  final seenChannels = <String>{};
  final members = <String>[];
  final seenMembers = <String>{};
  for (final tag in event.tags) {
    if (tag.length < 2) continue;
    if (tag[0] == 'channel' && tag[1].isNotEmpty) {
      if (seenChannels.add(tag[1])) channelIds.add(tag[1]);
    } else if (tag[0] == 'p') {
      final pubkey = tag[1].toLowerCase();
      if (!_hex64.hasMatch(pubkey) || pubkey == owner) continue;
      if (seenMembers.add(pubkey)) members.add(pubkey);
    }
  }
  final name = _singleTag(event, 'name');
  final description = _singleTag(event, 'description');
  return Project(
    address: '30621:$owner:$dtag',
    owner: owner,
    dtag: dtag,
    name: name == null || name.trim().isEmpty ? dtag : name.trim(),
    description: description ?? event.content,
    isPrivate: _singleTag(event, 'buzz-access') == 'private',
    channelIds: List.unmodifiable(channelIds),
    memberPubkeys: List.unmodifiable(members),
    createdAt: event.createdAt,
  );
}

/// NIP-33: the newest head per `(owner, d)` slot.
List<NostrEvent> dedupProjectHeads(Iterable<NostrEvent> events) {
  final best = <String, NostrEvent>{};
  for (final event in events) {
    if (event.kind != EventKind.project) continue;
    final key = '${event.pubkey.toLowerCase()}:${_singleTag(event, 'd') ?? ''}';
    final incumbent = best[key];
    if (incumbent == null ||
        event.createdAt > incumbent.createdAt ||
        (event.createdAt == incumbent.createdAt &&
            event.id.compareTo(incumbent.id) < 0)) {
      best[key] = event;
    }
  }
  return best.values.toList();
}

/// True when a kind:5 signed by the project's owner names its address.
bool isProjectDeleted(Project project, Iterable<NostrEvent> deletions) {
  for (final event in deletions) {
    if (event.kind != EventKind.deletion) continue;
    if (event.pubkey.toLowerCase() != project.owner) continue;
    for (final tag in event.tags) {
      if (tag.length >= 2 && tag[0] == 'a' && tag[1] == project.address) {
        return true;
      }
    }
  }
  return false;
}

/// The live projects in [heads] minus those [deletions] tombstoned, sorted
/// by name.
List<Project> projectsFromEvents(
  Iterable<NostrEvent> heads,
  Iterable<NostrEvent> deletions,
) {
  final tombstones = deletions.toList();
  final projects = <Project>[];
  for (final head in dedupProjectHeads(heads)) {
    final project = projectFromEvent(head);
    if (project == null) continue;
    if (isProjectDeleted(project, tombstones)) continue;
    projects.add(project);
  }
  projects.sort((left, right) {
    final byName = left.name.toLowerCase().compareTo(right.name.toLowerCase());
    return byName != 0 ? byName : left.address.compareTo(right.address);
  });
  return projects;
}

/// The two ways a channel is bound to a project, unioned as the desktop does
/// (`projectCascade.ts` `channelBelongsToProject`): the head's own `channel`
/// tags, and the `project` tag the relay stamps on the channel's metadata.
bool channelBelongsToProject({
  required Project project,
  required String channelId,
  required String? channelProjectRef,
}) =>
    project.channelIds.contains(channelId) ||
    (channelProjectRef != null && channelProjectRef == project.address);

/// A channel as the project page needs to know it.
@immutable
class ProjectChannel {
  final String id;
  final String name;

  /// `stream`, `forum`, `dm`, or `transport` — the relay-assigned type.
  final String channelType;

  /// Whether this device's channel list includes it (a 39002 row exists).
  /// A project's transport channel admits members without one, so `false`
  /// does not mean "not allowed in".
  final bool isMember;

  /// Unix seconds of the newest known activity, when the reader has any.
  final int? lastActivityAt;

  const ProjectChannel({
    required this.id,
    required this.name,
    required this.channelType,
    required this.isMember,
    this.lastActivityAt,
  });

  bool get isTransport => channelType == 'transport';
}

String _collapse(String value) => value.trim().replaceAll(RegExp(r'\s+'), ' ');

/// The canonical name a project's sessions channel is created with.
String projectSessionsChannelName(String projectName) {
  final base = _collapse(projectName);
  return base.isEmpty ? 'sessions' : '$base sessions';
}

/// Which of a project's channels carries its coding sessions.
///
/// The desktop's derivation (`projectSessionsChannel.ts`), not a stored
/// mapping: a `transport`-typed channel wins outright; failing that the
/// channel named `<project> sessions`; failing that the channel with the most
/// recent activity; failing that there is none yet.
ProjectChannel? pickProjectSessionsChannel(
  Project project,
  Iterable<ProjectChannel> channels,
) {
  final list = channels.toList();
  for (final channel in list) {
    if (channel.isTransport) return channel;
  }
  final wanted = _collapse(
    projectSessionsChannelName(project.name),
  ).toLowerCase();
  for (final channel in list) {
    if (_collapse(channel.name).toLowerCase() == wanted) return channel;
  }
  ProjectChannel? busiest;
  for (final channel in list) {
    final at = channel.lastActivityAt;
    if (at == null) continue;
    if (busiest == null || at > (busiest.lastActivityAt ?? 0)) {
      busiest = channel;
    }
  }
  return busiest;
}
