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
    if (snapshot.isDisconnected) {
      // Not "no sessions" and not "still loading": the relay session for this
      // community is down and nothing is being read.
      return _CodingSessionsMessage(
        key: const ValueKey('coding-sessions-disconnected'),
        icon: LucideIcons.plugZap,
        title: codingSessionDisconnectedLabel,
        detail: codingSessionDisconnectedDetail,
        onRetry: onRetry,
      );
    }
    if (snapshot.isLoadingFirstRead) {
      return const _CodingSessionsLoading();
    }
    final disclosures = _disclosures(snapshot);
    if (snapshot.sessions.isEmpty) {
      // "No coding sessions in this channel" is a claim about the channel,
      // not about the read. When the read refused facts or stopped at the
      // history limit, the claim is qualified by what it cost — otherwise a
      // page that threw away a thousand signed events reads as a clean no.
      return _CodingSessionsMessage(
        key: const ValueKey('coding-sessions-empty'),
        icon: LucideIcons.terminal,
        title: codingSessionsEmptyLabel,
        detail:
            'Sessions opened from a desktop client in this channel appear '
            'here as their providers publish them.',
        notices: disclosures,
      );
    }

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
      ...disclosures,
    ];
    return ListView.separated(
      padding: const EdgeInsets.fromLTRB(Grid.xs, Grid.xs, Grid.xs, Grid.xl),
      itemCount: children.length,
      separatorBuilder: (_, _) => const SizedBox(height: Grid.twelve),
      itemBuilder: (_, index) => children[index],
    );
  }

  /// What this read cost, in the reader's terms.
  ///
  /// Shown whether or not the list has rows: both notices are admissions
  /// about the read itself, and an empty list is exactly the case where the
  /// reader most needs them.
  List<Widget> _disclosures(CodingSessionObserverSnapshot snapshot) {
    final counts = codingSessionCountsLabel(snapshot.counts);
    return [
      if (snapshot.truncatedAt1000)
        const _CodingSessionsNotice(
          key: ValueKey('coding-sessions-truncated'),
          text: codingSessionTruncatedLabel,
        ),
      if (counts != null)
        _CodingSessionsNotice(
          key: const ValueKey('coding-sessions-counts'),
          text: counts,
        ),
    ];
  }
}
