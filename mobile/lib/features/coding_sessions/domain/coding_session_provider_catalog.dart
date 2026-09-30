import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:pointycastle/digests/sha256.dart';

import '../../../shared/relay/nostr_models.dart';
import 'coding_session_decode_result.dart';
import 'coding_session_keys.dart';
import 'coding_session_models.dart';
import 'coding_session_signature.dart';
import 'coding_session_wire.dart';

/// The 44222 provider catalog: what a provider offers a channel, canonically.
///
/// A catalog is how a create learns that a provider exists, which runtimes
/// and models it will run, and — through its signer — the
/// `providerAuthorityPubkey` the create must name. Mirror of the desktop's
/// `codingSessionProviderCatalog.ts`: the tags are exact, the `cspc-key`
/// digests the signed bytes, and the content must be the producer's own
/// canonical serialization — a catalog this reader would re-serialize
/// differently is refused rather than trusted, because the key check is the
/// whole proof that nobody rewrote the offer in flight.
///
/// Authority is open, as on the desktop's display surfaces: the relay admits
/// 44222 only from channel members, so a signature-verified catalog in a
/// readable channel is an offer somebody in that channel signed. Which of
/// them to *run under* is the member's choice, made with the signer's name
/// in front of them.

/// The locked public payload schema for 44222.
const codingSessionProviderCatalogSchema =
    'buzz-coding-session-provider-catalog/v1';

/// The locked `cspc-v` tag version.
const codingSessionProviderCatalogTagVersion = 'cspc1-1';

/// The wire domain for a 44222 `cspc-key` semantic key.
const codingSessionProviderCatalogKeyDomain =
    'coding-session-provider-catalog/v1';

const _maxCatalogContentBytes = 256 * 1024;
const _maxReferenceBytes = 2 * 1024;
const _maxProviders = 32;
const _maxModels = 64;
// NIP-CSPC § Per-model rows: the runtime's own words about a model.
const _maxModelNameBytes = 128;
const _maxModelDescriptionBytes = 512;
const _maxModelEfforts = 16;
const _maxEffortBytes = 32;
// `rank` is a `u32` on the Rust side.
const _maxModelRank = 0xffffffff;
const _maxProjects = 512;

/// What a catalog says one provider instance can do.
@immutable
class CodingSessionProviderCapabilities {
  final bool threadTurnStart;
  final bool threadTurnInterrupt;
  final bool threadSteer;
  final bool context;
  final bool diff;
  final bool plan;

  /// The driver's static claim about image prompts; absent from a catalog
  /// published before the field existed, which reads as `false`.
  final bool promptImage;

  const CodingSessionProviderCapabilities({
    required this.threadTurnStart,
    required this.threadTurnInterrupt,
    required this.threadSteer,
    required this.context,
    required this.diff,
    required this.plan,
    required this.promptImage,
  });
}

/// One provider instance a catalog offers.
@immutable
class CodingSessionProviderOffer {
  /// The coordinate a create's `providerInstanceRef` names.
  final String providerInstanceRef;
  final String driver;
  final String runtime;

  /// Always `allowedModels.first`, by the catalog's canonical form.
  final String defaultModel;
  final List<String> allowedModels;
  final CodingSessionProviderCapabilities capabilities;

  const CodingSessionProviderOffer({
    required this.providerInstanceRef,
    required this.driver,
    required this.runtime,
    required this.defaultModel,
    required this.allowedModels,
    required this.capabilities,
  });
}

/// An optional narrowing: which offers serve one project coordinate.
@immutable
class CodingSessionProviderCatalogProject {
  final String projectRef;
  final String? repoRef;
  final List<String> providers;

  const CodingSessionProviderCatalogProject({
    required this.projectRef,
    required this.repoRef,
    required this.providers,
  });
}

/// A decoded, signature-checked 44222.
@immutable
class CodingSessionProviderCatalog {
  final CodingSessionEventRef ref;
  final int revision;
  final List<CodingSessionProviderOffer> providers;

