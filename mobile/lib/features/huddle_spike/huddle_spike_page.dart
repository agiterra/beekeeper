import 'package:buzz/shared/huddle/huddle.dart';
import 'package:buzz/shared/relay/relay.dart';
import 'package:buzz/shared/theme/grid.dart';
import 'package:buzz/shared/theme/theme_extensions.dart';
import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

/// THROWAWAY — Phase 0 Huddle transport spike. Delete before the UI lands.
///
/// This exists to answer one question on physical hardware: can this app
/// complete the v2 handshake against *our* relay and exchange intelligible
/// Opus with a real Desktop participant? It deliberately has no channel
/// integration and no lifecycle publishing — Desktop starts the Huddle and
/// this pastes into it, because the relay's `ensure_membership` gate resolves
/// an ephemeral channel's parent from a creator-signed kind:48100 rather than
/// trusting a client-supplied UUID, so a fabricated room would be refused.
///
/// Reaching this screen requires no route registration: push it directly.
///
/// ```dart
/// Navigator.of(context).push(
///   MaterialPageRoute<void>(builder: (_) => const HuddleSpikePage()),
/// );
/// ```
class HuddleSpikePage extends HookConsumerWidget {
  const HuddleSpikePage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final session = ref.watch(huddleSessionProvider);
    final config = ref.watch(relayConfigProvider);
    final myPubkey = ref.watch(myPubkeyProvider);

    final ephemeralController = useTextEditingController();
    final parentController = useTextEditingController();
    final launchError = useState<String?>(null);

    Future<void> joinHuddle() async {
      launchError.value = null;
      final nsec = config.nsec;
      if (nsec == null || nsec.isEmpty) {
        launchError.value = 'No nsec in relay config — sign in first.';
        return;
      }
      final HuddleConnectionParameters parameters;
      try {
        // Constructed rather than assumed: this validates the relay scheme and
        // both UUIDs before any socket opens, so a typo fails here with a
        // readable message instead of as an opaque relay rejection.
        parameters = HuddleConnectionParameters(
          relayWebSocketUrl: config.wsUrl,
          nsec: nsec,
          parentChannelId: parentController.text.trim(),
          ephemeralChannelId: ephemeralController.text.trim(),
        );
      } on ArgumentError catch (error) {
        launchError.value = '${error.name}: ${error.message}';
        return;
      }
      debugPrint('huddle-spike: joining ${parameters.audioWebSocketUri}');
      await ref
          .read(huddleSessionProvider.notifier)
          .join(parameters, currentPubkey: myPubkey);
    }

    return Scaffold(
      appBar: AppBar(title: const Text('Huddle transport spike')),
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.all(Grid.xs),
          children: [
            _Field(
              controller: ephemeralController,
              label: 'Ephemeral channel UUID',
              hint: 'from the relay log when Desktop starts the Huddle',
            ),
            const SizedBox(height: Grid.twelve),
            _Field(
              controller: parentController,
              label: 'Parent channel UUID',
              hint: 'the channel the Huddle was started in',
            ),
            const SizedBox(height: Grid.xs),
            Text('relay: ${config.wsUrl}', style: context.textTheme.bodySmall),
            const SizedBox(height: Grid.xs),
            Wrap(
              spacing: Grid.xxs,
              runSpacing: Grid.xxs,
              children: [
                FilledButton(
                  onPressed: session.isInSession ? null : joinHuddle,
                  child: const Text('Join'),
                ),
                OutlinedButton(
                  onPressed: session.isInSession
                      ? () => ref.read(huddleSessionProvider.notifier).leave()
                      : null,
                  child: const Text('Leave'),
                ),
                OutlinedButton(
                  onPressed: session.phase == HuddleSessionPhase.connected
                      ? () => ref
                            .read(huddleSessionProvider.notifier)
                            .setMuted(!session.isMuted)
                      : null,
                  child: Text(session.isMuted ? 'Unmute' : 'Mute'),
                ),
                OutlinedButton(
                  onPressed: session.phase == HuddleSessionPhase.connected
                      ? () => ref
                            .read(huddleSessionProvider.notifier)
                            .setSpeakerEnabled(!session.isSpeakerEnabled)
                      : null,
                  child: Text(
                    session.isSpeakerEnabled ? 'Earpiece' : 'Speaker',
                  ),
                ),
                if (session.microphonePermissionRequired)
                  OutlinedButton(
                    onPressed: () => ref
                        .read(huddleSessionProvider.notifier)
                        .openMicrophoneSettings(),
                    child: const Text('Open settings'),
                  ),
              ],
            ),
            const Divider(height: Grid.lg),
            // Each row below maps to a Phase 0 success criterion, so the go /
            // no-go call can be read straight off the device.
            _Stat('phase (S1)', session.phase.name),
            _Stat('admitted (S1)', '${session.wasAdmitted}'),
            _Stat('sent frames (S2)', '${session.sentFrameCount}'),
            _Stat('received frames (S3)', '${session.receivedFrameCount}'),
            _Stat('participants', '${session.participantCount}'),
            _Stat('speaking', session.activeSpeakerPubkeys.length.toString()),
            _Stat('reconnects (S5)', '${session.reconnectAttempt}'),
            if (launchError.value != null)
              _Stat('launch error', launchError.value!, isProblem: true),
            if (session.issue != null)
              _Stat('issue', session.issue!, isProblem: true),
            if (session.error != null)
              _Stat('error', session.error!, isProblem: true),
            const SizedBox(height: Grid.xs),
            for (final pubkey in session.participantPubkeys)
              _Stat(
                pubkey.length > 12 ? '${pubkey.substring(0, 12)}…' : pubkey,
                session.speakerLevels[pubkey]?.toStringAsFixed(2) ?? '—',
              ),
          ],
        ),
      ),
    );
  }
}

class _Field extends StatelessWidget {
  const _Field({
    required this.controller,
    required this.label,
    required this.hint,
  });

  final TextEditingController controller;
  final String label;
  final String hint;

  @override
  Widget build(BuildContext context) {
    return TextField(
      controller: controller,
      autocorrect: false,
      decoration: InputDecoration(
        labelText: label,
        hintText: hint,
        border: const OutlineInputBorder(),
      ),
    );
  }
}

class _Stat extends StatelessWidget {
  const _Stat(this.label, this.value, {this.isProblem = false});

  final String label;
  final String value;
  final bool isProblem;

  @override
  Widget build(BuildContext context) {
    final color = isProblem ? context.colors.error : context.colors.onSurface;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: Grid.quarter),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 160,
            child: Text(label, style: context.textTheme.bodySmall),
          ),
          Expanded(
            child: Text(
              value,
              style: context.textTheme.bodyMedium?.copyWith(color: color),
            ),
          ),
        ],
      ),
    );
  }
}
