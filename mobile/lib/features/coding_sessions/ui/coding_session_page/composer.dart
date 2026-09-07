part of '../coding_session_page.dart';

/// The composer: sends a turn to, or interrupts, the session's live execution.
///
/// Rendered only when [standing] lets this device steer; the disclosure for
/// every other standing lives in the transcript list, not here. The editor
/// clears and the pending row appears *before* the relay is awaited, and both
/// are undone if the publish fails — the one outcome where the words were
/// never sent (desktop `CodingSessionComposer.tsx`).
class _SessionComposer extends HookConsumerWidget {
  final CodingSessionUmbrella session;
  final CodingSessionObserverBinding binding;
  final CodingSessionSteerStanding standing;

  /// A draft a refused row handed back; the editor adopts it when it changes.
  final ValueNotifier<_RestoredDraft?> restoredDraft;

  const _SessionComposer({
    required this.session,
    required this.binding,
    required this.standing,
    required this.restoredDraft,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final candidates = _steerableExecutions(session);
    final selectedKey = useState<String?>(null);
    final controller = useTextEditingController();
    final text = useState('');
    final sending = useState(false);
    final error = useState<String?>(null);
    final restored = useValueListenable(restoredDraft);

    useEffect(() {
      void listen() => text.value = controller.text;
      controller.addListener(listen);
      return () => controller.removeListener(listen);
    }, [controller]);

    useEffect(() {
      if (restored == null) return null;
      controller.text = restored.draft;
      controller.selection = TextSelection.collapsed(
        offset: restored.draft.length,
      );
      if (restored.executionKey != null) {
        selectedKey.value = restored.executionKey;
      }
      return null;
    }, [restored]);

    if (!codingSessionMaySteer(standing)) return const SizedBox.shrink();
    if (candidates.isEmpty) {
      return _ComposerShell(
        key: const ValueKey('coding-session-composer-unavailable'),
        child: Text(
          codingSessionNoLiveExecutionLabel,
          style: context.textTheme.bodySmall?.copyWith(
            color: context.colors.onSurfaceVariant,
          ),
        ),
      );
    }

    final target = candidates.firstWhere(
      (execution) => execution.executionKey == selectedKey.value,
      orElse: () => candidates.first,
    );
    final isWorking = target.status == CodingSessionStatus.running;
    final canSteer = target.metadata?.capabilities['threadSteer'] == true;
    final canSend = text.value.trim().isNotEmpty && !sending.value;

    Future<void> run(Future<void> Function() action) async {
      sending.value = true;
      error.value = null;
      try {
        await action();
        binding.markSteerAccepted(ref, session.key);
      } on CodingSessionPublishException catch (failure) {
        error.value = failure.message;
      } finally {
        sending.value = false;
      }
    }

    Future<void> send() async {
      final draft = controller.text;
      final body = draft.trim();
      if (body.isEmpty || sending.value) return;
      // Cleared before the relay answers: the pending row is the claim now,
      // and it says "Sending…" until the relay does. A failure hands the
      // draft back below.
      controller.clear();
      final commands = binding.commands(ref, session.channelId);
      await run(() async {
        try {
          await commands.sendTurn(
            execution: target,
            text: body,
            draft: draft,
            // Only steer when the runtime promised it; otherwise this is a
            // boundary send, which is what the button said.
            deliver: isWorking && canSteer
                ? CodingSessionTurnDelivery.steer
                : CodingSessionTurnDelivery.boundary,
          );
        } on CodingSessionPublishException {
          controller.text = draft;
          controller.selection = TextSelection.collapsed(offset: draft.length);
          rethrow;
        }
      });
    }

    Future<void> interrupt() async {
      final commands = binding.commands(ref, session.channelId);
      await run(() => commands.interrupt(target));
    }

    return _ComposerShell(
      key: const ValueKey('coding-session-composer'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (error.value case final message?)
            Padding(
              padding: const EdgeInsets.only(bottom: Grid.xxs),
              child: Text(
                message,
                key: const ValueKey('coding-session-composer-error'),
                style: context.textTheme.bodySmall?.copyWith(
                  color: context.colors.error,
                ),
              ),
            ),
          if (candidates.length > 1)
            Padding(
              padding: const EdgeInsets.only(bottom: Grid.xxs),
              child: Wrap(
                spacing: Grid.xxs,
                children: [
                  for (final execution in candidates)
                    ChoiceChip(
                      key: ValueKey(
                        'coding-session-composer-target-'
                        '${execution.targetKey}',
                      ),
                      label: Text(execution.label),
                      selected: execution.executionKey == target.executionKey,
                      onSelected: (_) =>
                          selectedKey.value = execution.executionKey,
                    ),
                ],
              ),
            ),
          Row(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              Expanded(
                child: TextField(
                  key: const ValueKey('coding-session-composer-field'),
                  controller: controller,
                  enabled: !sending.value,
                  minLines: 1,
                  maxLines: 5,
                  textInputAction: TextInputAction.newline,
                  decoration: InputDecoration(
                    hintText: isWorking
                        ? 'Message ${target.label} (runs at the next turn)'
                        : 'Message ${target.label}',
                    isDense: true,
                    border: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(Radii.lg),
                    ),
                  ),
                ),
              ),
              const SizedBox(width: Grid.xxs),
              if (isWorking)
                IconButton.outlined(
                  key: const ValueKey('coding-session-interrupt'),
                  tooltip: 'Interrupt',
                  onPressed: sending.value ? null : interrupt,
                  icon: const Icon(LucideIcons.octagonX, size: 18),
                ),
              IconButton.filled(
                key: const ValueKey('coding-session-send'),
                tooltip: codingSessionSendLabel(
                  isWorking: isWorking,
                  canSteer: canSteer,
                ),
                onPressed: canSend ? send : null,
                icon: Icon(
                  isWorking && !canSteer
                      ? LucideIcons.listPlus
                      : LucideIcons.send,
                  size: 18,
                ),
              ),
            ],
          ),
          Padding(
            padding: const EdgeInsets.only(top: Grid.quarter),
            child: Text(
              '${codingSessionSendLabel(isWorking: isWorking, canSteer: canSteer)}'
              ' · ${target.label} · gen ${target.target.generation}',
              key: const ValueKey('coding-session-composer-hint'),
              style: context.textTheme.labelSmall?.copyWith(
                color: context.colors.onSurfaceVariant,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// A draft handed back to the editor by a refused or dropped row.
@immutable
class _RestoredDraft {
  final String draft;
  final String? executionKey;

  const _RestoredDraft(this.draft, {this.executionKey});
}

/// The executions a turn may be addressed to: current generations that are
/// not stopped, failed, or disconnected.
List<CodingSessionExecution> _steerableExecutions(
  CodingSessionUmbrella session,
) => [
  for (final execution in session.executions)
    if (execution.isCurrentGeneration &&
        !execution.status.isStopped &&
        execution.status != CodingSessionStatus.disconnected)
      execution,
];

/// The bar the composer sits in, above the home indicator.
class _ComposerShell extends StatelessWidget {
  final Widget child;

  const _ComposerShell({super.key, required this.child});

  @override
  Widget build(BuildContext context) => Material(
    color: context.colors.surfaceContainerLow,
    child: SafeArea(
      top: false,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(
          Grid.twelve,
          Grid.xxs,
          Grid.twelve,
          Grid.xxs,
        ),
        child: child,
      ),
    ),
  );
}

/// One turn this device sent, shown where the transcript will show it once
/// the provider answers.
class _PendingTurnRow extends StatelessWidget {
  final CodingSessionPendingTurnView view;
  final VoidCallback onDismiss;
  final VoidCallback onEdit;
  final VoidCallback? onReaddress;

  const _PendingTurnRow({
    super.key,
    required this.view,
    required this.onDismiss,
    required this.onEdit,
    this.onReaddress,
  });

  @override
  Widget build(BuildContext context) {
    final failed = view.failed;
    return _RowShell(
      itemKey: 'pending-${view.turn.commandId}',
      icon: failed ? LucideIcons.circleX : LucideIcons.user,
      title: codingSessionPendingPhaseLabel(view),
      emphasise: failed,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SelectableText(view.turn.text, style: context.textTheme.bodyMedium),
          if (failed)
            Wrap(
              spacing: Grid.xxs,
              children: [
                TextButton(
                  key: ValueKey(
                    'coding-session-pending-edit-${view.turn.commandId}',
                  ),
                  onPressed: onEdit,
                  child: const Text('Edit and resend'),
                ),
                if (view.readdressGeneration case final generation?)
                  TextButton(
                    key: ValueKey(
                      'coding-session-pending-readdress-'
                      '${view.turn.commandId}',
                    ),
                    onPressed: onReaddress,
                    child: Text('Resend to generation $generation'),
                  ),
                TextButton(
                  key: ValueKey(
                    'coding-session-pending-dismiss-${view.turn.commandId}',
                  ),
                  onPressed: onDismiss,
                  child: const Text('Dismiss'),
                ),
              ],
            ),
        ],
      ),
    );
  }
}

/// The line that replaces the composer when this device may not steer.
class _SteerDisclosure extends StatelessWidget {
  final CodingSessionSteerStanding standing;

  const _SteerDisclosure({required this.standing});

  @override
  Widget build(BuildContext context) => Padding(
    key: const ValueKey('coding-session-steer-disclosure'),
    padding: const EdgeInsets.only(top: Grid.xs),
    child: Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Icon(LucideIcons.eye, size: 14, color: context.colors.onSurfaceVariant),
        const SizedBox(width: Grid.xxs),
        Expanded(
          child: Text(
            codingSessionSteerDisclosure(standing),
            style: context.textTheme.bodySmall?.copyWith(
              color: context.colors.onSurfaceVariant,
            ),
          ),
        ),
      ],
    ),
  );
}
