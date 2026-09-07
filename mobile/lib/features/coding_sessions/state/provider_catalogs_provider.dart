import 'package:flutter/foundation.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/relay/relay.dart';
import '../domain/coding_sessions_domain.dart';

/// What one channel's provider catalogs say, read once.
///
/// [catalogs] is the newest accepted catalog per signer; [rejected] counts
/// the 44222s that failed the decoder — disclosed, because a channel with
/// three unreadable catalogs is a different thing from a channel with none.
@immutable
class CodingSessionProviderCatalogs {
  final String channelId;
  final List<CodingSessionProviderCatalog> catalogs;
  final int rejected;

  const CodingSessionProviderCatalogs({
    required this.channelId,
    required this.catalogs,
    required this.rejected,
  });

  bool get isEmpty => catalogs.isEmpty;
}

/// The signature verifier the catalog read uses; overridable in tests.
final codingSessionCatalogVerifierProvider =
    Provider<CodingSessionSignatureVerifier?>(
      (ref) => const NostrPackageSignatureVerifier(),
    );

/// The provider catalogs advertised in one channel — a one-shot read when
/// a create sheet opens. `ref.invalidate` re-reads.
///
/// Not a live subscription on purpose: a catalog changes when a provider
/// restarts, and the sheet is open for seconds. Every read is signature
/// checked; authority is channel membership, which the relay enforced
/// before storing the event.
final codingSessionProviderCatalogsProvider = FutureProvider.autoDispose
    .family<CodingSessionProviderCatalogs, String>((ref, channelId) async {
      final session = ref.read(relaySessionProvider.notifier);
      final verifier = ref.read(codingSessionCatalogVerifierProvider);
      final events = await session.fetchHistory(
        NostrFilters.codingSessionProviderCatalogs(channelId),
      );
      final accepted = <CodingSessionProviderCatalog>[];
      var rejected = 0;
      for (final event in events) {
        final decoded = decodeCodingSessionProviderCatalog(
          event,
          verifier: verifier,
        );
        final catalog = decoded.value;
        if (catalog == null) {
          if (decoded.isRejected) rejected++;
          continue;
        }
        if (catalog.ref.channelId != channelId) {
          rejected++;
          continue;
        }
        accepted.add(catalog);
      }
      return CodingSessionProviderCatalogs(
        channelId: channelId,
        catalogs: newestCodingSessionProviderCatalogs(accepted),
        rejected: rejected,
      );
    });
