import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../../shared/relay/nostr_models.dart';
import '../../../shared/relay/relay_provider.dart';
import '../../../shared/theme/theme.dart';
import '../../../shared/utils/string_utils.dart';
import '../../../shared/widgets/modal_presentation.dart';
import '../../channels/channel.dart';
import '../../channels/channel_detail_page.dart';
import '../../coding_sessions/ui/coding_session_page.dart';
import '../../coding_sessions/ui/coding_session_status_chip.dart';
import '../../coding_sessions/ui/coding_sessions_page.dart';
import '../../coding_sessions/ui/observer_contract.dart';
import '../../profile/user_cache_provider.dart';
import '../../terminals/domain/terminals_domain.dart';
import '../../terminals/state/terminals_index_provider.dart';
import '../../terminals/ui/terminal_observe_page.dart';
import '../../terminals/ui/terminal_row.dart';
import '../domain/project_children.dart';
import '../domain/project_models.dart';
import '../domain/project_session_filter.dart';
import '../state/project_session_filter_provider.dart';
import '../state/projects_provider.dart';

part 'project_tree/filter_sheet.dart';
part 'project_tree/rows.dart';

/// Opens a terminal from a project tree.
typedef ProjectTerminalOpener =
    void Function(BuildContext context, RemoteTerminal terminal);

/// The opener the tree uses: the observe page. Tests override it with
/// `null` to keep rows inert, which the row shows (no chevron).
final projectTerminalOpenerProvider = Provider<ProjectTerminalOpener?>(
  (ref) =>
      (context, terminal) => Navigator.of(context).push(
        MaterialPageRoute<void>(
          builder: (_) => TerminalObservePage(terminal: terminal),
        ),
      ),
);

/// The one line a project with nothing in it shows.
const projectEmptyLabel = 'No channels, coding sessions or terminals yet';

/// One project's contents as the desktop sidebar lists them: one flat list,
/// type-ranked — channels and forums, then the open coding sessions, then
/// terminals, then the settled sessions dimmed — each row with a single
/// icon for its type and, for sessions and terminals, who started or owns
/// it. A session filter sits under the list whenever the project has any
/// session or terminal at all, even when it currently hides every one, so a
/// "My sessions" choice that matches nothing can always be undone.
///
/// Channels come from two bindings unioned as the desktop does: the head's
/// own `channel` tags and the relay-stamped `project` tag on each channel's
/// metadata. Sessions are read from every channel the project binds; a
/// terminal announce names only a project, so terminals are siblings of the
/// channels, never under one.
class ProjectTree extends HookConsumerWidget {
  final Project project;

  /// This device's own channel list, or an empty list before it loads.
  final List<Channel> myChannels;

