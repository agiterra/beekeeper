part of '../agents_repo_page.dart';

class _EditResult {
  final String text;
  final String? message;
  const _EditResult(this.text, this.message);
}

/// The whole text of one file, and a one-line note; Save publishes a draft.
class _EditSheet extends HookWidget {
  final String initial;
  const _EditSheet({required this.initial});

  @override
  Widget build(BuildContext context) {
    final controller = useTextEditingController(text: initial);
    final note = useTextEditingController();
    final text = useState(initial);
    useEffect(() {
      void listen() => text.value = controller.text;
      controller.addListener(listen);
      return () => controller.removeListener(listen);
    }, [controller]);
    final error = draftTextError(text.value);
    final changed = text.value != initial;

    void submit() {
      if (error != null || !changed) return;
      final message = note.text.trim();
      Navigator.of(
        context,
      ).pop(_EditResult(controller.text, message.isEmpty ? null : message));
    }

    final height = MediaQuery.sizeOf(context).height;
    return Padding(
      padding: EdgeInsets.fromLTRB(
        Grid.xs,
        0,
        Grid.xs,
        MediaQuery.viewInsetsOf(context).bottom + Grid.xs,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ConstrainedBox(
            constraints: BoxConstraints(maxHeight: height * 0.45),
            child: TextField(
              key: const ValueKey('agents-repo-edit-field'),
              controller: controller,
              autofocus: true,
              maxLines: null,
              minLines: 8,
              keyboardType: TextInputType.multiline,
              style: context.textTheme.bodySmall?.copyWith(
                fontFamily: 'monospace',
              ),
              decoration: InputDecoration(
                labelText: 'Text',
                errorText: error,
                alignLabelWithHint: true,
              ),
            ),
          ),
          const SizedBox(height: Grid.xxs),
          TextField(
            key: const ValueKey('agents-repo-edit-note'),
            controller: note,
            maxLength: maxAgentsRepoDraftMessageBytes,
            decoration: const InputDecoration(
              labelText: 'Why (one line, optional)',
              isDense: true,
              counterText: '',
            ),
          ),
          const SizedBox(height: Grid.xxs),
          FilledButton(
            key: const ValueKey('agents-repo-edit-save'),
            onPressed: error == null && changed ? submit : null,
            child: const Text('Save draft'),
          ),
          const SizedBox(height: Grid.half),
          Text(
            agentsRepoMobileCommitNote,
            style: context.textTheme.bodySmall?.copyWith(
              color: context.colors.onSurfaceVariant,
            ),
          ),
        ],
      ),
    );
  }
}