  /// `null` when the catalog narrows nothing: every offer serves every
  /// project, which is what makes a create under a project the catalog never
  /// heard of possible.
  final List<CodingSessionProviderCatalogProject>? projects;

  const CodingSessionProviderCatalog({
    required this.ref,
    required this.revision,
    required this.providers,
    required this.projects,
  });

  /// The signer — the `providerAuthorityPubkey` a create under this catalog
  /// names, and the only pubkey whose facts about the session will count.
  String get signerPubkey => ref.signerPubkey;

  /// The offers that serve [projectRef] (desktop
  /// `codingSessionProvidersForProject`).
  List<CodingSessionProviderOffer> offersForProject(
    String? projectRef, {
    String? repoRef,
  }) {
    final narrowing = projects;
    if (narrowing == null || projectRef == null) return providers;
    for (final project in narrowing) {
      if (project.projectRef == projectRef && project.repoRef == repoRef) {
        return [
          for (final offer in providers)
            if (project.providers.contains(offer.providerInstanceRef)) offer,
        ];
      }
    }
    return const [];
  }
}

/// The 44222 `cspc-key`: channel, revision and the sha256 of the exact
/// signed content, domain-separated (desktop
/// `codingSessionProviderCatalogSemanticKey`).
String codingSessionProviderCatalogSemanticKey(
  String channelId,
  int revision,
  String content,
) => encodeStructuredKey(codingSessionProviderCatalogKeyDomain, [
  channelId,
  '$revision',
  _sha256Hex(content),
]);

String _sha256Hex(String content) {
  final digest = SHA256Digest().process(
    Uint8List.fromList(utf8.encode(content)),
  );
  return digest.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
}

/// Decode a 44222 catalog.
///
/// Every rejection is [CodingSessionDecodeReason.badTags],
/// [CodingSessionDecodeReason.badSignature] or
/// [CodingSessionDecodeReason.malformedPayload]; an event of another kind is
/// [CodingSessionDecodeReason.wrongKind] and not a defect.
CodingSessionDecoded<CodingSessionProviderCatalog>
decodeCodingSessionProviderCatalog(
  NostrEvent event, {
  CodingSessionSignatureVerifier? verifier,
}) {
  if (event.kind != EventKind.codingSessionProviderCatalog) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  final tags = parseExactTags(event.tags, [
    'h',
    'cspc-v',
    'cspc-revision',
    'cspc-key',
  ]);
  if (tags == null ||
      tags[0].isEmpty ||
      tags[1] != codingSessionProviderCatalogTagVersion) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  if (verifier != null &&
      verifier.verify(event) == CodingSessionSignatureVerdict.invalid) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.badSignature,
    );
  }
  final signer = normalizePubkey(event.pubkey);
  if (signer.isEmpty || !isHex64(event.id)) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }

  final value = parseBoundedJson(event.content, _maxCatalogContentBytes);
  if (!isPlainRecord(value)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final payload = value! as Map<String, dynamic>;
  final revision = payload['revision'];
  if (!hasRequiredAndOptionalKeys(
        payload,
        const ['schema', 'revision', 'providers'],
        const ['projects'],
      ) ||
      payload['schema'] != codingSessionProviderCatalogSchema ||
      revision is! int ||
      revision <= 0) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final providers = _parseProviders(payload['providers']);
  if (providers == null) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  List<CodingSessionProviderCatalogProject>? projects;
  if (payload.containsKey('projects')) {
    projects = _parseProjects(payload['projects'], providers);
    if (projects == null) {
      return const CodingSessionDecoded.failed(
        CodingSessionDecodeReason.malformedPayload,
      );
    }
  }
  // The producer's bytes must be its own canonical serialization: the
  // parsed values re-encoded compactly in the contract's key order. Anything
  // else means the content was rewritten after signing, or never canonical —
  // and the `cspc-key` below only proves what was signed, not that it was
  // well-formed.
  if (jsonEncode(_canonicalCatalog(payload)) != event.content) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  if (tags[2] != '$revision' ||
      tags[3] !=
          codingSessionProviderCatalogSemanticKey(
            tags[0],
            revision,
            event.content,
          )) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  return CodingSessionDecoded.ok(
    CodingSessionProviderCatalog(
      ref: CodingSessionEventRef(
        channelId: tags[0],
        eventId: event.id,
        signerPubkey: signer,
        createdAt: event.createdAt,
      ),
      revision: revision,
      providers: providers,
      projects: projects,
    ),
  );
}