  const ProjectTree({
    super.key,
    required this.project,
    required this.myChannels,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final read = ref.watch(projectsProvider);
    final terminals = ref.watch(terminalsIndexProvider);
    final me = ref.watch(myPubkeyProvider);
    final profiles = ref.watch(userCacheProvider);
    final opener = ref.watch(projectTerminalOpenerProvider);
    final binding = ref.watch(codingSessionObserverBindingProvider);
    final filter = ref
        .watch(projectSessionFiltersProvider)
        .forProject(project.address);
    final pageCount = useState(1);
    // A narrower list starts at the top.
    useEffect(() {
      pageCount.value = 1;
      return null;
    }, [filter]);

    final channels = projectChannelsFor(
      project: project,
      myChannels: myChannels,
      referenced: read.referencedChannels,
    );
    // Every bound channel's sessions, through the same observer the session
    // pages read. What a channel's read could not settle is disclosed, not
    // hidden: a failed read is a line, not an empty list.
    final allSessions = <ProjectSessionRow>[];
    final readNotes = <String>[];
    for (final channel in channels) {
      final snapshot = binding.watch(ref, channel.id);
      for (final session in snapshot.sessions) {
        allSessions.add(
          ProjectSessionRow(
            session: session,
            channelId: channel.id,
            channelName: channel.name,
          ),
        );
      }
      if (snapshot.hasBlockingError) {
        readNotes.add(
          '${channel.name.isEmpty ? channel.id : channel.name}: sessions '
          'could not be read (${snapshot.lastError})',
        );
      }
    }
    final filtered = filterProjectSessions(
      allSessions,
      filter,
      facts: (row) => ProjectSessionFilterEntry(
        founderPubkey: row.founderPubkey,
        isClosed: row.isClosed,
        lastActivityAt: row.session.lastActivityAt,
      ),
      myPubkey: me,
    );
    final projectTerminals = terminals.forProject(project.address);
    final children = buildProjectChildren(
      sessions: filtered.shown,
      channels: channels,
      terminals: projectTerminals,
    );
    final channelRows = children.whereType<ProjectChannelRow>().toList();
    final terminalRows = children.whereType<ProjectTerminalRow>().toList();
    final sessionRows = children.whereType<ProjectSessionRow>().toList();
    final openRows = [
      for (final row in sessionRows)
        if (!row.isClosed) row,
    ];
    final settledRows = [
      for (final row in sessionRows)
        if (row.isClosed) row,
    ];
    final limit = pageCount.value * projectSessionPageSize;
    final visibleOpen = openRows.take(limit).toList();
    final visibleSettled = settledRows
        .take((limit - visibleOpen.length).clamp(0, settledRows.length))
        .toList();
    final remaining =
        sessionRows.length - visibleOpen.length - visibleSettled.length;
    final hasAnySession = allSessions.isNotEmpty || projectTerminals.isNotEmpty;
    final terminalsUnreadable =
        !terminals.hasRead && terminals.connection == TerminalsConnection.error;
    final isEmpty =
        channelRows.isEmpty &&
        allSessions.isEmpty &&
        projectTerminals.isEmpty &&
        readNotes.isEmpty &&
        !terminalsUnreadable;

    String ownerLabel(String pubkey) =>
        profiles[pubkey.toLowerCase()]?.label ?? shortPubkey(pubkey);
    Channel? myChannel(String id) {
      for (final channel in myChannels) {
        if (channel.id == id) return channel;
      }
      return null;
    }

    void openSession(ProjectSessionRow row) => Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => CodingSessionPage(
          channelId: row.channelId,
          sessionKey: row.session.key,
        ),
      ),
    );
    Widget sessionTile(ProjectSessionRow row) => _SessionTile(
      key: ValueKey('project-row-${row.key}'),
      row: row,
      founderLabel: row.founderPubkey == null
          ? null
          : ownerLabel(row.founderPubkey!),
      isMine:
          me != null &&
          row.founderPubkey != null &&
          row.founderPubkey == me.toLowerCase(),
      onTap: () => openSession(row),
    );

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (isEmpty)
          ProjectSectionNote(
            key: ValueKey('project-empty-${project.address}'),
            text: projectEmptyLabel,
          )
        else ...[
          for (final row in channelRows)
            _ProjectChildTile(
              key: ValueKey('project-row-${row.key}'),
              icon: row.isForum ? LucideIcons.messagesSquare : LucideIcons.hash,
              label: row.label,
              detail: row.channel.isMember ? null : 'not in your list',
              onTap: () {
                final channel = myChannel(row.channel.id);
                Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) => channel != null
                        ? ChannelDetailPage(channel: channel)
                        : CodingSessionsPage(
                            channelId: row.channel.id,
                            channelName: row.channel.name,
                          ),
                  ),
                );
              },
            ),
          for (final row in visibleOpen) sessionTile(row),
          for (final row in terminalRows)
            TerminalRow(
              key: ValueKey('project-row-${row.key}'),
              terminal: row.terminal,
              ownerLabel: ownerLabel(row.terminal.ownerPubkey),
              viewerPubkey: me,
              onTap: opener == null
                  ? null
                  : () => opener(context, row.terminal),
            ),
          for (final row in visibleSettled) sessionTile(row),
          if (remaining > 0)
            TextButton(
              key: ValueKey('project-show-more-${project.address}'),
              onPressed: () => pageCount.value += 1,
              child: Text(
                'Show more ('
                '${remaining < projectSessionPageSize ? remaining : projectSessionPageSize}'
                ' of $remaining)',
              ),
            ),
          for (final note in readNotes)
            ProjectSectionNote(text: note, emphasise: true),
          if (terminalsUnreadable)
            ProjectSectionNote(
              text: 'Terminals could not be read: ${terminals.lastError}',
              emphasise: true,
            ),
          if (hasAnySession)
            _ProjectFilterBar(
              project: project,
              filter: filter,
              founders: projectSessionFounders(allSessions),
              hiddenUnattributed: filtered.hiddenUnattributed,
              hiddenByState: filtered.hiddenByState,
              ownerLabel: ownerLabel,
              myPubkey: me,
              onChange: (next) => ref
                  .read(projectSessionFiltersProvider.notifier)
                  .setFilter(project.address, next),
            ),
        ],
      ],
    );
  }
}

/// The project's channels: this device's own list filtered by binding, plus
/// the heads' referenced channels the list does not carry.
List<ProjectChannel> projectChannelsFor({
  required Project project,
  required List<Channel> myChannels,
  required Map<String, ChannelData> referenced,
}) {
  final result = <ProjectChannel>[];
  final seen = <String>{};
  for (final channel in myChannels) {
    if (!channelBelongsToProject(
      project: project,
      channelId: channel.id,
      channelProjectRef: channel.projectRef,
    )) {
      continue;
    }
    if (!seen.add(channel.id)) continue;
    result.add(
      ProjectChannel(
        id: channel.id,
        name: channel.name,
        channelType: channel.channelType,
        isMember: true,
        lastActivityAt: channel.lastMessageAt == null
            ? null
            : channel.lastMessageAt!.millisecondsSinceEpoch ~/ 1000,
      ),
    );
  }
  for (final id in project.channelIds) {
    if (seen.contains(id)) continue;
    final data = referenced[id];
    if (data == null) continue;
    seen.add(id);
    result.add(
      ProjectChannel(
        id: id,
        name: data.name,
        channelType: data.channelType,
        isMember: false,
      ),
    );
  }
  return result;
}

/// The channel ids every project in [projects] claims, either by its own
/// `channel` tags or by a channel's relay-stamped back-reference — what the
/// Home list keeps out of its plain "Channels" section.
Set<String> projectBoundChannelIds(
  Iterable<Project> projects,
  Iterable<Channel> channels,
) {
  final bound = <String>{};
  for (final project in projects) {
    bound.addAll(project.channelIds);
    for (final channel in channels) {
      if (channel.projectRef == project.address) bound.add(channel.id);
    }
  }
  return bound;
}

/// A one-line note inside a project section.
class ProjectSectionNote extends StatelessWidget {
  final String text;
  final bool emphasise;

  const ProjectSectionNote({
    super.key,
    required this.text,
    this.emphasise = false,
  });

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.symmetric(
      horizontal: Grid.xs,
      vertical: Grid.xxs,
    ),
    child: Text(
      text,
      style: context.textTheme.bodySmall?.copyWith(
        color: emphasise
            ? context.colors.error
            : context.colors.onSurfaceVariant,
      ),
    ),
  );
}
