import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../../shared/relay/nostr_models.dart';
import '../../../shared/relay/relay_provider.dart';
import '../../../shared/theme/theme.dart';
import '../../../shared/utils/string_utils.dart';
import '../../../shared/widgets/app_list_card.dart';
import '../../../shared/widgets/bee_refresh_indicator.dart';
import '../../../shared/widgets/frosted_app_bar.dart';
import '../../../shared/widgets/frosted_scaffold.dart';
import '../../channels/channel.dart';
import '../../channels/channel_detail_page.dart';
import '../../channels/channels_provider.dart';
import '../../coding_sessions/ui/coding_session_labels.dart';
import '../../coding_sessions/ui/coding_session_page.dart';
import '../../coding_sessions/ui/coding_session_status_chip.dart';
import '../../coding_sessions/ui/coding_sessions_page.dart';
import '../../coding_sessions/ui/observer_contract.dart';
import '../../profile/user_cache_provider.dart';
import '../../terminals/domain/terminals_domain.dart';
import '../../terminals/state/terminals_index_provider.dart';
import '../../terminals/ui/terminal_row.dart';
import '../domain/project_models.dart';
import '../state/projects_provider.dart';
import 'projects_page.dart';

part 'project_page/channels.dart';
part 'project_page/terminals.dart';

/// Opens a terminal from a project page; wired by the observe slice.
///
/// Until then the rows are inert, which the row itself makes visible (no
/// chevron) rather than promising a page that does not exist.
typedef ProjectTerminalOpener =
    void Function(BuildContext context, RemoteTerminal terminal);

/// The opener the project page uses; `null` leaves terminal rows inert.
final projectTerminalOpenerProvider = Provider<ProjectTerminalOpener?>(
  (ref) => null,
);

/// One project: its channels with their coding sessions, and its terminals.
///
/// Channels come from two bindings unioned as the desktop does: the head's
/// own `channel` tags and the relay-stamped `project` tag on each channel's
/// metadata. Terminals sit under the project, not a channel — a terminal
/// announce names only a project, and this page does not invent more.
class ProjectPage extends HookConsumerWidget {
  /// The `30621:<owner>:<d>` address.
  final String address;

  const ProjectPage({super.key, required this.address});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final read = ref.watch(projectsProvider);
    final project = read.byAddress(address);
    final channelsAsync = ref.watch(channelsProvider);
    final terminals = ref.watch(terminalsIndexProvider);
    final me = ref.watch(myPubkeyProvider);
    final profiles = ref.watch(userCacheProvider);
    final opener = ref.watch(projectTerminalOpenerProvider);

    Future<void> refresh() => Future.wait([
      ref.read(projectsProvider.notifier).refresh(),
      ref.read(terminalsIndexProvider.notifier).refresh(),
    ]);

    if (project == null) {
      return FrostedScaffold(
        backgroundColor: context.colors.surface,
        appBar: const FrostedAppBar(title: Text('Project')),
        body: Column(
          children: [
            SizedBox(height: frostedAppBarHeight(context)),
            Expanded(
              child: ProjectsMessage(
                key: const ValueKey('project-missing'),
                icon: LucideIcons.triangleAlert,
                title: read.hasRead
                    ? 'This project is not in the current read'
                    : 'Projects have not been read yet',
                detail:
                    read.lastError ??
                    'It may have been deleted, or the relay withholds it.',
                onRetry: refresh,
              ),
            ),
          ],
        ),
      );
    }

    final myChannels = channelsAsync.asData?.value ?? const <Channel>[];
    final channels = projectChannelsFor(
      project: project,
      myChannels: myChannels,
      referenced: read.referencedChannels,
    );
    final sessionsChannel = pickProjectSessionsChannel(project, channels);
    final shared = terminals.forProject(project.address);

    String ownerLabel(String pubkey) =>
        profiles[pubkey.toLowerCase()]?.label ?? shortPubkey(pubkey);

    return FrostedScaffold(
      backgroundColor: context.colors.surface,
      appBar: FrostedAppBar(
        title: Text(project.name, overflow: TextOverflow.ellipsis),
      ),
      body: Column(
        children: [
          SizedBox(height: frostedAppBarHeight(context)),
          Expanded(
            child: BeeRefreshIndicator(
              onRefresh: refresh,
              child: ListView(
                padding: const EdgeInsets.fromLTRB(
                  Grid.xs,
                  Grid.xxs,
                  Grid.xs,
                  Grid.xl,
                ),
                children: [
                  _ProjectHeader(project: project),
                  if (channels.isEmpty)
                    const _SectionNote(
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
                    terminals: shared,
                    index: terminals,
                    viewerPubkey: me,
                    ownerLabel: ownerLabel,
                    opener: opener,
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
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

class _ProjectHeader extends StatelessWidget {
  final Project project;

  const _ProjectHeader({required this.project});

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final members = project.memberPubkeys.length + 1;
    return Container(
      key: const ValueKey('project-header'),
      margin: const EdgeInsets.only(bottom: Grid.twelve),
      padding: const EdgeInsets.all(Grid.twelve),
      decoration: BoxDecoration(
        color: colors.surfaceContainerLow,
        borderRadius: BorderRadius.circular(Radii.card),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (project.description.trim().isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(bottom: Grid.half),
              child: Text(
                project.description.trim(),
                style: context.textTheme.bodyMedium,
              ),
            ),
          Text(
            '${project.isPrivate ? 'Private' : 'Public'} · '
            '$members ${members == 1 ? 'member' : 'members'} · '
            'owner ${shortPubkey(project.owner)}',
            style: context.textTheme.bodySmall?.copyWith(
              color: colors.onSurfaceVariant,
            ),
          ),
        ],
      ),
    );
  }
}

class _SectionNote extends StatelessWidget {
  final String text;

  const _SectionNote({super.key, required this.text});

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