List<CodingSessionProviderOffer>? _parseProviders(Object? value) {
  if (value is! List || value.isEmpty || value.length > _maxProviders) {
    return null;
  }
  final refs = <String>{};
  final providers = <CodingSessionProviderOffer>[];
  for (final raw in value) {
    final provider = _parseProvider(raw);
    if (provider == null || !refs.add(provider.providerInstanceRef)) {
      return null;
    }
    providers.add(provider);
  }
  return _isSorted(providers.map((p) => p.providerInstanceRef))
      ? providers
      : null;
}

CodingSessionProviderOffer? _parseProvider(Object? value) {
  if (!isPlainRecord(value)) return null;
  final record = value! as Map<String, dynamic>;
  if (!hasRequiredAndOptionalKeys(
    record,
    const [
      'providerInstanceRef',
      'driver',
      'runtime',
      'defaultModel',
      'allowedModels',
      'capabilities',
    ],
    const ['models'],
  )) {
    return null;
  }
  final allowed = record['allowedModels'];
  if (!_nonblank(record['providerInstanceRef']) ||
      !_nonblank(record['driver']) ||
      !_nonblank(record['runtime']) ||
      !_nonblank(record['defaultModel']) ||
      allowed is! List ||
      allowed.isEmpty ||
      allowed.length > _maxModels ||
      !isPlainRecord(record['capabilities'])) {
    return null;
  }
  final models = <String>[];
  for (final model in allowed) {
    if (!_nonblank(model) || models.contains(model)) return null;
    models.add(model as String);
  }
  // Canonical form: the default is `allowedModels[0]` and the rest are
  // sorted (the provider's `catalog::build`), so one offer has one spelling.
  if (models.first != record['defaultModel'] || !_isSorted(models.skip(1))) {
    return null;
  }
  final capabilities = _parseCapabilities(
    record['capabilities']! as Map<String, dynamic>,
  );
  if (capabilities == null) return null;
  if (record.containsKey('models') &&
      !_describedModelsAreCanonical(record['models'], models)) {
    return null;
  }
  return CodingSessionProviderOffer(
    providerInstanceRef: record['providerInstanceRef'] as String,
    driver: record['driver'] as String,
    runtime: record['runtime'] as String,
    defaultModel: record['defaultModel'] as String,
    allowedModels: List.unmodifiable(models),
    capabilities: capabilities,
  );
}

CodingSessionProviderCapabilities? _parseCapabilities(
  Map<String, dynamic> value,
) {
  const required = [
    'threadTurnStart',
    'threadTurnInterrupt',
    'threadSteer',
    'context',
    'diff',
    'plan',
  ];
  if (!hasRequiredAndOptionalKeys(value, required, const ['promptImage'])) {
    return null;
  }
  for (final key in required) {
    if (value[key] is! bool) return null;
  }
  final promptImage = value['promptImage'];
  if (value.containsKey('promptImage') && promptImage is! bool) return null;
  return CodingSessionProviderCapabilities(
    threadTurnStart: value['threadTurnStart'] as bool,
    threadTurnInterrupt: value['threadTurnInterrupt'] as bool,
    threadSteer: value['threadSteer'] as bool,
    context: value['context'] as bool,
    diff: value['diff'] as bool,
    plan: value['plan'] as bool,
    promptImage: promptImage == true,
  );
}

