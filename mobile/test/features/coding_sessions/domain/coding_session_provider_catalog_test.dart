import 'dart:convert';

import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/shared/relay/nostr_models.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nostr/nostr.dart' as nostr;

const _channel = '0b3a7d9c-3d0f-4c2b-9d0e-9f6a1c2b3d4e';
const _secretA =
    '5ee1c8000ab28edd64d74a7d951ac2dd559814887b1b9e1ac7c5f89e96125c12';
const _secretB =
    '7f9c2ba4e88f827d616045507605853ed73b8093f6efbc88eb1a6eacfa66ef26';

/// The provider's own canonical catalog, verbatim from
/// `crates/beekeeper-session-provider/src/catalog.rs` (`to_canonical_json`).
const rustCatalogContent =
    '{"schema":"buzz-coding-session-provider-catalog/v1","revision":3,'
    '"providers":[{"providerInstanceRef":"claude-primary",'
    '"driver":"claude-agent-acp","runtime":"claude","defaultModel":"model-b",'
    '"allowedModels":["model-b","model-a","model-c"],'
    '"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,'
    '"threadSteer":false,"context":false,"diff":false,"plan":true,'
    '"promptImage":false},"models":[{"id":"model-b","vendor":"anthropic"},'
    '{"id":"model-a","vendor":"anthropic"},'
    '{"id":"model-c","vendor":"anthropic"}]}]}';

/// The key the Rust builder writes: domain-separated channel, revision and
/// the sha256 of the exact content. Computed here independently of the
/// function under test.
String expectedKey(String channel, int revision, String content) =>
    encodeStructuredKey('coding-session-provider-catalog/v1', [
      channel,
      '$revision',
      sha256.convert(utf8.encode(content)).toString(),
    ]);

/// A genuinely signed 44222, as `build_coding_session_provider_catalog`
/// shapes it.
NostrEvent signedCatalog({
  String content = rustCatalogContent,
  String channel = _channel,
  int revision = 3,
  String secretKey = _secretA,
  int createdAt = 1700000000,
  List<List<String>>? tags,
}) {
  final signed = nostr.Event.from(
    kind: EventKind.codingSessionProviderCatalog,
    content: content,
    secretKey: secretKey,
    createdAt: createdAt,
    tags:
        tags ??
        [
          ['h', channel],
          ['cspc-v', 'cspc1-1'],
          ['cspc-revision', '$revision'],
          ['cspc-key', expectedKey(channel, revision, content)],
        ],
  );
  return NostrEvent(
    id: signed.id,
    pubkey: signed.pubkey,
    createdAt: signed.createdAt,
    kind: signed.kind,
    tags: signed.tags,
    content: signed.content,
    sig: signed.sig,
  );
}

