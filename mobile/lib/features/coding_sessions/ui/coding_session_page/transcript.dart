part of '../coding_session_page.dart';

/// One `(signer, target)` stream of rows, grouped into turns.
///
/// Blocks are never merged: two providers number their own `eventSeq`
/// independently, so interleaving their rows would invent an order neither of
/// them signed (D9).
class _TranscriptBlockView extends StatelessWidget {
  final CodingSessionTranscriptBlock block;

  /// Whether to name the execution above the rows.
  final bool showLabel;

  const _TranscriptBlockView({
    super.key,
    required this.block,
    required this.showLabel,
  });

  @override
  Widget build(BuildContext context) {
    final rows = <Widget>[];
    for (var index = 0; index < block.turns.length; index++) {
      final turn = block.turns[index];
      if (index > 0) {
        rows.add(_TurnSeparator(turnId: turn.turnId));
      }
      for (final item in turn.items) {
        rows.add(_TranscriptRow(item: item));
      }
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (showLabel)
          Padding(
            padding: const EdgeInsets.only(bottom: Grid.xxs),
            child: Row(
              children: [
                Icon(
                  LucideIcons.cpu,
                  size: 14,
                  color: context.colors.onSurfaceVariant,
                ),
                const SizedBox(width: Grid.xxs),
                Expanded(
                  child: Text(
                    '${block.label} · ${shortPubkey(block.signerPubkey)}',
                    style: context.textTheme.labelSmall?.copyWith(
                      color: context.colors.onSurfaceVariant,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ...rows,
      ],
    );
  }
}

/// The boundary between two turns in one stream.
class _TurnSeparator extends StatelessWidget {
  final String? turnId;

  const _TurnSeparator({required this.turnId});

  @override
  Widget build(BuildContext context) {
    final id = turnId;
    return Padding(
      key: const ValueKey('coding-session-turn-separator'),
      padding: const EdgeInsets.symmetric(vertical: Grid.twelve),
      child: Row(
        children: [
          Expanded(child: Divider(color: context.colors.outlineVariant)),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: Grid.xxs),
            child: Text(
              id == null ? 'Outside a turn' : 'Turn ${shortPubkey(id)}',
              style: context.textTheme.labelSmall?.copyWith(
                color: context.colors.onSurfaceVariant,
              ),
            ),
          ),
          Expanded(child: Divider(color: context.colors.outlineVariant)),
        ],
      ),
    );
  }
}
