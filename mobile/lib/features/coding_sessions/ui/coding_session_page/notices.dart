part of '../coding_session_page.dart';

/// A single disclosure line above the transcript.
class _SessionNotice extends StatelessWidget {
  final IconData icon;
  final String text;
  final bool emphasise;

  const _SessionNotice({
    super.key,
    required this.icon,
    required this.text,
    this.emphasise = false,
  });

  @override
  Widget build(BuildContext context) {
    final color = emphasise
        ? context.colors.error
        : context.colors.onSurfaceVariant;
    return Container(
      margin: const EdgeInsets.only(bottom: Grid.xxs),
      padding: const EdgeInsets.symmetric(
        horizontal: Grid.xxs,
        vertical: Grid.xxs,
      ),
      decoration: BoxDecoration(
        color: emphasise
            ? context.colors.errorContainer
            : context.colors.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(Radii.md),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: Grid.quarter),
            child: Icon(icon, size: 14, color: color),
          ),
          const SizedBox(width: Grid.xxs),
          Expanded(
            child: Text(
              text,
              style: context.textTheme.bodySmall?.copyWith(color: color),
            ),
          ),
        ],
      ),
    );
  }
}