void main() {
  group('decodeCodingSessionProviderCatalog', () {
    test('reads the provider\'s canonical catalog and verifies its key', () {
      final decoded = decodeCodingSessionProviderCatalog(
        signedCatalog(),
        verifier: const NostrPackageSignatureVerifier(),
      );
      final catalog = decoded.value;
      expect(catalog, isNotNull);
      expect(catalog!.revision, 3);
      expect(catalog.ref.channelId, _channel);
      expect(catalog.signerPubkey, nostr.Keys(_secretA).public);
      expect(catalog.projects, isNull);
      final offer = catalog.providers.single;
      expect(offer.providerInstanceRef, 'claude-primary');
      expect(offer.runtime, 'claude');
      expect(offer.defaultModel, 'model-b');
      expect(offer.allowedModels, ['model-b', 'model-a', 'model-c']);
      expect(offer.capabilities.plan, isTrue);
      expect(offer.capabilities.threadSteer, isFalse);
      expect(offer.capabilities.promptImage, isFalse);
      // No narrowing: every offer serves every project.
      expect(catalog.offersForProject('30621:x:y').single, offer);
    });

    test('the semantic key matches the Rust builder\'s', () {
      expect(
        codingSessionProviderCatalogSemanticKey(
          _channel,
          3,
          rustCatalogContent,
        ),
        expectedKey(_channel, 3, rustCatalogContent),
      );
    });

    test('a catalog published before promptImage existed still decodes', () {
      const older =
          '{"schema":"buzz-coding-session-provider-catalog/v1","revision":1,'
          '"providers":[{"providerInstanceRef":"claude-primary",'
          '"driver":"claude-agent-acp","runtime":"claude",'
          '"defaultModel":"default","allowedModels":["default"],'
          '"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,'
          '"threadSteer":false,"context":false,"diff":false,"plan":true}}]}';
      final decoded = decodeCodingSessionProviderCatalog(
        signedCatalog(content: older, revision: 1),
      );
      expect(decoded.value?.providers.single.capabilities.promptImage, false);
    });

    test('a projects narrowing filters offers per coordinate', () {
      const narrowed =
          '{"schema":"buzz-coding-session-provider-catalog/v1","revision":2,'
          '"providers":[{"providerInstanceRef":"claude-primary",'
          '"driver":"claude-agent-acp","runtime":"claude",'
          '"defaultModel":"default","allowedModels":["default"],'
          '"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,'
          '"threadSteer":false,"context":false,"diff":false,"plan":true,'
          '"promptImage":false}}],'
          '"projects":[{"projectRef":"30621:aa:demo","repoRef":null,'
          '"providers":["claude-primary"]}]}';
      final catalog = decodeCodingSessionProviderCatalog(
        signedCatalog(content: narrowed, revision: 2),
      ).value!;
      expect(catalog.offersForProject('30621:aa:demo'), hasLength(1));
      expect(catalog.offersForProject('30621:aa:other'), isEmpty);
      expect(catalog.offersForProject(null), hasLength(1));
    });

    test('rewritten content is refused: the key no longer matches', () {
      // Same JSON, one byte of whitespace: not the producer's bytes.
      final padded = rustCatalogContent.replaceFirst(
        '"revision":3',
        '"revision": 3',
      );
      final decoded = decodeCodingSessionProviderCatalog(
        signedCatalog(
          content: padded,
          tags: [
            ['h', _channel],
            ['cspc-v', 'cspc1-1'],
            ['cspc-revision', '3'],
            ['cspc-key', expectedKey(_channel, 3, rustCatalogContent)],
          ],
        ),
      );
      expect(decoded.isValid, isFalse);
      expect(decoded.isRejected, isTrue);
    });

    test('non-canonical content is refused even under its own key', () {
      // Keys out of the contract's order re-serialize differently.
      const reordered =
          '{"revision":1,"schema":"buzz-coding-session-provider-catalog/v1",'
          '"providers":[{"providerInstanceRef":"p","driver":"d",'
          '"runtime":"r","defaultModel":"m","allowedModels":["m"],'
          '"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,'
          '"threadSteer":false,"context":false,"diff":false,"plan":false}}]}';
      expect(
        decodeCodingSessionProviderCatalog(
          signedCatalog(content: reordered, revision: 1),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
      // An unsorted tail of allowedModels has another spelling: refused.
      const unsorted =
          '{"schema":"buzz-coding-session-provider-catalog/v1","revision":1,'
          '"providers":[{"providerInstanceRef":"p","driver":"d",'
          '"runtime":"r","defaultModel":"m","allowedModels":["m","z","a"],'
          '"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,'
          '"threadSteer":false,"context":false,"diff":false,"plan":false}}]}';
      expect(
        decodeCodingSessionProviderCatalog(
          signedCatalog(content: unsorted, revision: 1),
        ).reason,
        CodingSessionDecodeReason.malformedPayload,
      );
    });

    // NIP-CSPC § Per-model rows: the runtime's own name, description,
    // efforts, fast mode and rank ride after the older facts, in that order.
    String withModels(String rows) =>
        '{"schema":"buzz-coding-session-provider-catalog/v1","revision":1,'
        '"providers":[{"providerInstanceRef":"p","driver":"d",'
        '"runtime":"r","defaultModel":"m","allowedModels":["m","a"],'
        '"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,'
        '"threadSteer":false,"context":false,"diff":false,"plan":false},'
        '"models":[$rows]}]}';

    test('a model row carries the runtime\'s name, efforts and fast mode', () {
      final content = withModels(
        '{"id":"m","contextWindow":1000000,"name":"Opus 5.5",'
        '"description":"For complex work","efforts":["default","low","high"],'
        '"fastMode":true,"rank":0},{"id":"a","name":"Haiku 4.5"}',
      );
      final decoded = decodeCodingSessionProviderCatalog(
        signedCatalog(content: content, revision: 1),
      );
      expect(decoded.value, isNotNull, reason: '${decoded.reason}');
    });

    test('a runtime fact that is blank, repeated, false or misordered is '
        'refused', () {
      for (final row in [
        '{"id":"m","name":"  "}',
        '{"id":"m","name":"${'x' * 129}"}',
        '{"id":"m","description":"${'x' * 513}"}',
        '{"id":"m","efforts":[]}',
        '{"id":"m","efforts":["low","low"]}',
        '{"id":"m","efforts":["${'x' * 33}"]}',
        '{"id":"m","efforts":[${List.generate(17, (i) => '"e$i"').join(',')}]}',
        '{"id":"m","fastMode":false}',
        '{"id":"m","rank":-1}',
        '{"id":"m","rank":1.5}',
        '{"id":"m","rank":4294967296}',
        '{"id":"m","efforts":["low"],"name":"Opus"}',
        '{"id":"m","fastMode":true,"efforts":["low"]}',
      ]) {
        expect(
          decodeCodingSessionProviderCatalog(
            signedCatalog(content: withModels(row), revision: 1),
          ).reason,
          CodingSessionDecodeReason.malformedPayload,
          reason: row,
        );
      }
    });

    test('the tag envelope is exact', () {
      final wrongVersion = signedCatalog(
        tags: [
          ['h', _channel],
          ['cspc-v', 'cspc1-0'],
          ['cspc-revision', '3'],
          ['cspc-key', expectedKey(_channel, 3, rustCatalogContent)],
        ],
      );
      expect(
        decodeCodingSessionProviderCatalog(wrongVersion).reason,
        CodingSessionDecodeReason.badTags,
      );
      final wrongRevision = signedCatalog(
        tags: [
          ['h', _channel],
          ['cspc-v', 'cspc1-1'],
          ['cspc-revision', '4'],
          ['cspc-key', expectedKey(_channel, 3, rustCatalogContent)],
        ],
      );
      expect(
        decodeCodingSessionProviderCatalog(wrongRevision).reason,
        CodingSessionDecodeReason.badTags,
      );
    });

    test('a bad signature is refused before the payload is read', () {
      final event = signedCatalog();
      final forged = NostrEvent(
        id: event.id,
        pubkey: event.pubkey,
        createdAt: event.createdAt,
        kind: event.kind,
        tags: event.tags,
        content: event.content,
        sig: '00' * 64,
      );
      expect(
        decodeCodingSessionProviderCatalog(
          forged,
          verifier: const NostrPackageSignatureVerifier(),
        ).reason,
        CodingSessionDecodeReason.badSignature,
      );
    });

    test('another kind is not for this decoder', () {
      final event = signedCatalog();
      final other = NostrEvent(
        id: event.id,
        pubkey: event.pubkey,
        createdAt: event.createdAt,
        kind: EventKind.codingSessionMetadata,
        tags: event.tags,
        content: event.content,
        sig: event.sig,
      );
      expect(decodeCodingSessionProviderCatalog(other).isWrongKind, isTrue);
    });
  });

  group('newestCodingSessionProviderCatalogs', () {
    test('keeps the highest revision per signer, sorted by signer', () {
      final a1 = decodeCodingSessionProviderCatalog(
        signedCatalog(
          content: rustCatalogContent.replaceFirst(
            '"revision":3',
            '"revision":1',
          ),
          revision: 1,
          createdAt: 100,
        ),
      ).value!;
      final a3 = decodeCodingSessionProviderCatalog(
        signedCatalog(createdAt: 50),
      ).value!;
      final b1 = decodeCodingSessionProviderCatalog(
        signedCatalog(
          content: rustCatalogContent.replaceFirst(
            '"revision":3',
            '"revision":1',
          ),
          revision: 1,
          secretKey: _secretB,
        ),
      ).value!;
      final newest = newestCodingSessionProviderCatalogs([a1, b1, a3]);
      expect(newest, hasLength(2));
      final bySigner = {for (final c in newest) c.signerPubkey: c.revision};
      expect(bySigner[nostr.Keys(_secretA).public], 3);
      expect(bySigner[nostr.Keys(_secretB).public], 1);
      expect(
        newest.map((c) => c.signerPubkey).toList(),
        newest.map((c) => c.signerPubkey).toList()..sort(),
      );
    });
  });
}
