import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/theme/theme.dart';
import '../../../shared/utils/string_utils.dart';
import '../domain/coding_sessions_domain.dart';

/// The muted "Auto-named" label beside a session name a provider generated.
///
/// A model's words are never shown as if a person chose them: when the shared
/// display-name rule settled on a provider-signed 44252, this label says so,
/// and one tap opens the detail — which machine's provider named it, and with
/// which model. A person's name, and the fallback, render nothing here.
///
/// T3 Code shows a generated thread title exactly like a typed one; Beekeeper
/// differs on purpose, because the title is signed by a different author than
/// the person whose session it names (SESSION_PARITY_SPEC_AUTOTITLE.md).
class CodingSessionTitleOriginLabel extends HookConsumerWidget {
  /// The session whose name this labels.
  final CodingSessionUmbrella session;

  /// Prefix for the widget keys, so the header and the list card stay
  /// distinguishable in one tree.
  final String keyPrefix;

  const CodingSessionTitleOriginLabel({
    super.key,
    required this.session,
    this.keyPrefix = 'coding-session-title-origin',
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final expanded = useState(false);
    final detail = codingSessionTitleOriginDetail(session);
    if (detail == null) return const SizedBox.shrink();
    final colors = context.colors;
    final muted = context.textTheme.labelSmall?.copyWith(
      color: colors.onSurfaceVariant,
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Semantics(
          button: true,
          label: 'Auto-named',
          hint: detail,
          excludeSemantics: true,
          child: Material(
            color: colors.surfaceContainerHighest,
            borderRadius: BorderRadius.circular(Radii.full),
            child: InkWell(
              key: ValueKey(keyPrefix),
              borderRadius: BorderRadius.circular(Radii.full),
              onTap: () => expanded.value = !expanded.value,
              child: Padding(
                padding: const EdgeInsets.symmetric(
                  horizontal: Grid.xxs,
                  vertical: Grid.quarter,
                ),
                child: Text('Auto-named', style: muted),
              ),
            ),
          ),
        ),
        if (expanded.value)
          Padding(
            padding: const EdgeInsets.only(top: Grid.quarter),
            child: Text(
              detail,
              key: ValueKey('$keyPrefix-detail'),
              style: context.textTheme.bodySmall?.copyWith(
                color: colors.onSurfaceVariant,
              ),
            ),
          ),
      ],
    );
  }
}

/// "Named automatically from the first message by `provider` · `model`", or
/// `null` when the session's name is not a generated one.
///
/// The provider is named the way this app already names the execution that
/// signed the title — the provider's own signed label, else its runtime, else
/// its driver — followed by its signer, because the signer is the only part
/// of that a stranger cannot claim.
String? codingSessionTitleOriginDetail(CodingSessionUmbrella session) {
  final resolved = session.resolvedName;
  if (!resolved.isGenerated) return null;
  final signer = resolved.signerPubkey ?? '';
  CodingSessionExecution? signing;
  for (final execution in session.executions) {
    if (execution.targetKey == resolved.targetKey) {
      signing = execution;
      break;
    }
  }
  final metadata = signing?.metadata;
  final provider =
      [
        metadata?.provider,
        metadata?.runtime,
        signing?.target.driver,
      ].firstWhere(
        (label) => label != null && label.trim().isNotEmpty,
        orElse: () => null,
      );
  final who = provider == null
      ? 'provider ${shortPubkey(signer)}'
      : '$provider (${shortPubkey(signer)})';
  return 'Named automatically from the first message by $who · '
      '${resolved.model}';
}
