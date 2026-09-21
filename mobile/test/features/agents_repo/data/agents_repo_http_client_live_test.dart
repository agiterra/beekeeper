import 'dart:io';

import 'package:buzz/features/agents_repo/data/agents_repo_http_client.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nostr/nostr.dart' as nostr;

/// The mobile reader against a live relay — the NIP-98 binding to the
/// repository root is the one seam no other suite covers. Skipped unless
/// `BUZZ_AGENTS_REPO_LIVE` names `<relay http>|<hex key>|<owner>|<repo id>|<path>`:
///
/// ```text
/// BUZZ_AGENTS_REPO_LIVE='http://localhost:3010|<hex>|<owner>|<id>|plans/rpg.md' \
///   flutter test test/features/agents_repo/data/agents_repo_http_client_live_test.dart
/// ```
void main() {
  final spec = Platform.environment['BUZZ_AGENTS_REPO_LIVE'];
  test(
    'tree and raw answer under the repo-root NIP-98 token',
    () async {
      final parts = spec!.split('|');
      final client = AgentsRepoHttpClient(
        baseUrl: parts[0],
        nsec: nostr.Nip19.encode(
          prefix: nostr.Nip19Prefix.nsec,
          data: parts[1],
        ),
      );
      final listing = await client.listMain(parts[2], parts[3]);
      expect(listing.commit.length, 40);
      expect(listing.entries, isNotEmpty);
      final file = await client.readMain(parts[2], parts[3], parts[4]);
      expect(file.state, 'on-main');
      expect(file.blob?.length, 40);
      expect(file.commit, listing.commit);
      expect(file.text, isNotNull);
      final missing = await client.readMain(
        parts[2],
        parts[3],
        'plans/nope.md',
      );
      expect(missing.state, 'not-on-main');
      // ignore: avoid_print
      print(
        'main ${listing.commit} · ${listing.entries.length} entries · '
        '${parts[4]} blob ${file.blob}',
      );
    },
    skip: spec == null ? 'BUZZ_AGENTS_REPO_LIVE not set' : false,
  );
}
