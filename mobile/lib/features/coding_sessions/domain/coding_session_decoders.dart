import '../../../shared/relay/nostr_models.dart';
import 'coding_session_decode_result.dart';
import 'coding_session_keys.dart';
import 'coding_session_models.dart';
import 'coding_session_signature.dart';
import 'coding_session_target.dart';
import 'coding_session_wire.dart';

/// Strict decoders for the per-generation fact kinds a provider signs:
/// 44223 metadata, 44224 lifecycle/turn receipts, 44225 transcript envelopes,
/// and the 24223 lease that proves reachability.
///
/// Each mirrors the desktop reader exactly — same tag order, same exact-key
/// discipline, same byte bounds — because the two clients must agree on which
/// events exist before they can agree on what they say.

/// Schema string on a 44223 payload.
const codingSessionMetadataSchema = 'buzz-coding-session-metadata/v1';

/// Tag version on a 44223 event (`csm-v`).
const codingSessionMetadataTagVersion = 'csm1-1';

/// Schema string on a 44224 payload.
const codingSessionReceiptSchema = 'buzz-coding-session-lifecycle-receipt/v1';

/// Tag version on a 44224 event (`cslr-v`).
const codingSessionReceiptTagVersion = 'cslr1-1';

/// Schema string on a 44225 payload.
const codingSessionTranscriptSchema = 'buzz-coding-session-transcript/v1';

/// Tag version on a 44225 event (`cst-v`).
const codingSessionTranscriptTagVersion = 'cst1-1';

/// Schema string on a 24223 payload.
const codingSessionLeaseSchema = 'buzz-coding-session-lease/v1';

/// Tag version on a 24223 event (`cslease-v`).
const codingSessionLeaseTagVersion = 'cslease1-1';

const _maxMetadataContentBytes = 32 * 1024;
const _maxReceiptContentBytes = 16 * 1024;
const _maxTranscriptContentBytes = 32 * 1024;
const _maxLeaseContentBytes = 4 * 1024;
const _maxTranscriptItemDepth = 24;
const _maxTranscriptIdentityBytes = 512;
const _maxIdentifierBytes = 256;

/// A lifecycle receipt's error code, bounded as `validate_lifecycle_receipt`'s
/// generic check bounds it (`coding_session_payload.rs:826`).
const _maxErrorCodeBytes = 256;

/// A **turn** failure code: `MAX_RECEIPT_ERROR_CODE_BYTES`
/// (`coding_session_payload.rs:278`) — 64, the narrower of the two. The
/// asymmetry is the writer's: `is_receipt_error_code` is applied only to
/// `turn_dropped`/`turn_refused`/`turn_degraded`/`turn_delivery_unknown`,
/// whose code vocabulary is deliberately open and therefore kept tight. This
/// decoder used 256 for both until lane 216, so it accepted turn codes the
/// writer's own validator — and so the relay — call malformed.
const _maxTurnErrorCodeBytes = 64;

/// `1024 + '…'.len_utf8()` — the exact bound `validate_lifecycle_receipt`
/// (`coding_session_payload.rs:829`) sets on a receipt error message. This
/// decoder used the generic 2 KiB reference bound.
const _maxErrorMessageBytes = 1024 + 3;
const _maxReferenceBytes = 2 * 1024;
const _maxLabelBytes = 2 * 1024;

/// `MAX_COMPOSE_REF_APP_VERSION_BYTES` (`coding_session_payload.rs`).
const _maxComposeRefAppVersionBytes = 64;

