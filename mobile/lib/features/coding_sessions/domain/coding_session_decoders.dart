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
const _maxErrorCodeBytes = 256;
const _maxReferenceBytes = 2 * 1024;
const _maxLabelBytes = 2 * 1024;
const _maxSummaryBytes = 16 * 1024;

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
  const summaries = ['contextSummary', 'diffSummary', 'planSummary'];
  // The B1 code-coordinate amendment: all four keys or none. They are
  // validated here so an amended payload is not read as corruption, then
  // dropped — this observer surfaces no code coordinates in v1.
  const facts = ['observedCommit', 'dirty', 'relayReachable', 'verifiedAt'];
  // Every additive amendment buzz-core has landed on this payload. `role`,
  // `turnBudget` and the 2026-08-30 `routing` record were all missing from
  // this list, so a seated, budgeted or routed session decoded as corruption
  // here and vanished from the observer — a forgotten amendment is a blank
  // session list, not a strictness nuance. This app surfaces none of the three
  // in v1; they are named so an amended payload is read rather than dropped.
  const optional = [
    ...summaries,
    'sessionRef',
    'role',
    'turnBudget',
    'routing',
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
  if (payload['routing'] != null &&
      !isStrictRoutingRecord(payload['routing'])) {
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
  for (final key in summaries) {
    if (payload.containsKey(key) &&
        !boundedNonempty(payload[key], _maxSummaryBytes)) {
      return const CodingSessionDecoded.failed(
        CodingSessionDecodeReason.malformedPayload,
      );
    }
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
      contextSummary: payload['contextSummary'] as String?,
      diffSummary: payload['diffSummary'] as String?,
      planSummary: payload['planSummary'] as String?,
    ),
  );
}

/// Decode a 44224 lifecycle or turn receipt.
///
/// Exactly five keys — `{schema, commandId, status, session, error}` — or six
/// with `turnId` when and only when the status is `turn_started`. An
/// unexpected key is a rejection, never a partial accept.
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
  final expected = status == CodingSessionReceiptStatus.turnStarted
      ? [...envelope, 'turnId']
      : envelope;
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
  final error = _decodeReceiptError(payload['error']);
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
  if (status == CodingSessionReceiptStatus.turnStarted) {
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
  if (!isPlainRecord(value)) return null;
  final record = value! as Map<String, dynamic>;
  if (!hasExactKeys(record, keys)) return null;
  final capabilities = <String, bool>{};
  for (final key in keys) {
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

CodingSessionReceiptError? _decodeReceiptError(Object? value) {
  if (!isPlainRecord(value)) return null;
  final record = value! as Map<String, dynamic>;
  if (!hasExactKeys(record, ['code', 'message']) ||
      !boundedNonempty(record['code'], _maxErrorCodeBytes) ||
      !boundedNonempty(record['message'], _maxReferenceBytes)) {
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
  CodingSessionReceiptStatus.turnQueued ||
  CodingSessionReceiptStatus.turnStarted => const _ReceiptErrorExpectation(
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
  CodingSessionReceiptStatus.turnRefused => const _ReceiptErrorExpectation(
    mustHaveError: true,
  ),
  CodingSessionReceiptStatus.turnDegraded ||
  CodingSessionReceiptStatus.interruptDelivered => null,
};