/// `models[]` describes ids `allowedModels` already offers, in that order,
/// each once, each with at least one fact beyond its id (desktop
/// `parseCatalogModels`).
bool _describedModelsAreCanonical(Object? value, List<String> allowed) {
  if (value is! List || value.isEmpty || value.length > allowed.length) {
    return false;
  }
  var cursor = 0;
  for (final raw in value) {
    if (!isPlainRecord(raw)) return false;
    final row = raw! as Map<String, dynamic>;
    if (!hasRequiredAndOptionalKeys(
          row,
          const ['id'],
          const [
            'contextWindow',
            'family',
            'vendor',
            'deprecated',
            'name',
            'description',
            'efforts',
            'fastMode',
            'rank',
          ],
        ) ||
        row.length < 2 ||
        !_nonblank(row['id'])) {
      return false;
    }
    final index = allowed.indexOf(row['id'] as String, cursor);
    if (index < 0) return false;
    cursor = index + 1;
    final window = row['contextWindow'];
    if (row.containsKey('contextWindow') && (window is! int || window <= 0)) {
      return false;
    }
    if (row.containsKey('family') && !_nonblank(row['family'])) return false;
    if (row.containsKey('vendor') && !_nonblank(row['vendor'])) return false;
    if (row.containsKey('deprecated') && row['deprecated'] is! bool) {
      return false;
    }
    if (!_runtimeModelFactsAreCanonical(row)) return false;
  }
  return true;
}

/// Name, description, efforts, fast mode and rank, checked as `buzz-core`'s
/// `check_models` checks them (desktop `runtimeModelFactsAreCanonical`). An
/// empty `efforts` or a `false` fast mode is refused: the producer omits both.
bool _runtimeModelFactsAreCanonical(Map<String, dynamic> row) {
  if (row.containsKey('name') &&
      !boundedNonempty(row['name'], _maxModelNameBytes)) {
    return false;
  }
  if (row.containsKey('description') &&
      !boundedNonempty(row['description'], _maxModelDescriptionBytes)) {
    return false;
  }
  if (row.containsKey('efforts')) {
    final efforts = row['efforts'];
    if (efforts is! List ||
        efforts.isEmpty ||
        efforts.length > _maxModelEfforts ||
        efforts.toSet().length != efforts.length ||
        !efforts.every((effort) => boundedNonempty(effort, _maxEffortBytes))) {
      return false;
    }
  }
  if (row.containsKey('fastMode') && row['fastMode'] != true) return false;
  final rank = row['rank'];
  if (row.containsKey('rank') &&
      (rank is! int || rank < 0 || rank > _maxModelRank)) {
    return false;
  }
  return true;
}

List<CodingSessionProviderCatalogProject>? _parseProjects(
  Object? value,
  List<CodingSessionProviderOffer> providers,
) {
  // An empty narrowing is never canonical: the producer omits the key.
  if (value is! List || value.isEmpty || value.length > _maxProjects) {
    return null;
  }
  final declared = {for (final p in providers) p.providerInstanceRef};
  final coordinates = <String>{};
  final projects = <CodingSessionProviderCatalogProject>[];
  for (final raw in value) {
    if (!isPlainRecord(raw)) return null;
    final record = raw! as Map<String, dynamic>;
    final refs = record['providers'];
    if (!hasExactKeys(record, const ['projectRef', 'repoRef', 'providers']) ||
        !_nonblank(record['projectRef']) ||
        !boundedNullable(record['repoRef'], _maxReferenceBytes) ||
        refs is! List ||
        refs.isEmpty ||
        refs.length > _maxProviders) {
      return null;
    }
    final coordinate = '${record['projectRef']} ${record['repoRef'] ?? ''}';
    if (!coordinates.add(coordinate)) return null;
    final seen = <String>{};
    for (final ref in refs) {
      // A narrowing that names a provider this catalog never offered is a
      // claim about something the signer did not declare.
      if (!_nonblank(ref) ||
          !seen.add(ref as String) ||
          !declared.contains(ref)) {
        return null;
      }
    }
    if (!_isSorted(refs.cast<String>())) return null;
    projects.add(
      CodingSessionProviderCatalogProject(
        projectRef: record['projectRef'] as String,
        repoRef: record['repoRef'] as String?,
        providers: List.unmodifiable(refs.cast<String>()),
      ),
    );
  }
  return _isSorted(projects.map((p) => '${p.projectRef} ${p.repoRef ?? ''}'))
      ? projects
      : null;
}

