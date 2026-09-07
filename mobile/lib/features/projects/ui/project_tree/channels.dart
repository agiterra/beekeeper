part of '../project_tree.dart';

/// One channel of the project, with its coding sessions underneath.
///
/// The sessions channel starts expanded and every other channel collapsed:
/// each expanded channel is one live observer read, and a project with many
/// chat channels should not open one per channel just to say "none". A
/// collapsed channel says nothing about its sessions — not even "0".
class _ProjectChannelSection extends HookConsumerWidget {
  final ProjectChannel channel;
  final bool isSessionsChannel;

  /// This device's own channel record, when the list carries it.
  final Channel? myChannel;

  const _ProjectChannelSection({
    super.key,
    required this.channel,
    required this.isSessionsChannel,
    required this.myChannel,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final expanded = useState(isSessionsChannel);
    final colors = context.colors;
    final subtitle = [
      switch (channel.channelType) {
        'transport' => 'sessions channel',
        'forum' => 'forum',
        'dm' => 'direct',
        _ => 'channel',
      },
      if (!channel.isMember) 'not in your list',
    ].join(' · ');

    return AppListCard(
      children: [
        ListTile(
          key: ValueKey('project-channel-row-${channel.id}'),
          leading: Icon(
            channel.isTransport
                ? LucideIcons.squareTerminal
                : channel.channelType == 'forum'
                ? LucideIcons.messagesSquare
                : LucideIcons.hash,
            color: colors.primary,
          ),
          title: Text(
            channel.name.isEmpty ? channel.id : channel.name,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: context.textTheme.titleSmall,
          ),
          subtitle: Text(
            subtitle,
            style: context.textTheme.bodySmall?.copyWith(
              color: colors.onSurfaceVariant,
            ),
          ),
          trailing: IconButton(
            key: ValueKey('project-channel-toggle-${channel.id}'),
            tooltip: expanded.value
                ? 'Hide coding sessions'
                : 'Show coding sessions',
            icon: Icon(
              expanded.value ? LucideIcons.chevronUp : LucideIcons.chevronDown,
              size: 18,
            ),
            onPressed: () => expanded.value = !expanded.value,
          ),
          onTap: myChannel == null
              ? () => Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) => CodingSessionsPage(
                      channelId: channel.id,
                      channelName: channel.name,
                    ),
                  ),
                )
              : () => Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) => ChannelDetailPage(channel: myChannel!),
                  ),
                ),
        ),
        if (expanded.value)
          _ChannelSessions(
            key: ValueKey('project-channel-sessions-${channel.id}'),
            channelId: channel.id,
            channelName: channel.name,
          ),
      ],
    );
  }
}

/// The coding sessions of one channel, from the shared observer.
class _ChannelSessions extends ConsumerWidget {
  final String channelId;
  final String channelName;

  const _ChannelSessions({
    super.key,
    required this.channelId,
    required this.channelName,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final binding = ref.watch(codingSessionObserverBindingProvider);
    final snapshot = binding.watch(ref, channelId);
    final colors = context.colors;
    if (snapshot.sessions.isEmpty) {
      final text = snapshot.hasBlockingError
          ? 'Sessions could not be read: ${snapshot.lastError}'
          : snapshot.isDisconnected
          ? codingSessionDisconnectedLabel
          : snapshot.isLoadingFirstRead
          ? 'Reading coding sessions…'
          : codingSessionsEmptyLabel;
      return Padding(
        key: ValueKey('project-channel-sessions-note-$channelId'),
        padding: const EdgeInsets.fromLTRB(
          Grid.xs,
          Grid.xxs,
          Grid.xs,
          Grid.twelve,
        ),
        child: Text(
          text,
          style: context.textTheme.bodySmall?.copyWith(
            color: snapshot.hasBlockingError
                ? colors.error
                : colors.onSurfaceVariant,
          ),
        ),
      );
    }
    return Column(
      children: [
        for (final session in snapshot.sessions)
          ListTile(
            key: ValueKey('project-session-row-${session.key}'),
            dense: true,
            leading: const SizedBox(width: Grid.xs),
            title: Text(
              session.displayName,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
            subtitle: Text(
              [
                codingSessionFounderLabel(session.founder),
                if (session.closed) 'closed',
              ].join(' · '),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
            trailing: CodingSessionStatusChip(status: session.status),
            onTap: () => Navigator.of(context).push(
              MaterialPageRoute<void>(
                builder: (_) => CodingSessionPage(
                  channelId: channelId,
                  sessionKey: session.key,
                ),
              ),
            ),
          ),
        if (snapshot.connection == CodingSessionObserverConnection.error)
          Padding(
            padding: const EdgeInsets.fromLTRB(Grid.xs, 0, Grid.xs, Grid.xxs),
            child: Text(
              'This list may be out of date: ${snapshot.lastError}',
              style: context.textTheme.bodySmall?.copyWith(color: colors.error),
            ),
          ),
      ],
    );
  }
}
