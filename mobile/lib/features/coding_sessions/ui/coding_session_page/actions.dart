part of '../coding_session_page.dart';

/// The app-bar menu: rename, set goal, close or reopen, stop an execution.
///
/// Every item publishes a member-signed fact and discloses the relay's answer
/// verbatim. Nothing here is optimistic: the fold renders the accepted name,
/// goal or closure when the relay echoes it, so the sheet only says "Saving…"
/// until the relay's `OK`, then closes.
class _SessionActionsMenu extends ConsumerWidget {
  final CodingSessionUmbrella session;
  final CodingSessionObserverBinding binding;

  const _SessionActionsMenu({required this.session, required this.binding});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final hasUmbrella = session.sessionRef != null;
    final genesisRef = session.founder.genesisRef;
    final stoppable = [
      for (final execution in session.executions)
        if (execution.isCurrentGeneration && !execution.status.isStopped)
          execution,
    ];
    return PopupMenuButton<_SessionAction>(
      key: const ValueKey('coding-session-actions'),
      tooltip: 'Session actions',
      icon: const Icon(LucideIcons.ellipsisVertical, size: 20),
      onSelected: (action) => _run(context, ref, action, stoppable),
      itemBuilder: (context) => [
        PopupMenuItem(
          key: const ValueKey('coding-session-action-rename'),
          value: _SessionAction.rename,
          enabled: hasUmbrella,
          child: const Text('Rename'),
        ),
        PopupMenuItem(
          key: const ValueKey('coding-session-action-goal'),
          value: _SessionAction.goal,
          enabled: hasUmbrella,
          child: const Text('Set goal'),
        ),
        PopupMenuItem(
          key: const ValueKey('coding-session-action-closure'),
          value: session.closed ? _SessionAction.reopen : _SessionAction.close,
          enabled: hasUmbrella && genesisRef != null,
          child: Text(session.closed ? 'Reopen' : 'Close'),
        ),
        if (genesisRef == null)
          const PopupMenuItem(
            enabled: false,
            height: 32,
            child: Text(
              'No readable genesis — closing needs its id',
              style: TextStyle(fontSize: 12),
            ),
          ),
        // A founded session has no execution to stop, so the item is absent
        // rather than disabled: a greyed "Stop" would imply one exists.
        if (!session.isFounded)
          PopupMenuItem(
            key: const ValueKey('coding-session-action-stop'),
            value: _SessionAction.stop,
            enabled: stoppable.isNotEmpty,
            child: const Text('Stop execution'),
          ),
      ],
    );
  }

  Future<void> _run(
    BuildContext context,
    WidgetRef ref,
    _SessionAction action,
    List<CodingSessionExecution> stoppable,
  ) async {
    final commands = binding.commands(ref, session.channelId);
    switch (action) {
      case _SessionAction.rename:
        await _showTextSheet(
          context,
          title: 'Rename session',
          // A generated title is offered as the starting text, as the
          // desktop's rename dialog does; saving signs it as the person's
          // name, and the "Auto-named" label goes with it.
          initial:
              session.name ??
              (session.resolvedName.isGenerated
                  ? session.resolvedName.name
                  : ''),
          hint: 'A name for this session',
          multiline: false,
          maxBytes: maxCodingSessionNameBytes,
          onSave: (value) => commands.rename(session, value),
        );
      case _SessionAction.goal:
        await _showTextSheet(
          context,
          title: 'Set goal',
          initial: session.goal ?? '',
          hint: 'What this session is for',
          multiline: true,
          maxBytes: maxCodingSessionGoalBytes,
          onSave: (value) => commands.setGoal(session, value),
        );
      case _SessionAction.close:
        await _confirmAndRun(
          context,
          title: 'Close this session?',
          body:
              'A closure is signed by you and admitted by the relay only '
              'from the genesis signer. It can be reopened later.',
          confirmLabel: 'Close session',
          action: () => commands.close(session),
        );
      case _SessionAction.reopen:
        await _confirmAndRun(
          context,
          title: 'Reopen this session?',
          body: 'The session will read as open again for everyone.',
          confirmLabel: 'Reopen',
          action: () => commands.reopen(session),
        );
      case _SessionAction.stop:
        if (stoppable.isEmpty) return;
        final execution = stoppable.length == 1
            ? stoppable.first
            : await _pickExecution(context, stoppable);
        if (execution == null || !context.mounted) return;
        await _confirmAndRun(
          context,
          title: 'Stop ${execution.label}?',
          body:
              'This ends the execution for good; a stopped generation is not '
              'resumed. The provider, not this phone, decides whether your '
              'key may stop it — a refusal will be shown here.',
          confirmLabel: 'Stop execution',
          destructive: true,
          action: () => commands.stop(execution),
        );
    }
  }

  Future<CodingSessionExecution?> _pickExecution(
    BuildContext context,
    List<CodingSessionExecution> executions,
  ) => showBuzzModalBottomSheet<CodingSessionExecution>(
    context: context,
    title: 'Which execution?',
    builder: (sheetContext) => ListView(
      shrinkWrap: true,
      children: [
        for (final execution in executions)
          ListTile(
            key: ValueKey('coding-session-stop-pick-${execution.targetKey}'),
            title: Text(execution.label),
            subtitle: Text(
              'gen ${execution.target.generation} · '
              '${codingSessionStatusWords(execution.status)}',
            ),
            onTap: () => Navigator.of(sheetContext).pop(execution),
          ),
      ],
    ),
  );

  Future<void> _confirmAndRun(
    BuildContext context, {
    required String title,
    required String body,
    required String confirmLabel,
    required Future<void> Function() action,
    bool destructive = false,
  }) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: Text(title),
        content: Text(body),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(false),
            child: const Text('Cancel'),
          ),
          TextButton(
            key: const ValueKey('coding-session-confirm'),
            onPressed: () => Navigator.of(dialogContext).pop(true),
            style: destructive
                ? TextButton.styleFrom(
                    foregroundColor: Theme.of(dialogContext).colorScheme.error,
                  )
                : null,
            child: Text(confirmLabel),
          ),
        ],
      ),
    );
    if (confirmed != true || !context.mounted) return;
    try {
      await action();
    } on CodingSessionPublishException catch (failure) {
      if (!context.mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text(failure.message)));
    }
  }
}