/// The parsed payload with every object rebuilt in the contract's key order,
/// keeping only the keys the producer wrote. Compared byte-for-byte with the
/// content, this is the order check the desktop makes with `hasOrderedKeys`.
Map<String, Object?> _canonicalCatalog(Map<String, dynamic> payload) =>
    _ordered(
      payload,
      const ['schema', 'revision', 'providers', 'projects'],
      {
        'providers': (value) => [
          for (final provider in value as List)
            _ordered(
              provider as Map<String, dynamic>,
              const [
                'providerInstanceRef',
                'driver',
                'runtime',
                'defaultModel',
                'allowedModels',
                'capabilities',
                'models',
              ],
              {
                'capabilities': (capabilities) =>
                    _ordered(capabilities as Map<String, dynamic>, const [
                      'threadTurnStart',
                      'threadTurnInterrupt',
                      'threadSteer',
                      'context',
                      'diff',
                      'plan',
                      'promptImage',
                    ], const {}),
                'models': (models) => [
                  for (final model in models as List)
                    _ordered(model as Map<String, dynamic>, const [
                      'id',
                      'contextWindow',
                      'family',
                      'vendor',
                      'deprecated',
                      'name',
                      'description',
                      'efforts',
                      'fastMode',
                      'rank',
                    ], const {}),
                ],
              },
            ),
        ],
        'projects': (value) => [
          for (final project in value as List)
            _ordered(project as Map<String, dynamic>, const [
              'projectRef',
              'repoRef',
              'providers',
            ], const {}),
        ],
      },
    );

Map<String, Object?> _ordered(
  Map<String, dynamic> value,
  List<String> order,
  Map<String, Object? Function(Object? value)> nested,
) => {
  for (final key in order)
    if (value.containsKey(key))
      key: nested.containsKey(key) ? nested[key]!(value[key]) : value[key],
};

bool _nonblank(Object? value) =>
    value is String &&
    value.trim().isNotEmpty &&
    boundedNonempty(value, _maxReferenceBytes);

bool _isSorted(Iterable<String> values) {
  String? previous;
  for (final value in values) {
    if (previous != null && previous.compareTo(value) > 0) return false;
    previous = value;
  }
  return true;
}

/// The newest catalog per signer in one channel: what a create's provider
/// picker lists (desktop `CodingSessionProviderCatalogStore`, minus the
/// cross-signer reconciliation — a create names its provider by signer *and*
/// instance ref together, so two signers offering the same ref are two
/// offers, not one ambiguity).
///
/// Per signer the highest revision wins; at the same revision the newest
/// `created_at`, then the lowest event id, so every reader lands on the same
/// one.
List<CodingSessionProviderCatalog> newestCodingSessionProviderCatalogs(
  Iterable<CodingSessionProviderCatalog> catalogs,
) {
  final bySigner = <String, CodingSessionProviderCatalog>{};
  for (final catalog in catalogs) {
    final current = bySigner[catalog.signerPubkey];
    if (current == null || _fresher(catalog, current)) {
      bySigner[catalog.signerPubkey] = catalog;
    }
  }
  final result = bySigner.values.toList()
    ..sort((a, b) => a.signerPubkey.compareTo(b.signerPubkey));
  return result;
}

bool _fresher(
  CodingSessionProviderCatalog candidate,
  CodingSessionProviderCatalog current,
) {
  if (candidate.revision != current.revision) {
    return candidate.revision > current.revision;
  }
  if (candidate.ref.createdAt != current.ref.createdAt) {
    return candidate.ref.createdAt > current.ref.createdAt;
  }
  return candidate.ref.eventId.compareTo(current.ref.eventId) < 0;
}
