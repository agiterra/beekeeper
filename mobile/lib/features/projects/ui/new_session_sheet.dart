import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/theme/theme.dart';
import '../../../shared/utils/string_utils.dart';
import '../../../shared/widgets/modal_presentation.dart';
import '../../channels/channel.dart';
import '../../coding_sessions/domain/coding_sessions_domain.dart';
import '../../coding_sessions/state/coding_sessions_state.dart';
import '../../coding_sessions/ui/observer_contract.dart';
import '../../profile/user_cache_provider.dart';
import '../domain/project_models.dart';
import '../state/projects_provider.dart';
import 'project_tree.dart';

/// Why a project cannot take a new session from this device.
const projectNoSessionsChannelLabel =
    'This project has no sessions channel yet. Start its first session from '
    'the desktop, which creates the channel; after that, new sessions can '
    'start from here.';

/// Open the "New coding session" sheet for [project], or say why it cannot
/// open: a session lives in the project's sessions channel, and only the
/// desktop can create that channel.
Future<void> showNewCodingSessionSheet(
  BuildContext context,
  WidgetRef ref, {
  required Project project,
  required List<Channel> myChannels,
}) async {
  final read = ref.read(projectsProvider);
  final channels = projectChannelsFor(
    project: project,
    myChannels: myChannels,
    referenced: read.referencedChannels,
  );
  // Whether there is a channel at all is decided here; which one, in the
  // sheet's build, where the channels' session reads can be watched.
  if (pickProjectSessionsChannel(project, channels) == null) {
    ScaffoldMessenger.maybeOf(context)?.showSnackBar(
      const SnackBar(content: Text(projectNoSessionsChannelLabel)),
    );
    return;
  }
  await showBeekeeperModalBottomSheet<void>(
    context: context,
    title: 'New coding session',
    isScrollControlled: true,
    builder: (_) => NewCodingSessionSheet(project: project, channels: channels),
  );
}

/// Newest session activity per transport channel, unix seconds — what
/// [pickProjectSessionsChannel] orders several transports by.
Map<String, int> projectSessionActivityByChannel(
  Iterable<ProjectChannel> channels,
  Iterable<CodingSessionUmbrella> Function(String channelId) sessionsOf,
) {
  final activity = <String, int>{};
  for (final channel in channels) {
    if (!channel.isTransport) continue;
    for (final session in sessionsOf(channel.id)) {
      final at = session.lastActivityAt;
      if (at > (activity[channel.id] ?? -1)) activity[channel.id] = at;
    }
  }
  return activity;
}

/// One offer a member may start a session under: a provider instance and
/// the catalog — hence the signer — it came from.
class _Offer {
  final CodingSessionProviderCatalog catalog;
  final CodingSessionProviderOffer offer;

  const _Offer(this.catalog, this.offer);

  String get key => '${catalog.signerPubkey} ${offer.providerInstanceRef}';
}

/// The form behind the project's "+": a title, a first prompt, and which
/// provider and model run it — read from the 44222 catalogs advertised in
/// the project's sessions channel, never assumed.
///
/// Sending publishes what the desktop's own launch publishes (genesis, name,
/// create) and nothing more: the working directory is the one that
/// provider's computer recorded for the project, and if it has none the
/// provider's refusal is what shows, under the project, in its words.
class NewCodingSessionSheet extends HookConsumerWidget {
  final Project project;

  /// The project's channels; the sessions channel is picked in [build] so
  /// that, among several transports, the one with the newest session
  /// activity — the one the provider is advertising in — wins.
  final List<ProjectChannel> channels;

  const NewCodingSessionSheet({
    super.key,
    required this.project,
    required this.channels,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final binding = ref.watch(codingSessionObserverBindingProvider);
    final channel = pickProjectSessionsChannel(
      project,
      channels,
      sessionActivityByChannel: projectSessionActivityByChannel(
        channels,
        (id) => binding.watch(ref, id).sessions,
      ),
    );
    if (channel == null) {
      // The opener refused already; this is the same sentence for a channel
      // list that changed between the tap and the frame.
      return const Padding(
        padding: EdgeInsets.all(Grid.md),
        child: Text(projectNoSessionsChannelLabel),
      );
    }
    return _NewCodingSessionSheetBody(project: project, channel: channel);
  }
}

class _NewCodingSessionSheetBody extends HookConsumerWidget {
  final Project project;
  final ProjectChannel channel;

