import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../../shared/theme/theme.dart';
import '../../../shared/utils/string_utils.dart';
import '../../../shared/widgets/bee_refresh_indicator.dart';
import '../../../shared/widgets/buzz_loading_indicator.dart';
import '../../../shared/widgets/frosted_app_bar.dart';
import '../../../shared/widgets/frosted_scaffold.dart';
import '../domain/coding_sessions_domain.dart';
import 'coding_session_labels.dart';
import 'coding_session_status_chip.dart';
import 'observer_contract.dart';

part 'coding_session_page/header.dart';
part 'coding_session_page/notices.dart';
part 'coding_session_page/transcript.dart';
part 'coding_session_page/rows.dart';

/// One coding session, observed read-only (D11c).
///
/// The page never publishes: there is no composer and no command. It shows
/// what the accepted facts say, marks what it could not verify, and says so
/// when the history it has is only part of the history that exists.
class CodingSessionPage extends HookConsumerWidget {
  /// The channel the session lives in.
  final String channelId;

  /// The session to open: an umbrella key, a `sessionRef`, an execution key,
  /// or a single generation's target key.
  final String sessionKey;

  /// Creates the read-only session page.
  const CodingSessionPage({
    super.key,
    required this.channelId,
    required this.sessionKey,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final binding = ref.watch(codingSessionObserverBindingProvider);
    final snapshot = binding.watch(ref, channelId);
    final session = snapshot.sessionFor(sessionKey);

    return FrostedScaffold(
      backgroundColor: context.colors.surface,
      appBar: FrostedAppBar(
        title: Text(
          session?.displayName ?? 'Coding session',
          overflow: TextOverflow.ellipsis,
        ),
      ),
      body: Column(
        children: [
          SizedBox(height: frostedAppBarHeight(context)),
          Expanded(
            child: BeeRefreshIndicator(
              onRefresh: () => binding.refresh(ref, channelId),
              child: _CodingSessionBody(
                snapshot: snapshot,
                session: session,
                onRetry: () => binding.refresh(ref, channelId),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _CodingSessionBody extends StatelessWidget {
  final CodingSessionObserverSnapshot snapshot;
  final CodingSessionUmbrella? session;
  final Future<void> Function() onRetry;

  const _CodingSessionBody({
    required this.snapshot,
    required this.session,
    required this.onRetry,
  });

  @override
  Widget build(BuildContext context) {
    final resolved = session;
    if (resolved == null) {
      return _MissingSession(snapshot: snapshot, onRetry: onRetry);
    }

    final blocks = snapshot.blocksFor(resolved);
    final counts = codingSessionCountsLabel(snapshot.counts);
    final refusedByKind = codingSessionRefusedByKindLabel(snapshot.counts);
    final children = <Widget>[
      if (snapshot.connection == CodingSessionObserverConnection.error)
        _SessionNotice(
          key: const ValueKey('coding-session-stale'),
          icon: LucideIcons.triangleAlert,
          text:
              'This transcript may be out of date: '
              '${snapshot.lastError ?? 'the relay read failed'}',
          emphasise: true,
        ),
      if (snapshot.signaturesVerified == false)
        const _SessionNotice(
          key: ValueKey('coding-session-signatures-unverified'),
          icon: LucideIcons.shieldAlert,
          text: codingSessionUnverifiedSignaturesLabel,
          emphasise: true,
        ),
      _SessionHeader(session: resolved, snapshot: snapshot),
      if (snapshot.truncatedAt1000)
        const _SessionNotice(
          key: ValueKey('coding-session-truncated'),
          icon: LucideIcons.history,
          text: codingSessionTruncatedLabel,
        ),
      // What this device dropped for *this* session's generations (plus the
      // shared buckets its authority and founder are read from). A transcript
      // this device shortened must not read as a short transcript.
      if (snapshot.evictedFor(resolved) > 0)
        const _SessionNotice(
          key: ValueKey('coding-session-evicted'),
          icon: LucideIcons.trash2,
          text: codingSessionEvictedLabel,
        ),
      if (counts != null)
        _SessionNotice(
          key: const ValueKey('coding-session-counts'),
          icon: LucideIcons.info,
          text: counts,
        ),
      // The same losses named per kind and per reason: an unauthorized signer
      // and an invalid signature are different accusations, and a transcript
      // refused is a different loss from a name refused.
      if (refusedByKind != null)
        _SessionNotice(
          key: const ValueKey('coding-session-counts-by-kind'),
          icon: LucideIcons.listTree,
          text: refusedByKind,
        ),
      if (blocks.isEmpty)
        const _SessionNotice(
          key: ValueKey('coding-session-no-transcript'),
          icon: LucideIcons.fileText,
          text: 'No transcript has been published for this session yet',
        )
      else
        for (final block in blocks)
          _TranscriptBlockView(
            key: ValueKey('coding-session-block-${block.key}'),
            block: block,
            showLabel: blocks.length > 1 || resolved.executions.length > 1,
          ),
      const _ReadOnlyFooter(),
    ];

    return ListView.builder(
      padding: const EdgeInsets.fromLTRB(Grid.xs, Grid.xs, Grid.xs, Grid.xl),
      itemCount: children.length,
      itemBuilder: (_, index) => children[index],
    );
  }
}

/// The session the route asked for is not in the current read.
class _MissingSession extends StatelessWidget {
  final CodingSessionObserverSnapshot snapshot;
  final Future<void> Function() onRetry;

  const _MissingSession({required this.snapshot, required this.onRetry});

  @override
  Widget build(BuildContext context) {
    if (snapshot.isDisconnected) {
      return _CodingSessionMessage(
        key: const ValueKey('coding-session-disconnected'),
        icon: LucideIcons.plugZap,
        title: codingSessionDisconnectedLabel,
        detail: codingSessionDisconnectedDetail,
        onRetry: onRetry,
      );
    }
    if (snapshot.isLoadingFirstRead) {
      return ListView(
        key: const ValueKey('coding-session-loading'),
        padding: const EdgeInsets.only(top: Grid.xxl),
        children: const [
          Center(
            child: BuzzLoadingIndicator(
              size: 40,
              semanticLabel: 'Reading this coding session',
            ),
          ),
        ],
      );
    }
    return _CodingSessionMessage(
      key: const ValueKey('coding-session-missing'),
      icon: LucideIcons.triangleAlert,
      title: 'This session is not in the current read',
      detail:
          snapshot.lastError ??
          'Nothing this device accepted names it. It may be outside the '
              'history that was fetched, or its facts were refused.',
      onRetry: onRetry,
    );
  }
}

/// A full-height message with a Retry, kept scrollable for pull-to-refresh.
class _CodingSessionMessage extends StatelessWidget {
  final IconData icon;
  final String title;
  final String detail;
  final Future<void> Function() onRetry;

  const _CodingSessionMessage({
    super.key,
    required this.icon,
    required this.title,
    required this.detail,
    required this.onRetry,
  });

  @override
  Widget build(BuildContext context) => ListView(
    padding: const EdgeInsets.fromLTRB(Grid.gutter, Grid.xxl, Grid.gutter, 0),
    children: [
      Icon(icon, size: 32, color: context.colors.onSurfaceVariant),
      const SizedBox(height: Grid.twelve),
      Text(
        title,
        textAlign: TextAlign.center,
        style: context.textTheme.titleSmall,
      ),
      const SizedBox(height: Grid.half),
      Text(
        detail,
        textAlign: TextAlign.center,
        style: context.textTheme.bodySmall?.copyWith(
          color: context.colors.onSurfaceVariant,
        ),
      ),
      const SizedBox(height: Grid.xs),
      Center(
        child: TextButton.icon(
          key: const ValueKey('coding-session-retry'),
          onPressed: onRetry,
          icon: const Icon(LucideIcons.refreshCw, size: 16),
          label: const Text('Retry'),
        ),
      ),
    ],
  );
}
