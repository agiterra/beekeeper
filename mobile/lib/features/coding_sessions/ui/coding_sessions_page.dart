import 'package:flutter/material.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../../shared/theme/theme.dart';
import '../../../shared/widgets/bee_refresh_indicator.dart';
import '../../../shared/widgets/buzz_loading_indicator.dart';
import '../../../shared/widgets/frosted_app_bar.dart';
import '../../../shared/widgets/frosted_scaffold.dart';
import '../domain/coding_sessions_domain.dart';
import 'coding_session_labels.dart';
import 'coding_session_page.dart';
import 'coding_session_status_chip.dart';
import 'observer_contract.dart';

part 'coding_sessions_page/session_card.dart';
part 'coding_sessions_page/states.dart';

/// The coding sessions a channel's members have opened, observed read-only.
///
/// Every line on this page comes from a signed fact the trust gate accepted.
/// A read that has not returned shows as loading and a read that failed shows
/// as an error — neither is ever rendered as "no coding sessions", because an
/// empty list is a claim about the channel, not about the connection.
class CodingSessionsPage extends HookConsumerWidget {
  /// The channel whose sessions are listed.
  final String channelId;

  /// The channel's display name, used only in the app bar subtitle.
  final String? channelName;

  /// Creates the coding-sessions list for [channelId].
  const CodingSessionsPage({
    super.key,
    required this.channelId,
    this.channelName,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final binding = ref.watch(codingSessionObserverBindingProvider);
    final snapshot = binding.watch(ref, channelId);

    return FrostedScaffold(
      backgroundColor: context.colors.surface,
      appBar: FrostedAppBar(
        title: Text(
          channelName == null
              ? 'Coding sessions'
              : 'Coding sessions · $channelName',
          overflow: TextOverflow.ellipsis,
        ),
      ),
      body: Column(
        children: [
          SizedBox(height: frostedAppBarHeight(context)),
          Expanded(
            child: BeeRefreshIndicator(
              onRefresh: () => binding.refresh(ref, channelId),
              child: _CodingSessionsBody(
                channelId: channelId,
                snapshot: snapshot,
                onRetry: () => binding.refresh(ref, channelId),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _CodingSessionsBody extends StatelessWidget {
  final String channelId;
  final CodingSessionObserverSnapshot snapshot;
  final Future<void> Function() onRetry;

  const _CodingSessionsBody({
    required this.channelId,
    required this.snapshot,
    required this.onRetry,
  });

  @override
  Widget build(BuildContext context) {
    if (snapshot.hasBlockingError) {
      return _CodingSessionsMessage(
        key: const ValueKey('coding-sessions-error'),
        icon: LucideIcons.triangleAlert,
        title: 'Could not read coding sessions',
        detail: snapshot.lastError ?? 'The relay read failed.',
        onRetry: onRetry,
      );
    }
    if (snapshot.isLoadingFirstRead) {
      return const _CodingSessionsLoading();
    }
    if (snapshot.sessions.isEmpty) {
      return const _CodingSessionsMessage(
        key: ValueKey('coding-sessions-empty'),
        icon: LucideIcons.terminal,
        title: codingSessionsEmptyLabel,
        detail:
            'Sessions opened from a desktop client in this channel appear '
            'here as their providers publish them.',
      );
    }

    final counts = codingSessionCountsLabel(snapshot.counts);
    final children = <Widget>[
      // A failed read behind a list that still has rows is disclosed rather
      // than hidden: what is on screen is older than the reader thinks.
      if (snapshot.connection == CodingSessionObserverConnection.error)
        _CodingSessionsNotice(
          key: const ValueKey('coding-sessions-stale'),
          text:
              'This list may be out of date: '
              '${snapshot.lastError ?? 'the relay read failed'}',
          emphasise: true,
        ),
      for (final session in snapshot.sessions)
        _SessionCard(
          session: session,
          reachability: snapshot.reachabilityFor(session.key),
          onOpen: () => Navigator.of(context).push(
            MaterialPageRoute<void>(
              builder: (_) => CodingSessionPage(
                channelId: channelId,
                sessionKey: session.key,
              ),
            ),
          ),
        ),
      if (counts != null)
        _CodingSessionsNotice(
          key: const ValueKey('coding-sessions-counts'),
          text: counts,
        ),
    ];
    return ListView.separated(
      padding: const EdgeInsets.fromLTRB(Grid.xs, Grid.xs, Grid.xs, Grid.xl),
      itemCount: children.length,
      separatorBuilder: (_, _) => const SizedBox(height: Grid.twelve),
      itemBuilder: (_, index) => children[index],
    );
  }
}
