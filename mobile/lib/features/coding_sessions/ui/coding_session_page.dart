import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../../shared/theme/theme.dart';
import '../../../shared/utils/string_utils.dart';
import '../../../shared/widgets/bee_refresh_indicator.dart';
import '../../../shared/widgets/beekeeper_loading_indicator.dart';
import '../../../shared/widgets/frosted_app_bar.dart';
import '../../../shared/widgets/frosted_scaffold.dart';
import '../../../shared/widgets/modal_presentation.dart';
import '../domain/coding_sessions_domain.dart';
import '../state/coding_sessions_state.dart' show CodingSessionPublishException;
import 'coding_session_labels.dart';
import 'coding_session_status_chip.dart';
import 'coding_session_title_origin.dart';
import 'observer_contract.dart';

part 'coding_session_page/actions.dart';
part 'coding_session_page/composer.dart';
part 'coding_session_page/header.dart';
part 'coding_session_page/notices.dart';
part 'coding_session_page/transcript.dart';
part 'coding_session_page/rows.dart';

/// One coding session: its accepted facts, and — for the founder or a
/// granted operator — a composer to steer it.
///
/// The read side is unchanged from the observer (D3–D10): the page shows what
/// the accepted facts say, marks what it could not verify, and says so when
/// the history it has is only part of the history that exists. Since
/// 2026-09-07 it also publishes: turns, interrupts, stops, and the umbrella's
/// name, goal and closure. Every publish is settled by the relay's `OK` and
/// then by the provider's signed receipt, never by this page's own optimism.
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
    final restoredDraft = useValueNotifier<_RestoredDraft?>(null);

    final standing = session == null
        ? CodingSessionSteerStanding.founderUnresolved
        : codingSessionSteerStanding(
            session: session,
            myPubkey: binding.signerPubkey(ref),
            relayAcceptedMine: binding
                .watchSteerAccepted(ref)
                .contains(session.key),
          );
    final maySteer = session != null && codingSessionMaySteer(standing);

    // The rows this device is still owed an answer on, settled against the
    // receipts the trust gate accepted (D4). A settled row leaves the store
    // after the frame, never during a build.
    final pendingViews = <CodingSessionPendingTurnView>[];
    if (session != null) {
      final pending = binding.watchPendingTurns(ref);
      for (final execution in session.executions) {
        if (!execution.isCurrentGeneration) continue;
        for (final turn in pending.forExecution(
          channelId,
          execution.executionKey,
        )) {
          pendingViews.add(
            settleCodingSessionPendingTurn(
              turn,
              snapshot.turnReceiptsByCommandId[turn.commandId] ?? const [],
              currentGeneration: execution.target.generation,
            ),
          );
        }
      }
    }
    final settledKeys = [
      for (final view in pendingViews)
        if (view.settled) view.turn.key,
    ];
    useEffect(() {
      if (settledKeys.isEmpty) return null;
      Future<void>.microtask(() {
        for (final key in settledKeys) {
          binding.forgetPendingTurn(ref, key);
        }
      });
      return null;
    }, [settledKeys.join('\n')]);

    return FrostedScaffold(
      backgroundColor: context.colors.surface,
      appBar: FrostedAppBar(
        title: Text(
          session?.displayName ?? 'Coding session',
          overflow: TextOverflow.ellipsis,
        ),
        actions: [
          if (session != null && maySteer)
            _SessionActionsMenu(session: session, binding: binding),
        ],
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
                standing: standing,
                onRetry: () => binding.refresh(ref, channelId),
                pendingRows: [
                  for (final view in pendingViews)
                    if (!view.settled)
                      _PendingTurnRow(
                        key: ValueKey(
                          'coding-session-pending-${view.turn.commandId}',
                        ),
                        view: view,
                        onDismiss: () =>
                            binding.forgetPendingTurn(ref, view.turn.key),
                        onEdit: () {
                          restoredDraft.value = _RestoredDraft(
                            view.turn.draft,
                            executionKey: view.turn.executionKey,
                          );
                          binding.forgetPendingTurn(ref, view.turn.key);
                        },
                        onReaddress: view.readdressGeneration == null
                            ? null
                            : () {
                                // The successor generation shares the
                                // execution key, so the composer's target
                                // resolves to it; the words come back too.
                                restoredDraft.value = _RestoredDraft(
                                  view.turn.draft,
                                  executionKey: view.turn.executionKey,
                                );
                                binding.forgetPendingTurn(ref, view.turn.key);
                              },
                      ),
                ],
              ),
            ),
          ),
          // A founded session has no execution to address, so there is no
          // composer at all — not the "every generation is stopped" line,
          // which would claim generations that never existed. The header
          // says where to start it.
          if (session != null && !session.isFounded)
            _SessionComposer(
              session: session,
              binding: binding,
              standing: standing,
              restoredDraft: restoredDraft,
            ),
        ],
      ),
    );
  }
}

class _CodingSessionBody extends StatelessWidget {
  final CodingSessionObserverSnapshot snapshot;
  final CodingSessionUmbrella? session;
  final CodingSessionSteerStanding standing;
  final Future<void> Function() onRetry;

  /// Turns this device sent that no receipt has settled yet.
  final List<Widget> pendingRows;

  const _CodingSessionBody({
    required this.snapshot,
    required this.session,
    required this.standing,
    required this.onRetry,
    required this.pendingRows,
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
      if (blocks.isEmpty && pendingRows.isEmpty)
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
      ...pendingRows,
      // The disclosure explains a missing composer; a founded session has
      // none for everybody, which the header already says.
      if (!codingSessionMaySteer(standing) && !resolved.isFounded)
        _SteerDisclosure(standing: standing),
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
            child: BeekeeperLoadingIndicator(
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
