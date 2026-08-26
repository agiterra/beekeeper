part of '../channel_detail_page.dart';

const _dmHeaderAvatarSize = 32.0;
const _dmPresenceDotRatio = 8 / 14;

bool _showsMembersAction(Channel channel) {
  if (!channel.isDm) return true;
  final participants = channel.participantPubkeys
      .map((pubkey) => pubkey.toLowerCase())
      .toSet();
  return participants.length != 2;
}

double _scaledTextHeight(BuildContext context, TextStyle style) {
  final scaledFontSize = MediaQuery.textScalerOf(
    context,
  ).scale(style.fontSize ?? 0);
  return scaledFontSize * (style.height ?? 1);
}

double _dmAppBarTitleContentHeight(BuildContext context) {
  final titleStyle = context.textTheme.titleSmall;
  final presenceStyle = context.textTheme.bodyMedium;
  if (titleStyle == null || presenceStyle == null) {
    return _dmHeaderAvatarSize;
  }
  final textHeight =
      _scaledTextHeight(context, titleStyle) +
      _scaledTextHeight(context, presenceStyle);
  return textHeight > _dmHeaderAvatarSize ? textHeight : _dmHeaderAvatarSize;
}

/// The channel header's member actions, plus the coding-session observer
/// beside them.
///
/// The coding-session action lives here rather than in the page's own
/// `actions:` list because this part file is the only header surface this
/// change owns; it renders as a second compact icon next to Members.
class _MembersButton extends ConsumerWidget {
  final String channelId;
  final Channel channel;
  final String? currentPubkey;

  const _MembersButton({
    required this.channelId,
    required this.channel,
    required this.currentPubkey,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) => Row(
    mainAxisSize: MainAxisSize.min,
    children: [
      _CodingSessionsButton(channel: channel),
      _MembersIconButton(
        channelId: channelId,
        channel: channel,
        currentPubkey: currentPubkey,
      ),
    ],
  );
}

/// Opens the read-only coding-session observer for this channel.
class _CodingSessionsButton extends StatelessWidget {
  final Channel channel;

  const _CodingSessionsButton({required this.channel});

  @override
  Widget build(BuildContext context) => IconButton(
    key: const ValueKey('channel-coding-sessions-action'),
    color: context.colors.primary,
    tooltip: 'Coding sessions',
    onPressed: () => Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => CodingSessionsPage(
          channelId: channel.id,
          channelName: channel.isDm ? null : channel.name,
        ),
      ),
    ),
    icon: const Icon(LucideIcons.squareTerminal, size: 22),
  );
}

class _MembersIconButton extends ConsumerWidget {
  final String channelId;
  final Channel channel;
  final String? currentPubkey;

  const _MembersIconButton({
    required this.channelId,
    required this.channel,
    required this.currentPubkey,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final hasWorkingBot = ref
        .watch(workingBotPubkeysProvider(channelId))
        .isNotEmpty;

    return IconButton(
      color: context.colors.primary,
      onPressed: () {
        showBuzzModalBottomSheet<void>(
          context: context,
          title: 'Members',
          isScrollControlled: true,
          showDragHandle: true,
          builder: (_) =>
              MembersSheet(channel: channel, currentPubkey: currentPubkey),
        );
      },
      tooltip: 'View members',
      icon: Stack(
        clipBehavior: Clip.none,
        children: [
          const Icon(LucideIcons.users, size: 22),
          if (hasWorkingBot)
            Positioned(
              top: -2,
              right: -2,
              child: Container(
                width: 8,
                height: 8,
                decoration: BoxDecoration(
                  color: context.appColors.success,
                  shape: BoxShape.circle,
                  border: Border.all(color: context.colors.surface, width: 1.5),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _DmAppBarTitle extends ConsumerWidget {
  final Channel channel;
  final String? currentPubkey;

  const _DmAppBarTitle({required this.channel, required this.currentPubkey});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final normalizedCurrent = currentPubkey?.toLowerCase();

    String? otherPubkey;
    for (final pk in channel.participantPubkeys) {
      if (pk.toLowerCase() != normalizedCurrent) {
        otherPubkey = pk.toLowerCase();
        break;
      }
    }

    final profile = ref.watch(
      userCacheProvider.select(
        (profiles) => otherPubkey == null ? null : profiles[otherPubkey],
      ),
    );
    final presence = ref.watch(
      presenceCacheProvider.select(
        (presenceMap) => otherPubkey == null
            ? 'offline'
            : (presenceMap[otherPubkey] ?? 'offline'),
      ),
    );

    if (otherPubkey != null) {
      if (profile == null) {
        ref.read(userCacheProvider.notifier).preload([otherPubkey]);
      }
      ref.read(presenceCacheProvider.notifier).track([otherPubkey]);
    }

    final avatarUrl = profile?.avatarUrl;
    final animatedAvatar = parseAnimatedAvatarUrl(avatarUrl);
    final initial =
        profile?.initial ??
        (channel.participants.isNotEmpty
            ? channel.participants.first[0].toUpperCase()
            : '?');
    final presenceLabel = switch (presence) {
      'online' => 'Online',
      'away' => 'Away',
      _ => 'Offline',
    };

    return Row(
      children: [
        MaskedAvatarBadge(
          key: const ValueKey('dm-header-avatar'),
          size: _dmHeaderAvatarSize,
          geometry: AvatarBadgeMaskGeometry.presenceDot,
          avatar: ClipOval(
            child: ColoredBox(
              color: animatedAvatar == null
                  ? context.colors.primaryContainer
                  : Colors.transparent,
              child: AvatarImageContent(
                imageUrl: animatedAvatar?.posterUrl ?? avatarUrl,
                fallback: Text(
                  initial,
                  style: context.textTheme.labelSmall?.copyWith(
                    color: context.colors.onPrimaryContainer,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ),
          ),
          badge: Center(
            child: FractionallySizedBox(
              widthFactor: _dmPresenceDotRatio,
              heightFactor: _dmPresenceDotRatio,
              child: DecoratedBox(
                decoration: BoxDecoration(
                  color: switch (presence) {
                    'online' => context.appColors.success,
                    'away' => context.appColors.warning,
                    _ => context.colors.outline,
                  },
                  shape: BoxShape.circle,
                ),
              ),
            ),
          ),
        ),
        const SizedBox(width: Grid.xxs),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Flexible(
                    child: Text(
                      resolveDmChannelDisplayLabel(
                        channel,
                        currentPubkey: currentPubkey,
                      ),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      key: const ValueKey('dm-header-name'),
                      style: context.textTheme.titleSmall,
                    ),
                  ),
                  if (channel.isEphemeral) ...[
                    const SizedBox(width: Grid.quarter),
                    _HeaderEphemeralBadge(channel: channel),
                  ],
                ],
              ),
              Text(
                presenceLabel,
                key: const ValueKey('dm-header-presence'),
                style: context.textTheme.bodyMedium?.copyWith(
                  color: context.colors.onSurfaceVariant,
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }
}