  const _NewCodingSessionSheetBody({
    required this.project,
    required this.channel,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final catalogs = ref.watch(
      codingSessionProviderCatalogsProvider(channel.id),
    );
    return Padding(
      padding: const EdgeInsets.fromLTRB(Grid.gutter, 0, Grid.gutter, Grid.md),
      child: catalogs.when(
        loading: () => const Padding(
          padding: EdgeInsets.all(Grid.md),
          child: Center(child: CircularProgressIndicator()),
        ),
        error: (error, _) => _Note(
          key: const ValueKey('new-session-read-error'),
          text: 'The providers for ${_channelName()} could not be read: $error',
          emphasise: true,
          onRetry: () =>
              ref.invalidate(codingSessionProviderCatalogsProvider(channel.id)),
        ),
        data: (read) {
          final offers = [
            for (final catalog in read.catalogs)
              for (final offer in catalog.offersForProject(project.address))
                _Offer(catalog, offer),
          ];
          if (offers.isEmpty) {
            return _Note(
              key: const ValueKey('new-session-empty'),
              text:
                  'No coding-session provider has advertised in '
                  '${_channelName()}'
                  '${read.rejected > 0 ? ' (${read.rejected} unreadable '
                            '${read.rejected == 1 ? 'catalog' : 'catalogs'} '
                            'ignored)' : ''}. '
                  'Open the desktop app that runs this project\'s provider — '
                  'its provider joins the channel and advertises itself — '
                  'then try again.',
              onRetry: () => ref.invalidate(
                codingSessionProviderCatalogsProvider(channel.id),
              ),
            );
          }
          return _NewSessionForm(
            project: project,
            channel: channel,
            offers: offers,
            rejected: read.rejected,
          );
        },
      ),
    );
  }

  String _channelName() => channel.name.isEmpty ? channel.id : channel.name;
}

class _NewSessionForm extends HookConsumerWidget {
  final Project project;
  final ProjectChannel channel;
  final List<_Offer> offers;
  final int rejected;

