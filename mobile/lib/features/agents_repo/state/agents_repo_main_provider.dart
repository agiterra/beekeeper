import 'dart:async';

import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/relay/relay.dart';
import '../data/agents_repo_http_client.dart';
import 'agents_repo_source_provider.dart';

/// The HTTP reader for the active community, built from its config.
final agentsRepoHttpClientProvider = Provider<AgentsRepoHttpClient>((ref) {
  final config = ref.watch(relayConfigProvider);
  return AgentsRepoHttpClient(baseUrl: config.baseUrl, nsec: config.nsec);
});

/// The listing of `main`, re-read when the repository's relay-signed ref
/// state (kind:30618) moves — a push landed — and on pull-to-refresh.
final agentsRepoListingProvider =
    AsyncNotifierProvider.family<
      AgentsRepoListingNotifier,
      AgentsRepoListing,
      String
    >(AgentsRepoListingNotifier.new);

class AgentsRepoListingNotifier extends AsyncNotifier<AgentsRepoListing> {
  AgentsRepoListingNotifier(this.address);

  final String address;
  void Function()? _unsubscribe;

  @override
  Future<AgentsRepoListing> build() async {
    ref.onDispose(() {
      _unsubscribe?.call();
      _unsubscribe = null;
    });
    final source = await ref.watch(agentsRepoSourceProvider(address).future);
    if (source == null) {
      throw StateError(
        'This project has no agents repository yet (no kind:30624 source).',
      );
    }
    if (!source.isAgentsRepo) {
      throw StateError(
        'This project\'s source is a pack-layout repository (${source.repo}'
        '${source.path == '.' ? '' : ' at ${source.path}'}'
        '${source.sha == null ? '' : ', pinned to ${source.sha!.substring(0, 8)}'}); '
        'the Files page reads an agents repository at a repository root '
        'following a branch.',
      );
    }
    _watchRefState(source.id);
    final client = ref.read(agentsRepoHttpClientProvider);
    return client.listMain(source.owner, source.id);
  }

  /// Re-read `main` — a push landed, or the person pulled to refresh.
  Future<void> refresh() async {
    state = await AsyncValue.guard(() async {
      final source = await ref.read(agentsRepoSourceProvider(address).future);
      if (source == null || !source.isAgentsRepo) {
        throw StateError('This project has no agents repository yet.');
      }
      final client = ref.read(agentsRepoHttpClientProvider);
      return client.listMain(source.owner, source.id);
    });
  }

  void _watchRefState(String repoId) {
    if (_unsubscribe != null) return;
    final session = ref.read(relaySessionProvider.notifier);
    unawaited(() async {
      try {
        final unsubscribe = await session.subscribe(
          NostrFilters.repoState(repoId),
          (_) {
            // Any new ref state: main may have moved. Re-read both.
            unawaited(refresh());
            ref.invalidate(agentsRepoFileProvider);
          },
        );
        _unsubscribe = unsubscribe;
      } catch (_) {
        // Pull-to-refresh is the fallback; a failed watch is not a read
        // failure.
      }
    }());
  }
}

/// One file at `main`'s tip, keyed by `<address>\u0000<path>`.
final agentsRepoFileProvider =
    FutureProvider.family<AgentsRepoFile, ({String address, String path})>((
      ref,
      key,
    ) async {
      final source = await ref.watch(
        agentsRepoSourceProvider(key.address).future,
      );
      if (source == null || !source.isAgentsRepo) {
        throw StateError('This project has no agents repository yet.');
      }
      // Re-read when the listing does (a push moved main).
      ref.watch(agentsRepoListingProvider(key.address));
      final client = ref.read(agentsRepoHttpClientProvider);
      return client.readMain(source.owner, source.id, key.path);
    });