/// Decode a 44223 per-generation metadata event.
CodingSessionDecoded<CodingSessionMetadata> decodeCodingSessionMetadata(
  NostrEvent event, {
  CodingSessionSignatureVerifier? verifier,
}) {
  if (event.kind != EventKind.codingSessionMetadata) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  final tags = parseExactTags(event.tags, [
    'h',
    'csm-v',
    'cs-target',
    'csm-key',
  ]);
  if (tags == null ||
      tags[0].isEmpty ||
      tags[1] != codingSessionMetadataTagVersion) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final ref = _eventRef(event, tags[0]);
  if (ref == null) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final signature = _checkSignature(event, verifier);
  if (signature != null) return CodingSessionDecoded.failed(signature);

  final value = parseBoundedJson(event.content, _maxMetadataContentBytes);
  const required = [
    'schema',
    'session',
    'projectRef',
    'repoRef',
    'title',
    'agentRef',
    'provider',
    'runtime',
    'model',
    'status',
    'branch',
    'capabilities',
  ];
  // The B1 code-coordinate amendment: all four keys or none. They are
  // validated here so an amended payload is not read as corruption, then
  // dropped — this observer surfaces no code coordinates in v1.
  const facts = ['observedCommit', 'dirty', 'relayReachable', 'verifiedAt'];
  // Every additive amendment buzz-core has landed on this payload. `role`,
  // `turnBudget` and the 2026-08-30 `routing` record were all missing from
  // this list, so a seated, budgeted or routed session decoded as corruption
  // here and vanished from the observer — a forgotten amendment is a blank
  // session list, not a strictness nuance. It happened again: `beeStamp`,
  // `packRef`, `handover` and `composeRef` were all missing until lane 216,
  // and `composeRef` is emitted for every seat staged from a composed pack.
  // `contextSummary`/`diffSummary`/`planSummary` went the other way and are
  // gone: `METADATA_BASE_FIELDS` never named them, and the only writer of a
  // 44223 is `serde_json::to_string(&SessionMetadata)` in the provider, whose
  // struct has no such fields — no signed event has ever carried one.
  // This app surfaces none of these in v1; they are named so an amended
  // payload is read rather than dropped.
  const optional = [
    'sessionRef',
    'role',
    'turnBudget',
    'routing',
    'beeStamp',
    'packRef',
    'handover',
    'composeRef',
    ...facts,
  ];
  if (!isPlainRecord(value)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final payload = value! as Map<String, dynamic>;
  if (!hasRequiredAndOptionalKeys(payload, required, optional) ||
      payload['schema'] != codingSessionMetadataSchema) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  // The three amendments above are read but not surfaced; a present, non-null
  // one that is malformed is still corruption and is refused, exactly as the
  // desktop and the relay refuse it.
  // An explicit `null` is refused, not short-circuited past: `routing`,
  // `beeStamp`, `packRef`, `handover` and `composeRef` were every one of them
  // introduced with an omit-when-absent writer contract, and
  // `decode_coding_session_metadata` refuses each null **naming the key**.
  // `role` and `turnBudget` are the exception — their serde `Option`s do read
  // a null as absent, so a null there is accepted here too.
  if (payload.containsKey('routing') &&
      !isStrictRoutingRecord(payload['routing'])) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  if (payload.containsKey('turnBudget') &&
      payload['turnBudget'] != null &&
      !_isTurnBudget(payload['turnBudget'], payload['sessionRef'])) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  if (payload.containsKey('beeStamp') && !_isBeeStamp(payload['beeStamp'])) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  if (payload.containsKey('packRef') && !_isPackRef(payload['packRef'])) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  if (payload.containsKey('handover') &&
      !_isMetadataHandover(payload['handover'])) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  // A composition of nothing is not a fact: `validate_session_metadata`
  // refuses `composeRef` without a `packRef`, so this decoder does too.
  if (payload.containsKey('composeRef') &&
      (!payload.containsKey('packRef') ||
          !_isComposeRef(payload['composeRef']))) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final target = CodingSessionTarget.decode(payload['session']);
  final capabilities = _decodeCapabilities(payload['capabilities']);
  final status = CodingSessionStatus.fromWire(payload['status']);
  if (target == null || capabilities == null || status == null) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  if (!boundedNullable(payload['projectRef'], _maxReferenceBytes) ||
      !boundedNullable(payload['repoRef'], _maxReferenceBytes) ||
      !boundedNullable(payload['title'], _maxLabelBytes) ||
      !boundedNullable(payload['agentRef'], _maxReferenceBytes) ||
      !boundedNullable(payload['provider'], _maxLabelBytes) ||
      !boundedNullable(payload['runtime'], _maxLabelBytes) ||
      !boundedNullable(payload['model'], _maxLabelBytes) ||
      !boundedNullable(payload['branch'], _maxLabelBytes)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  if (payload.containsKey('sessionRef') &&
      !isCodingSessionSessionRef(payload['sessionRef'])) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  if (!hasAllOrNoneKeys(payload, facts) || !_factFieldsValid(payload)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  if (tags[2] != target.key || tags[3] != target.metadataSemanticKey) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  // Canonicalise the validated payload here, once, rather than reassembling
  // it downstream from the fields this observer happens to model: everything
  // the decoder accepted — including the code coordinates it drops — is part
  // of what a same-second conflict is a conflict about.
  final canonicalPayload = canonicalJson(payload);
  if (canonicalPayload == null) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  return CodingSessionDecoded.ok(
    CodingSessionMetadata(
      ref: ref,
      target: target,
      status: status,
      capabilities: capabilities,
      canonicalPayload: canonicalPayload,
      projectRef: payload['projectRef'] as String?,
      repoRef: payload['repoRef'] as String?,
      title: payload['title'] as String?,
      agentRef: payload['agentRef'] as String?,
      provider: payload['provider'] as String?,
      runtime: payload['runtime'] as String?,
      model: payload['model'] as String?,
      branch: payload['branch'] as String?,
      sessionRef: payload['sessionRef'] as String?,
    ),
  );
}

/// Decode a 44224 lifecycle or turn receipt.
///
/// Exactly five keys — `{schema, commandId, status, session, error}` — or six
/// with `turnId` when and only when the status is `turn_started` or
/// `turn_injected`. An unexpected key is a rejection, never a partial accept.
/// An unrecognised status string fails closed, as it always has.
CodingSessionDecoded<CodingSessionReceipt> decodeCodingSessionReceipt(
  NostrEvent event, {
  CodingSessionSignatureVerifier? verifier,
}) {
  if (event.kind != EventKind.codingSessionLifecycleReceipt) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  final tags = parseExactTags(event.tags, [
    'h',
    'cslr-v',
    'csl-command',
    'csl-key',
  ]);
  if (tags == null ||
      tags[0].isEmpty ||
      tags[1] != codingSessionReceiptTagVersion) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final ref = _eventRef(event, tags[0]);
  if (ref == null) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final signature = _checkSignature(event, verifier);
  if (signature != null) return CodingSessionDecoded.failed(signature);

  final value = parseBoundedJson(event.content, _maxReceiptContentBytes);
  if (!isPlainRecord(value)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final payload = value! as Map<String, dynamic>;
  final status = CodingSessionReceiptStatus.fromWire(payload['status']);
  if (status == null) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  const envelope = ['schema', 'commandId', 'status', 'session', 'error'];
  final expected = status.carriesTurnId ? [...envelope, 'turnId'] : envelope;
  if (!hasExactKeys(payload, expected) ||
      payload['schema'] != codingSessionReceiptSchema ||
      !boundedNonempty(payload['commandId'], _maxIdentifierBytes)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final commandId = payload['commandId'] as String;
  if (tags[2] != commandId ||
      tags[3] != codingSessionReceiptSemanticKey(commandId, status)) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final target = CodingSessionTarget.decode(payload['session']);
  final error = _decodeReceiptError(payload['error'], status);
  final hasError = payload['error'] != null;
  if (hasError && error == null) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  // `failed` is the one status with no execution: it reports that the create
  // never minted one. Every other status names the execution it is about,
  // including the turn statuses that answer "that generation is stale".
  if (status == CodingSessionReceiptStatus.failed) {
    if (payload['session'] != null || error == null) {
      return const CodingSessionDecoded.failed(
        CodingSessionDecodeReason.malformedPayload,
      );
    }
  } else if (target == null) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final expectation = _receiptErrorExpectation(status);
  if (expectation != null) {
    if (expectation.mustHaveError && error == null) {
      return const CodingSessionDecoded.failed(
        CodingSessionDecodeReason.malformedPayload,
      );
    }
    if (!expectation.mustHaveError && error != null) {
      return const CodingSessionDecoded.failed(
        CodingSessionDecodeReason.malformedPayload,
      );
    }
    final code = expectation.code;
    if (code != null && error != null && error.code != code) {
      return const CodingSessionDecoded.failed(
        CodingSessionDecodeReason.malformedPayload,
      );
    }
  }
  String? turnId;
  if (status.carriesTurnId) {
    if (!boundedNonempty(payload['turnId'], _maxIdentifierBytes)) {
      return const CodingSessionDecoded.failed(
        CodingSessionDecodeReason.malformedPayload,
      );
    }
    turnId = payload['turnId'] as String;
  }
  return CodingSessionDecoded.ok(
    CodingSessionReceipt(
      ref: ref,
      commandId: commandId,
      status: status,
      session: target,
      error: error,
      turnId: turnId,
    ),
  );
}

/// Decode a 44225 transcript envelope.
CodingSessionDecoded<CodingSessionTranscriptEnvelope>
decodeCodingSessionTranscript(
  NostrEvent event, {
  CodingSessionSignatureVerifier? verifier,
}) {
  if (event.kind != EventKind.codingSessionTranscript) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  final tags = parseExactTags(event.tags, [
    'h',
    'cst-v',
    'cs-target',
    'cst-seq',
    'cst-key',
  ]);
  if (tags == null ||
      tags[0].isEmpty ||
      tags[1] != codingSessionTranscriptTagVersion) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final ref = _eventRef(event, tags[0]);
  if (ref == null) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final signature = _checkSignature(event, verifier);
  if (signature != null) return CodingSessionDecoded.failed(signature);

  final value = parseBoundedJson(event.content, _maxTranscriptContentBytes);
  if (!isPlainRecord(value)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final payload = value! as Map<String, dynamic>;
  if (!hasExactKeys(payload, [
        'schema',
        'session',
        'eventSeq',
        'timestamp',
        'turnId',
        'item',
      ]) ||
      payload['schema'] != codingSessionTranscriptSchema) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final target = CodingSessionTarget.decode(
    payload['session'],
    maxIdentityBytes: _maxTranscriptIdentityBytes,
  );
  final eventSeq = payload['eventSeq'];
  final timestamp = payload['timestamp'];
  final turnId = payload['turnId'];
  final item = payload['item'];
  if (target == null ||
      eventSeq is! int ||
      eventSeq <= 0 ||
      timestamp is! int ||
      !(turnId == null ||
          boundedNonempty(turnId, _maxTranscriptIdentityBytes)) ||
      !isPlainRecord(item) ||
      !boundedNonempty(
        (item! as Map<String, dynamic>)['kind'],
        _maxTranscriptIdentityBytes,
      ) ||
      !isWithinDepth(item, _maxTranscriptItemDepth)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  if (tags[2] != target.key ||
      tags[3] != '$eventSeq' ||
      tags[4] != target.transcriptSemanticKey(eventSeq)) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  return CodingSessionDecoded.ok(
    CodingSessionTranscriptEnvelope(
      ref: ref,
      target: target,
      eventSeq: eventSeq,
      timestamp: timestamp,
      turnId: turnId as String?,
      item: item as Map<String, dynamic>,
    ),
  );
}

/// Decode a 24223 provider lease.
///
/// Exact ordered tags `h / cslease-v / cs-target / csl-command / cslease-seq`
/// and a four-key content. A lease is the only proof of reachability there is,
/// so nothing looser may be read as one.
CodingSessionDecoded<CodingSessionLease> decodeCodingSessionLease(
  NostrEvent event, {
  CodingSessionSignatureVerifier? verifier,
}) {
  if (event.kind != EventKind.codingSessionLease) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  final tags = parseExactTags(event.tags, [
    'h',
    'cslease-v',
    'cs-target',
    'csl-command',
    'cslease-seq',
  ]);
  if (tags == null ||
      tags[0].isEmpty ||
      tags[1] != codingSessionLeaseTagVersion ||
      tags[3].isEmpty) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final ref = _eventRef(event, tags[0]);
  if (ref == null) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final signature = _checkSignature(event, verifier);
  if (signature != null) return CodingSessionDecoded.failed(signature);

  final value = parseBoundedJson(event.content, _maxLeaseContentBytes);
  if (!isPlainRecord(value)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final payload = value! as Map<String, dynamic>;
  if (!hasExactKeys(payload, ['schema', 'target', 'state', 'leaseSequence']) ||
      payload['schema'] != codingSessionLeaseSchema) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final target = CodingSessionTarget.decode(payload['target']);
  final state = CodingSessionLeaseState.fromWire(payload['state']);
  final sequence = payload['leaseSequence'];
  if (target == null || state == null || sequence is! int || sequence <= 0) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  if (tags[2] != target.key || tags[4] != '$sequence') {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  return CodingSessionDecoded.ok(
    CodingSessionLease(
      ref: ref,
      target: target,
      commandId: tags[3],
      state: state,
      leaseSequence: sequence,
    ),
  );
}

/// The `csl-key` a 44224 carries for [commandId] at [status].
///
/// One create publishes one lifecycle receipt, so those keep the single-field
/// key. One turn publishes several receipts, so a turn receipt's key names its
/// stage as well — keying by command id alone would fence the second receipt
/// out as a duplicate of the first.
String codingSessionReceiptSemanticKey(
  String commandId,
  CodingSessionReceiptStatus status,
) => status.isTurnStage
    ? encodeStructuredKey(codingSessionReceiptKeyDomain, [
        commandId,
        status.wire,
      ])
    : encodeStructuredKey(codingSessionReceiptKeyDomain, [commandId]);

CodingSessionEventRef? _eventRef(NostrEvent event, String channelId) {
  final signerPubkey = normalizePubkey(event.pubkey);
  if (signerPubkey.isEmpty || !isHex64(event.id) || event.createdAt <= 0) {
    return null;
  }
  return CodingSessionEventRef(
    channelId: channelId,
    eventId: event.id,
    signerPubkey: signerPubkey,
    createdAt: event.createdAt,
  );
}

CodingSessionDecodeReason? _checkSignature(
  NostrEvent event,
  CodingSessionSignatureVerifier? verifier,
) {
  if (verifier == null) return null;
  return verifier.verify(event) == CodingSessionSignatureVerdict.invalid
      ? CodingSessionDecodeReason.badSignature
      : null;
}

Map<String, bool>? _decodeCapabilities(Object? value) {
  const keys = [
    'threadTurnStart',
    'threadTurnInterrupt',
    'threadSteer',
    'context',
    'diff',
    'plan',
  ];
  // `promptImage` is optional, not exact-keyed — the same rule as the
  // desktop's `decodeCapabilities` and this app's own catalog decoder. It was
  // exact-keyed here, so every 44223 from a provider that publishes the
  // amendment (all of them, since 2026-09) decoded as corruption: no status,
  // no title, no model — a session list of "unknown" chips over live
  // sessions (live finding 2026-09-08, phone vs desktop on the same relay).
  // `modelSwitch` (SV-35) is optional for the same reason: it is omitted when
  // false, so only a switchable execution's 44223 carries it.
  const optional = ['promptImage', 'modelSwitch'];
  if (!isPlainRecord(value)) return null;
  final record = value! as Map<String, dynamic>;
  if (!hasRequiredAndOptionalKeys(record, keys, optional)) return null;
  final capabilities = <String, bool>{};
  for (final key in keys) {
    final flag = record[key];
    if (flag is! bool) return null;
    capabilities[key] = flag;
  }
  for (final key in optional) {
    if (!record.containsKey(key)) continue;
    final flag = record[key];
    if (flag is! bool) return null;
    capabilities[key] = flag;
  }
  return Map.unmodifiable(capabilities);
}

bool _factFieldsValid(Map<String, dynamic> payload) {
  if (!payload.containsKey('observedCommit')) return true;
  final dirty = payload['dirty'];
  final relayReachable = payload['relayReachable'];
  final verifiedAt = payload['verifiedAt'];
  return boundedNullable(payload['observedCommit'], _maxLabelBytes) &&
      (dirty == null || dirty is bool) &&
      (relayReachable == null || relayReachable is bool) &&
      (verifiedAt == null || verifiedAt is int) &&
      (relayReachable == null) == (verifiedAt == null);
}

/// A `turnBudget`: exactly `{used, limit}`, and only beside an umbrella.
///
/// A budget is a fact about an umbrella, so it cannot describe an execution
/// that claimed no `sessionRef`, and a `limit` of zero would read as "no turn
/// may ever pass" rather than "unbudgeted" — the producer omits the key
/// instead. Both are `validate_session_metadata`'s own rejections. This
/// decoder validated the object not at all until lane 216, so an unknown key
/// inside it decoded as a budget.
bool _isTurnBudget(Object? value, Object? sessionRef) {
  if (sessionRef is! String || !isPlainRecord(value)) return false;
  final record = value! as Map<String, dynamic>;
  final used = record['used'];
  final limit = record['limit'];
  return hasExactKeys(record, ['used', 'limit']) &&
      used is int &&
      used >= 0 &&
      limit is int &&
      limit > 0;
}

const _beeStampSources = {'bundled', 'path'};
final _shortShaPattern = RegExp(r'^[0-9a-f]{7,40}$');
final _exactShaPattern = RegExp(r'^[0-9a-f]{40}$');
final _packRefRoleSlugPattern = RegExp(r'^[a-z0-9-]{1,64}$');
final _packRefRepoCoordPattern = RegExp(
  r'^30617:[0-9a-f]{64}:[a-zA-Z0-9._-]{1,200}$',
);
final _packRefAppVersionPattern = RegExp(
  r'^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$',
);
final _composeRefDigestPattern = RegExp(r'^sha256:[0-9a-f]{64}$');

/// The literal `repo` a shipped-defaults `packRef` carries.
const _packRefShippedRepo = 'app:shipped';

/// A `beeStamp`: exactly the five keys `BeeStamp` carries, each on its own
/// terms. `source` is a genuine two-variant Rust enum, so it stays closed.
bool _isBeeStamp(Object? value) {
  if (!isPlainRecord(value)) return false;
  final record = value! as Map<String, dynamic>;
  final path = record['path'];
  final version = record['version'];
  final sha = record['sha'];
  final dirty = record['dirty'];
  return hasExactKeys(record, ['path', 'source', 'version', 'sha', 'dirty']) &&
      path is String &&
      path.isNotEmpty &&
      _beeStampSources.contains(record['source']) &&
      (version == null || version is String) &&
      (sha == null || (sha is String && _shortShaPattern.hasMatch(sha))) &&
      (dirty == null || dirty is bool);
}

/// A `packRef`: exactly the four keys `PackRef` carries. `sha` is an exact
/// 40-hex commit — never the 7-40 shorthand a `beeStamp` allows, because a
/// pack is pinned to one commit, never a prefix — unless `repo` is the
/// shipped-defaults literal, whose `sha` is the app version instead.
bool _isPackRef(Object? value) {
  if (!isPlainRecord(value)) return false;
  final record = value! as Map<String, dynamic>;
  final repo = record['repo'];
  final sha = record['sha'];
  final role = record['role'];
  final path = record['path'];
  if (!hasExactKeys(record, ['repo', 'sha', 'role', 'path']) ||
      repo is! String ||
      sha is! String ||
      role is! String ||
      !_packRefRoleSlugPattern.hasMatch(role) ||
      path is! String ||
      path.isEmpty ||
      path.length > 512) {
    return false;
  }
  if (repo == _packRefShippedRepo) {
    return _packRefAppVersionPattern.hasMatch(sha);
  }
  return _packRefRepoCoordPattern.hasMatch(repo) &&
      _exactShaPattern.hasMatch(sha);
}

/// A `handover`: exactly the four keys `SessionMetadataHandover` carries.
///
/// There is no "none" token — absence of the whole key is how a provider says
/// no claim stands — and a partial object is refused rather than read
/// loosely: reading a half-written fence as "not fenced" tells a person
/// nobody took this session over.
bool _isMetadataHandover(Object? value) {
  if (!isPlainRecord(value)) return false;
  final record = value! as Map<String, dynamic>;
  final state = record['state'];
  return hasExactKeys(record, [
        'state',
        'claimant',
        'bodyPubkey',
        'acceptedEventId',
      ]) &&
      (state == 'active' || state == 'voided') &&
      isHex64(record['claimant']) &&
      isHex64(record['bodyPubkey']) &&
      isHex64(record['acceptedEventId']);
}

/// A `composeRef`: exactly `{appVersion, digest}` (spec § 4.6).
bool _isComposeRef(Object? value) {
  if (!isPlainRecord(value)) return false;
  final record = value! as Map<String, dynamic>;
  final appVersion = record['appVersion'];
  final digest = record['digest'];
  return hasExactKeys(record, ['appVersion', 'digest']) &&
      boundedNonempty(appVersion, _maxComposeRefAppVersionBytes) &&
      digest is String &&
      _composeRefDigestPattern.hasMatch(digest);
}

/// The four turn stages whose `error.code` buzz-core bounds at 64 bytes.
const _turnFailureStatuses = {
  CodingSessionReceiptStatus.turnDropped,
  CodingSessionReceiptStatus.turnRefused,
  CodingSessionReceiptStatus.turnDegraded,
  CodingSessionReceiptStatus.turnDeliveryUnknown,
};

CodingSessionReceiptError? _decodeReceiptError(
  Object? value,
  CodingSessionReceiptStatus status,
) {
  if (!isPlainRecord(value)) return null;
  final record = value! as Map<String, dynamic>;
  final maxCodeBytes = _turnFailureStatuses.contains(status)
      ? _maxTurnErrorCodeBytes
      : _maxErrorCodeBytes;
  if (!hasExactKeys(record, ['code', 'message']) ||
      !boundedNonempty(record['code'], maxCodeBytes) ||
      !boundedNonempty(record['message'], _maxErrorMessageBytes)) {
    return null;
  }
  return CodingSessionReceiptError(
    code: record['code'] as String,
    message: record['message'] as String,
  );
}

class _ReceiptErrorExpectation {
  const _ReceiptErrorExpectation({required this.mustHaveError, this.code});

  final bool mustHaveError;
  final String? code;
}

/// What `error` must look like for a given status.
///
/// `null` means "either shape is legal": the two forward-compatible turn
/// statuses (`turn_degraded`, `interrupt_delivered`) are not in the producer's
/// contract on this branch, so pinning their error shape would be inventing
/// one. They still decode as turn stages, which is the property that matters —
/// no turn receipt ever creates or ends a generation.
_ReceiptErrorExpectation? _receiptErrorExpectation(
  CodingSessionReceiptStatus status,
) => switch (status) {
  CodingSessionReceiptStatus.created ||
  CodingSessionReceiptStatus.resumed ||
  CodingSessionReceiptStatus.stopped ||
  CodingSessionReceiptStatus.continuationRegistered ||
  CodingSessionReceiptStatus.turnQueued ||
  CodingSessionReceiptStatus.turnStarted ||
  CodingSessionReceiptStatus.modelApplied ||
  CodingSessionReceiptStatus.turnInjected => const _ReceiptErrorExpectation(
    mustHaveError: false,
  ),
  CodingSessionReceiptStatus.createdWithFailedInitialTurn =>
    const _ReceiptErrorExpectation(
      mustHaveError: true,
      code: 'INITIAL_TURN_FAILED',
    ),
  CodingSessionReceiptStatus.resumedWithoutContext =>
    const _ReceiptErrorExpectation(
      mustHaveError: true,
      code: 'CONTEXT_NOT_RECOVERED',
    ),
  CodingSessionReceiptStatus.failed ||
  CodingSessionReceiptStatus.turnDropped ||
  CodingSessionReceiptStatus.turnRefused ||
  // Delivery unknown always says why: the code is the whole answer.
  CodingSessionReceiptStatus.turnDeliveryUnknown =>
    const _ReceiptErrorExpectation(mustHaveError: true),
  CodingSessionReceiptStatus.turnDegraded ||
  CodingSessionReceiptStatus.interruptDelivered => null,
};
