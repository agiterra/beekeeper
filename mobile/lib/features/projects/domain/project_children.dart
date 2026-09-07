import 'package:flutter/foundation.dart';

import '../../coding_sessions/domain/coding_sessions_domain.dart';
import '../../terminals/domain/terminals_domain.dart';
import 'project_models.dart';

/// One row in a project's list; the type picks the icon (desktop
/// `projectChildren.ts`). The list holds channels and interactive work —
/// coding sessions and terminals; repositories, workflows and agents live
/// on the project page, not here.
@immutable
sealed class ProjectChildRow {
  const ProjectChildRow();

  /// Fixed display rank — live coding sessions on top: a session is the
  /// only child that changes while you watch it.
  int get rank;

  /// Stable, cross-type-unique key.
  String get key;

  String get label;
}

/// A coding session, read from its channel's observer.
class ProjectSessionRow extends ProjectChildRow {
  final CodingSessionUmbrella session;
  final String channelId;
  final String channelName;

  const ProjectSessionRow({
    required this.session,
    required this.channelId,
    required this.channelName,
  });

  @override
  int get rank => 0;

  @override
  String get key => 'session:$channelId:${session.key}';

  @override
  String get label => session.displayName;

  /// Lowercase founder pubkey, or `null` when the fold could not resolve one.
  String? get founderPubkey => session.founder.pubkey?.toLowerCase();

  bool get isClosed => session.closed;

  /// working → waiting → unknown → reported (idle) → ended.
  int get statusPriority => switch (session.status.kind) {
    CodingSessionFoldedStatusKind.working => 0,
    CodingSessionFoldedStatusKind.waiting => 1,
    CodingSessionFoldedStatusKind.unknown => 2,
    CodingSessionFoldedStatusKind.reported => 3,
    CodingSessionFoldedStatusKind.ended => 4,
  };
}

/// A stream or forum channel bound to the project.
class ProjectChannelRow extends ProjectChildRow {
  final ProjectChannel channel;

  const ProjectChannelRow(this.channel);

  bool get isForum => channel.channelType == 'forum';

  @override
  int get rank => isForum ? 2 : 1;

  @override
  String get key => '${isForum ? 'forum' : 'channel'}:${channel.id}';

  @override
  String get label => channel.name.isEmpty ? channel.id : channel.name;
}

/// A member's shared terminal announced under the project.
class ProjectTerminalRow extends ProjectChildRow {
  final RemoteTerminal terminal;

  const ProjectTerminalRow(this.terminal);

  @override
  int get rank => 3;

  @override
  String get key => 'remote-shell:${terminal.key}';

  @override
  String get label => terminal.title;
}

/// Sessions keep activity order (open before closed, then working →
/// unknown → idle, then newest first); everything else alphabetizes within
/// its rank. Alphabetizing sessions would sort by a label that is mostly the
/// same word plus a generation number, burying the one running now.
int compareProjectChildren(ProjectChildRow a, ProjectChildRow b) {
  final byRank = a.rank.compareTo(b.rank);
  if (byRank != 0) return byRank;
  if (a is ProjectSessionRow && b is ProjectSessionRow) {
    if (a.isClosed != b.isClosed) return a.isClosed ? 1 : -1;
    final byActivity = a.statusPriority.compareTo(b.statusPriority);
    if (byActivity != 0) return byActivity;
    final byTime = b.session.lastActivityAt.compareTo(a.session.lastActivityAt);
    if (byTime != 0) return byTime;
    return a.key.compareTo(b.key);
  }
  final byLabel = a.label.toLowerCase().compareTo(b.label.toLowerCase());
  return byLabel != 0 ? byLabel : a.key.compareTo(b.key);
}

/// Merge every kind of project child into the single flat, type-ranked,
/// alphabetized list. Callers apply the session filter before this.
List<ProjectChildRow> buildProjectChildren({
  required Iterable<ProjectSessionRow> sessions,
  required Iterable<ProjectChannel> channels,
  required Iterable<RemoteTerminal> terminals,
}) {
  final rows = <ProjectChildRow>[
    ...sessions,
    // The sessions transport carries the coding sessions; as a channel row
    // it would be an empty chat, so the desktop hides it and so does this.
    for (final channel in channels)
      if (!channel.isTransport) ProjectChannelRow(channel),
    for (final terminal in terminals) ProjectTerminalRow(terminal),
  ];
  rows.sort(compareProjectChildren);
  return rows;
}

/// How many session rows a page shows before "Show more".
const projectSessionPageSize = 10;

/// Distinct known founders across a project's sessions, first-seen order.
List<String> projectSessionFounders(Iterable<ProjectSessionRow> sessions) {
  final seen = <String>{};
  final ordered = <String>[];
  for (final row in sessions) {
    final founder = row.founderPubkey;
    if (founder != null && seen.add(founder)) ordered.add(founder);
  }
  return ordered;
}