enum _SessionAction { rename, goal, close, reopen, stop }

/// A bottom sheet with one text field and a Save that publishes.
Future<void> _showTextSheet(
  BuildContext context, {
  required String title,
  required String initial,
  required String hint,
  required bool multiline,
  required int maxBytes,
  required Future<void> Function(String value) onSave,
}) => showBuzzModalBottomSheet<void>(
  context: context,
  title: title,
  isScrollControlled: true,
  builder: (sheetContext) => Padding(
    padding: EdgeInsets.only(
      bottom: MediaQuery.viewInsetsOf(sheetContext).bottom,
    ),
    child: _TextSheet(
      initial: initial,
      hint: hint,
      multiline: multiline,
      maxBytes: maxBytes,
      onSave: onSave,
    ),
  ),
);

class _TextSheet extends HookWidget {
  final String initial;
  final String hint;
  final bool multiline;
  final int maxBytes;
  final Future<void> Function(String value) onSave;

  const _TextSheet({
    required this.initial,
    required this.hint,
    required this.multiline,
    required this.maxBytes,
    required this.onSave,
  });

  @override
  Widget build(BuildContext context) {
    final controller = useTextEditingController(text: initial);
    final text = useState(initial);
    final saving = useState(false);
    final error = useState<String?>(null);

    useEffect(() {
      void listen() => text.value = controller.text;
      controller.addListener(listen);
      return () => controller.removeListener(listen);
    }, [controller]);

    final bytes = utf8ByteLength(text.value.trim());
    final canSave =
        !saving.value &&
        text.value.trim().isNotEmpty &&
        bytes <= maxBytes &&
        text.value.trim() != initial.trim();

    Future<void> save() async {
      saving.value = true;
      error.value = null;
      try {
        await onSave(controller.text);
        if (context.mounted) Navigator.of(context).pop();
      } on CodingSessionPublishException catch (failure) {
        error.value = failure.message;
      } finally {
        saving.value = false;
      }
    }

    return Padding(
      padding: const EdgeInsets.fromLTRB(
        Grid.gutter,
        Grid.xxs,
        Grid.gutter,
        Grid.gutter,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          TextField(
            key: const ValueKey('coding-session-sheet-field'),
            controller: controller,
            autofocus: true,
            enabled: !saving.value,
            minLines: 1,
            maxLines: multiline ? 6 : 1,
            decoration: InputDecoration(
              hintText: hint,
              border: OutlineInputBorder(
                borderRadius: BorderRadius.circular(Radii.lg),
              ),
              helperText: bytes > maxBytes
                  ? 'Too long: $bytes of $maxBytes bytes'
                  : null,
              helperStyle: TextStyle(color: context.colors.error),
            ),
          ),
          if (error.value case final message?)
            Padding(
              padding: const EdgeInsets.only(top: Grid.xxs),
              child: Text(
                message,
                key: const ValueKey('coding-session-sheet-error'),
                style: context.textTheme.bodySmall?.copyWith(
                  color: context.colors.error,
                ),
              ),
            ),
          const SizedBox(height: Grid.twelve),
          FilledButton(
            key: const ValueKey('coding-session-sheet-save'),
            onPressed: canSave ? save : null,
            child: Text(saving.value ? 'Saving…' : 'Save'),
          ),
        ],
      ),
    );
  }
}
