import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../../shared/relay/nostr_models.dart';
import '../../../shared/relay/relay_provider.dart';
import '../../../shared/theme/theme.dart';
import '../../../shared/utils/string_utils.dart';
import '../../../shared/widgets/app_list_card.dart';
import '../../channels/channel.dart';
import '../../channels/channel_detail_page.dart';
import '../../coding_sessions/ui/coding_session_labels.dart';
import '../../coding_sessions/ui/coding_session_page.dart';
import '../../coding_sessions/ui/coding_session_status_chip.dart';
import '../../coding_sessions/ui/coding_sessions_page.dart';
import '../../coding_sessions/ui/observer_contract.dart';
import '../../profile/user_cache_provider.dart';
import '../../terminals/domain/terminals_domain.dart';
import '../../terminals/state/terminals_index_provider.dart';
import '../../terminals/ui/terminal_observe_page.dart';
import '../../terminals/ui/terminal_row.dart';
import '../domain/project_models.dart';
import '../state/projects_provider.dart';

part 'project_tree/channels.dart';
part 'project_tree/terminals.dart';

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

/// One project's contents: its channels with their coding sessions, and the
/// terminals shared under it.
///
/// Rendered inline on the Home screen under the project's header, so the
/// hierarchy is one list: project → channel → sessions, with terminals as a
/// sibling of the channels. A kind:30623 announce names only a project, so
/// a terminal cannot honestly sit under a channel.
///
/// Channels come from two bindings unioned as the desktop does: the head's
/// own `channel` tags and the relay-stamped `project` tag on each channel's
/// metadata. The sessions channel starts expanded and every other channel
/// collapsed: each expanded channel is one live observer read.
class ProjectTree extends ConsumerWidget {
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

    final channels = projectChannelsFor(
      project: project,
      myChannels: myChannels,
      referenced: read.referencedChannels,
    );
    final sessionsChannel = pickProjectSessionsChannel(project, channels);
    String ownerLabel(String pubkey) =>
        profiles[pubkey.toLowerCase()]?.label ?? shortPubkey(pubkey);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (channels.isEmpty)
          const ProjectSectionNote(
            key: ValueKey('project-no-channels'),
            text: 'No channel is bound to this project yet',
          )
        else
          for (final channel in channels)
            _ProjectChannelSection(
              key: ValueKey('project-channel-${channel.id}'),
              channel: channel,
              isSessionsChannel: channel.id == sessionsChannel?.id,
              myChannel: _channelById(myChannels, channel.id),
            ),
        _ProjectTerminalsSection(
          project: project,
          terminals: terminals.forProject(project.address),
          index: terminals,
          viewerPubkey: me,
          ownerLabel: ownerLabel,
          opener: opener,
        ),
      ],
    );
  }

  static Channel? _channelById(List<Channel> channels, String id) {
    for (final channel in channels) {
      if (channel.id == id) return channel;
    }
    return null;
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
  result.sort((left, right) {
    // The sessions transport first, then by name.
    if (left.isTransport != right.isTransport) {
      return left.isTransport ? -1 : 1;
    }
    return left.name.toLowerCase().compareTo(right.name.toLowerCase());
  });
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

  const ProjectSectionNote({super.key, required this.text});

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.symmetric(
      horizontal: Grid.xxs,
      vertical: Grid.xs,
    ),
    child: Text(
      text,
      style: context.textTheme.bodySmall?.copyWith(
        color: context.colors.onSurfaceVariant,
      ),
    ),
  );
}