  const _NewSessionForm({
    required this.project,
    required this.channel,
    required this.offers,
    required this.rejected,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final binding = ref.watch(codingSessionObserverBindingProvider);
    final profiles = ref.watch(userCacheProvider);
    final snapshot = binding.watch(ref, channel.id);
    // A signer that already runs a verified session here is the one the
    // person most likely means; a fresh channel has no such signer and the
    // first offer stands.
    final verified = {
      for (final execution in snapshot.executions)
        if (execution.authority.verified) execution.authority.pubkey,
    };
    final initial = offers.firstWhere(
      (entry) => verified.contains(entry.catalog.signerPubkey),
      orElse: () => offers.first,
    );
    final selected = useState(initial);
    final model = useState(initial.offer.defaultModel);
    final title = useTextEditingController();
    final prompt = useTextEditingController();
    final sending = useState(false);
    final error = useState<String?>(null);

    String signerLabel(String pubkey) =>
        profiles[pubkey.toLowerCase()]?.label ?? shortPubkey(pubkey);
    String offerLabel(_Offer entry) =>
        '${entry.offer.runtime} · ${entry.offer.providerInstanceRef} — '
        '${signerLabel(entry.catalog.signerPubkey)}';

    Future<void> start() async {
      sending.value = true;
      error.value = null;
      try {
        final chosen = selected.value;
        await binding
            .commands(ref, channel.id)
            .createSession(
              projectRef: project.address,
              repoRef: null,
              providerInstanceRef: chosen.offer.providerInstanceRef,
              providerAuthorityPubkey: chosen.catalog.signerPubkey,
              model: model.value,
              title: title.text,
              initialTurn: prompt.text,
            );
        if (!context.mounted) return;
        Navigator.of(context).pop();
        ScaffoldMessenger.maybeOf(context)?.showSnackBar(
          SnackBar(
            content: Text(
              'Asked ${signerLabel(chosen.catalog.signerPubkey)} to start the '
              'session. It appears under ${project.name} when the provider '
              'answers.',
            ),
          ),
        );
      } on CodingSessionPublishException catch (refusal) {
        error.value = refusal.message;
      } finally {
        sending.value = false;
      }
    }

    final colors = context.colors;
    return SingleChildScrollView(
      padding: EdgeInsets.only(bottom: MediaQuery.viewInsetsOf(context).bottom),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          TextField(
            key: const ValueKey('new-session-title'),
            controller: title,
            enabled: !sending.value,
            textInputAction: TextInputAction.next,
            decoration: const InputDecoration(
              labelText: 'Title (optional)',
              isDense: true,
            ),
          ),
          const SizedBox(height: Grid.xs),
          TextField(
            key: const ValueKey('new-session-prompt'),
            controller: prompt,
            enabled: !sending.value,
            minLines: 2,
            maxLines: 6,
            decoration: const InputDecoration(
              labelText: 'First prompt (optional)',
              hintText: 'What should the session do first?',
              alignLabelWithHint: true,
            ),
          ),
          const SizedBox(height: Grid.xs),
          DropdownButtonFormField<String>(
            key: const ValueKey('new-session-provider'),
            initialValue: selected.value.key,
            isExpanded: true,
            decoration: const InputDecoration(
              labelText: 'Provider',
              isDense: true,
            ),
            items: [
              for (final entry in offers)
                DropdownMenuItem(
                  key: ValueKey('new-session-provider-${entry.key}'),
                  value: entry.key,
                  child: Text(
                    offerLabel(entry),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
            ],
            onChanged: sending.value
                ? null
                : (key) {
                    final next = offers.firstWhere((e) => e.key == key);
                    selected.value = next;
                    model.value = next.offer.defaultModel;
                  },
          ),
          const SizedBox(height: Grid.xs),
          // Keyed by offer so a provider change rebuilds the picker with
          // that offer's own list instead of a stale selection.
          KeyedSubtree(
            key: ValueKey('new-session-model-for-${selected.value.key}'),
            child: DropdownButtonFormField<String>(
              key: const ValueKey('new-session-model'),
              initialValue: model.value,
              isExpanded: true,
              decoration: const InputDecoration(
                labelText: 'Model',
                isDense: true,
              ),
              items: [
                for (final id in selected.value.offer.allowedModels)
                  DropdownMenuItem(
                    key: ValueKey('new-session-model-$id'),
                    value: id,
                    child: Text(
                      id == selected.value.offer.defaultModel
                          ? '$id (provider default)'
                          : id,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
              ],
              onChanged: sending.value
                  ? null
                  : (id) {
                      if (id != null) model.value = id;
                    },
            ),
          ),
          const SizedBox(height: Grid.xs),
          Text(
            'Runs on the computer that published this provider, in the '
            'working directory it has recorded for ${project.name}. If it '
            'has none, the provider refuses and the refusal shows under the '
            'project.'
            '${rejected > 0 ? ' $rejected unreadable '
                      '${rejected == 1 ? 'catalog was' : 'catalogs were'} '
                      'ignored.' : ''}',
            style: context.textTheme.bodySmall?.copyWith(
              color: colors.onSurfaceVariant,
            ),
          ),
          if (error.value case final message?)
            Padding(
              padding: const EdgeInsets.only(top: Grid.xs),
              child: Text(
                message,
                key: const ValueKey('new-session-error'),
                style: context.textTheme.bodySmall?.copyWith(
                  color: colors.error,
                ),
              ),
            ),
          const SizedBox(height: Grid.sm),
          FilledButton(
            key: const ValueKey('new-session-start'),
            onPressed: sending.value ? null : start,
            child: Text(sending.value ? 'Starting…' : 'Start session'),
          ),
        ],
      ),
    );
  }
}

class _Note extends StatelessWidget {
  final String text;
  final bool emphasise;
  final VoidCallback onRetry;

  const _Note({
    super.key,
    required this.text,
    required this.onRetry,
    this.emphasise = false,
  });

  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      Text(
        text,
        style: context.textTheme.bodyMedium?.copyWith(
          color: emphasise
              ? context.colors.error
              : context.colors.onSurfaceVariant,
        ),
      ),
      const SizedBox(height: Grid.xs),
      Align(
        alignment: Alignment.centerRight,
        child: TextButton(
          key: const ValueKey('new-session-retry'),
          onPressed: onRetry,
          child: const Text('Try again'),
        ),
      ),
    ],
  );
}
