import '../../../shared/relay/nostr_models.dart';
import 'coding_session_decode_result.dart';
import 'coding_session_keys.dart';
import 'coding_session_models.dart';
import 'coding_session_signature.dart';
import 'coding_session_target.dart';
import 'coding_session_title_wire.dart';
import 'coding_session_wire.dart';

/// Strict decoders for the umbrella-session kinds a *member* signs: the 44221
/// create that names a provider authority, the 44226 genesis that anchors an
/// umbrella, and the 44229/44227/44230 name, goal and closure facts — plus
/// the one provider-signed umbrella fact, the 44252 generated title.

/// Schema string on a 44221 payload.
const codingSessionLifecycleCommandSchema =
    'buzz-coding-session-lifecycle-command/v1';

/// Tag version on a 44221 event (`csl-v`).
const codingSessionLifecycleCommandTagVersion = 'csl1-1';

/// Tag version on a 44226 event (`csg-v`).
const codingSessionGenesisTagVersion = 'csg1-1';

/// Schema version integer inside a 44226 payload.
const codingSessionGenesisSchemaVersion = 1;

/// Tag version on a 44229 event (`csnm-v`).
const codingSessionNameTagVersion = 'csnm1-1';

/// Tag version on a 44227 event (`csgl-v`).
const codingSessionGoalTagVersion = 'csgl1-1';

/// Tag version on a 44230 event (`cscl-v`).
const codingSessionClosureTagVersion = 'cscl1-1';

/// Schema version integer inside a 44230 payload.
const codingSessionClosureSchemaVersion = 1;

const _maxLifecycleContentBytes = 16 * 1024;
const _maxIdentifierBytes = 256;
const _maxReferenceBytes = 2 * 1024;
const _maxGenesisContentBytes = 1024;
const _maxNameBytes = 256;
const _maxGoalBytes = 4096;
const _maxClosureContentBytes = 512;
const _maxInitialTurnBytes = 12 * 1024;

/// Decode a 44221 `session.create`.
///
/// Only creates decode here: a resume or stop names an existing execution and
/// binds no new authority, so it is [CodingSessionDecodeReason.wrongKind] for
/// this reader rather than a defect.
CodingSessionDecoded<CodingSessionCreate> decodeCodingSessionCreate(
  NostrEvent event, {
  CodingSessionSignatureVerifier? verifier,
}) {
  if (event.kind != EventKind.codingSessionLifecycleCommand) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  final tags = parseExactTags(event.tags, ['h', 'csl-v', 'csl-command']);
  if (tags == null ||
      tags[0].isEmpty ||
      tags[1] != codingSessionLifecycleCommandTagVersion) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final ref = _eventRef(event, tags[0]);
  if (ref == null) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final signature = _checkSignature(event, verifier);
  if (signature != null) return CodingSessionDecoded.failed(signature);

  final value = parseBoundedJson(event.content, _maxLifecycleContentBytes);
  if (!isPlainRecord(value)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final payload = value! as Map<String, dynamic>;
  if (!hasExactKeys(payload, ['schema', 'commandId', 'action']) ||
      payload['schema'] != codingSessionLifecycleCommandSchema ||
      !boundedNonempty(payload['commandId'], _maxIdentifierBytes) ||
      payload['commandId'] != tags[2] ||
      !isPlainRecord(payload['action'])) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final action = payload['action']! as Map<String, dynamic>;
  if (action['type'] != 'session.create') {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  final hasSessionRef = action.containsKey('sessionRef');
  final hasGenesisRef = action.containsKey('genesisRef');
  // An agent seat: `actor` and `role` travel together or not at all, in the
  // exact forms buzz-core accepts (lowercase 64-hex; `[a-z0-9-]` slug).
  final hasActor = action.containsKey('actor');
  if (hasActor != action.containsKey('role')) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  // The 2026-08-30 routing amendment: trailing, optional, and either the
  // closed record or corruption. An unrouted create keeps the exact key set
  // every reader before this already accepted.
  final hasRouting = action.containsKey('routing');
  if (hasRouting && !isStrictRoutingRecord(action['routing'])) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  // The 2026-09-01 attribution amendment: `hireRef` names the kind:44221
  // `session.hire` a seated create answers. Trailing and independent of the
  // seat pair and of routing, exactly as buzz-core enumerates it. It was
  // missing from `createKeys` until lane 216 — an exact-key list, so **every
  // hired seat's create** decoded as corruption here and bound no provider
  // authority, leaving the execution on D5's unverified metadata-signer
  // fallback ("authority unverified" in the session header). Absent is not
  // null: a key-set check sees an explicit null as *present*, so
  // `"hireRef": null` is the shape buzz-core refuses, not "no hire".
  final hasHireRef = action.containsKey('hireRef');
  if (hasHireRef && !isHex64(action['hireRef'])) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final createKeys = [
    'type',
    'projectRef',
    'repoRef',
    if (hasSessionRef) 'sessionRef',
    if (hasGenesisRef) 'genesisRef',
    'providerInstanceRef',
    'providerAuthorityPubkey',
    'model',
    'title',
    'initialTurn',
    if (hasActor) 'actor',
    if (hasActor) 'role',
    if (hasHireRef) 'hireRef',
    if (hasRouting) 'routing',
  ];
  if (!hasExactKeys(action, createKeys) || (hasGenesisRef && !hasSessionRef)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  if (hasActor &&
      (!isHex64(action['actor']) ||
          action['actor'] != normalizePubkey(action['actor']) ||
          !_isRoleSlug(action['role']))) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  // Absent is the historical 8-key form ("no umbrella claimed"); an explicit
  // null means the same thing on a new create. Anything present must be a
  // canonical UUID — a malformed claim is refused rather than coerced.
  final claimed = action['sessionRef'];
  if (claimed != null && !isCodingSessionSessionRef(claimed)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final genesisRef = action['genesisRef'];
  if (hasGenesisRef && (!isHex64(genesisRef) || claimed == null)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  // Refused, never repaired. `validate_provider_authority_pubkey` requires
  // lowercase 64-hex, and this id is compared byte-for-byte against signed
  // facts: a decoder that lowercases an uppercase key admits a create every
  // other reader calls malformed, and can be made to agree with a forgery
  // (ledger 216). `normalizePubkey` stays for values this app *reads off an
  // event*, not for values it validates.
  final authority = action['providerAuthorityPubkey'];
  if (!isHex64(authority) ||
      !boundedNonempty(action['providerInstanceRef'], _maxReferenceBytes) ||
      !boundedNullable(action['projectRef'], _maxReferenceBytes) ||
      !boundedNullable(action['repoRef'], _maxReferenceBytes) ||
      !boundedNullable(action['model'], _maxReferenceBytes) ||
      !boundedNullable(action['title'], _maxReferenceBytes) ||
      !boundedNullable(action['initialTurn'], _maxInitialTurnBytes)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  return CodingSessionDecoded.ok(
    CodingSessionCreate(
      ref: ref,
      commandId: payload['commandId'] as String,
      providerAuthorityPubkey: authority as String,
      providerInstanceRef: action['providerInstanceRef'] as String,
      sessionRef: claimed as String?,
      genesisRef: genesisRef as String?,
      projectRef: action['projectRef'] as String?,
      repoRef: action['repoRef'] as String?,
      model: action['model'] as String?,
      title: action['title'] as String?,
    ),
  );
}

/// Decode a 44221 `session.resume`.
///
/// Read for authority only: the resume names the provider that may answer it,
/// and that provider's lifecycle receipt names the generation the resume
/// minted. Without this a resumed generation falls through to D5's
/// first-seen-metadata-signer fallback, which hands a stranger the current
/// generation of somebody else's session. `session.stop` is still
/// [CodingSessionDecodeReason.wrongKind]: it binds no authority this reader
/// needs.
CodingSessionDecoded<CodingSessionResume> decodeCodingSessionResume(
  NostrEvent event, {
  CodingSessionSignatureVerifier? verifier,
}) {
  if (event.kind != EventKind.codingSessionLifecycleCommand) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  final tags = parseExactTags(event.tags, ['h', 'csl-v', 'csl-command']);
  if (tags == null ||
      tags[0].isEmpty ||
      tags[1] != codingSessionLifecycleCommandTagVersion) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final ref = _eventRef(event, tags[0]);
  if (ref == null) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final signature = _checkSignature(event, verifier);
  if (signature != null) return CodingSessionDecoded.failed(signature);

  final value = parseBoundedJson(event.content, _maxLifecycleContentBytes);
  if (!isPlainRecord(value)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final payload = value! as Map<String, dynamic>;
  if (!hasExactKeys(payload, ['schema', 'commandId', 'action']) ||
      payload['schema'] != codingSessionLifecycleCommandSchema ||
      !boundedNonempty(payload['commandId'], _maxIdentifierBytes) ||
      payload['commandId'] != tags[2] ||
      !isPlainRecord(payload['action'])) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final action = payload['action']! as Map<String, dynamic>;
  if (action['type'] != 'session.resume') {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  if (!hasExactKeys(action, ['type', 'session', 'providerAuthorityPubkey'])) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final session = CodingSessionTarget.decode(action['session']);
  // Refused, never repaired — same rule as the create above.
  final authority = action['providerAuthorityPubkey'];
  if (session == null || !isHex64(authority)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  return CodingSessionDecoded.ok(
    CodingSessionResume(
      ref: ref,
      commandId: payload['commandId'] as String,
      providerAuthorityPubkey: authority as String,
      session: session,
    ),
  );
}

/// Decode a 44226 genesis. Its signer is the umbrella's founder.
///
/// The genesis is resolved by *event id* from the create that names it; the
/// `csg-session` tag is a scoping aid, never a selector.
CodingSessionDecoded<CodingSessionGenesis> decodeCodingSessionGenesis(
  NostrEvent event, {
  CodingSessionSignatureVerifier? verifier,
}) {
  if (event.kind != EventKind.codingSessionGenesis) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  final tags = parseExactTags(event.tags, ['h', 'csg-v', 'csg-session']);
  if (tags == null ||
      tags[0].isEmpty ||
      tags[1] != codingSessionGenesisTagVersion) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final ref = _eventRef(event, tags[0]);
  if (ref == null) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final signature = _checkSignature(event, verifier);
  if (signature != null) return CodingSessionDecoded.failed(signature);

  final value = parseBoundedJson(event.content, _maxGenesisContentBytes);
  if (!isPlainRecord(value)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final payload = value! as Map<String, dynamic>;
  final hasAdopts = payload.containsKey('adopts');
  if (!hasExactKeys(
        payload,
        hasAdopts ? ['sessionRef', 'v', 'adopts'] : ['sessionRef', 'v'],
      ) ||
      payload['v'] != codingSessionGenesisSchemaVersion ||
      !isCodingSessionSessionRef(payload['sessionRef']) ||
      payload['sessionRef'] != tags[2]) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  if (hasAdopts && !_adoptsValid(payload['adopts'])) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  return CodingSessionDecoded.ok(
    CodingSessionGenesis(ref: ref, sessionRef: payload['sessionRef'] as String),
  );
}

/// Decode a 44229 umbrella-session display name: one line, at most 256 bytes.
CodingSessionDecoded<CodingSessionName> decodeCodingSessionName(
  NostrEvent event, {
  CodingSessionSignatureVerifier? verifier,
}) {
  if (event.kind != EventKind.codingSessionName) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  final decoded = _decodeAddressableText(
    event,
    versionTag: 'csnm-v',
    version: codingSessionNameTagVersion,
    maxBytes: _maxNameBytes,
    singleLine: true,
    verifier: verifier,
  );
  final failure = decoded.reason;
  if (failure != null) return CodingSessionDecoded.failed(failure);
  final record = decoded.value!;
  return CodingSessionDecoded.ok(
    CodingSessionName(
      ref: record.ref,
      sessionRef: record.sessionRef,
      content: record.content,
    ),
  );
}

/// Decode a 44252 provider-signed generated title (NIP-CSG § Generated title).
///
/// Exactly `h`, `d`, `cstl-v=cstl1-1`, `cs-target`, in that order, each with
/// two fields; strict v1 JSON content of at most 2048 bytes. Mirror of
/// `validate_coding_session_title_parts` in
/// `crates/buzz-core/src/coding_session_title.rs`, bound to it by
/// `conformance/session-display-name/` — a shape buzz-core refuses is refused
/// here, never repaired.
CodingSessionDecoded<CodingSessionGeneratedTitle>
decodeCodingSessionGeneratedTitle(
  NostrEvent event, {
  CodingSessionSignatureVerifier? verifier,
}) {
  if (event.kind != EventKind.codingSessionGeneratedTitle) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  final tags = parseExactTags(event.tags, ['h', 'd', 'cstl-v', 'cs-target']);
  if (tags == null ||
      !isUuidText(tags[0]) ||
      !isCodingSessionSessionRef(tags[1]) ||
      tags[2] != codingSessionTitleTagVersion ||
      !isCodingSessionTitleTargetKey(tags[3])) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final ref = _eventRef(event, tags[0]);
  if (ref == null) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final signature = _checkSignature(event, verifier);
  if (signature != null) return CodingSessionDecoded.failed(signature);

  final value = parseBoundedJson(
    event.content,
    maxCodingSessionTitleContentBytes,
  );
  if (!isPlainRecord(value)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final payload = value! as Map<String, dynamic>;
  // `sourceCommand` is required and nullable: absent is a different shape
  // from null, exactly as buzz-core's `deserialize_required_nullable` reads it.
  final title = payload['title'];
  final model = payload['model'];
  final source = payload['sourceCommand'];
  if (!hasExactKeys(payload, const [
        'schema',
        'title',
        'model',
        'basis',
        'sourceCommand',
        'createEventId',
      ]) ||
      payload['schema'] != codingSessionTitleSchema ||
      !isCodingSessionNameText(title) ||
      model is! String ||
      model.trim().isEmpty ||
      utf8ByteLength(model) > maxCodingSessionTitleModelBytes ||
      hasControlCharacter(model) ||
      payload['basis'] != codingSessionTitleBasisFirstMessage ||
      (source != null && !isHex64(source)) ||
      !isHex64(payload['createEventId'])) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  return CodingSessionDecoded.ok(
    CodingSessionGeneratedTitle(
      ref: ref,
      sessionRef: tags[1],
      targetKey: tags[3],
      title: title! as String,
      model: model,
      sourceCommand: source as String?,
      createEventId: payload['createEventId'] as String,
    ),
  );
}

/// Decode a 44227 umbrella-session goal: at most 4096 bytes.
CodingSessionDecoded<CodingSessionGoal> decodeCodingSessionGoal(
  NostrEvent event, {
  CodingSessionSignatureVerifier? verifier,
}) {
  if (event.kind != EventKind.codingSessionGoal) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  final decoded = _decodeAddressableText(
    event,
    versionTag: 'csgl-v',
    version: codingSessionGoalTagVersion,
    maxBytes: _maxGoalBytes,
    singleLine: false,
    verifier: verifier,
  );
  final failure = decoded.reason;
  if (failure != null) return CodingSessionDecoded.failed(failure);
  final record = decoded.value!;
  return CodingSessionDecoded.ok(
    CodingSessionGoal(
      ref: record.ref,
      sessionRef: record.sessionRef,
      content: record.content,
    ),
  );
}

/// Decode a 44230 closure marker.
CodingSessionDecoded<CodingSessionClosure> decodeCodingSessionClosure(
  NostrEvent event, {
  CodingSessionSignatureVerifier? verifier,
}) {
  if (event.kind != EventKind.codingSessionClosure) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.wrongKind,
    );
  }
  final tags = parseExactTags(event.tags, ['h', 'd', 'cscl-v', 'cscl-genesis']);
  if (tags == null ||
      tags[0].isEmpty ||
      !isCodingSessionSessionRef(tags[1]) ||
      tags[2] != codingSessionClosureTagVersion ||
      !isHex64(tags[3])) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final ref = _eventRef(event, tags[0]);
  if (ref == null) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final signature = _checkSignature(event, verifier);
  if (signature != null) return CodingSessionDecoded.failed(signature);

  final value = parseBoundedJson(event.content, _maxClosureContentBytes);
  if (!isPlainRecord(value)) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  final payload = value! as Map<String, dynamic>;
  // Three actions, not two: `CodingSessionClosureAction` is a three-variant
  // Rust enum and `archived` is the third — finished *and* filed away, which
  // `is_closed()` answers exactly as a `closed` does. It was missing here
  // until lane 216, so every archived umbrella decoded as corruption. This
  // app does not yet distinguish archived from closed in its model; it reads
  // the settled fact rather than dropping the event.
  final action = payload['action'];
  if (!hasExactKeys(payload, ['action', 'genesisRef', 'sessionRef', 'v']) ||
      (action != 'closed' && action != 'open' && action != 'archived') ||
      payload['genesisRef'] != tags[3] ||
      payload['sessionRef'] != tags[1] ||
      payload['v'] != codingSessionClosureSchemaVersion) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  return CodingSessionDecoded.ok(
    CodingSessionClosure(
      ref: ref,
      sessionRef: tags[1],
      genesisRef: tags[3],
      closed: action != 'open',
    ),
  );
}

class _AddressableText {
  const _AddressableText({
    required this.ref,
    required this.sessionRef,
    required this.content,
  });

  final CodingSessionEventRef ref;
  final String sessionRef;
  final String content;
}

CodingSessionDecoded<_AddressableText> _decodeAddressableText(
  NostrEvent event, {
  required String versionTag,
  required String version,
  required int maxBytes,
  required bool singleLine,
  required CodingSessionSignatureVerifier? verifier,
}) {
  final tags = parseExactTags(event.tags, ['h', 'd', versionTag]);
  if (tags == null ||
      tags[0].isEmpty ||
      !isCodingSessionSessionRef(tags[1]) ||
      tags[2] != version) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final ref = _eventRef(event, tags[0]);
  if (ref == null) {
    return const CodingSessionDecoded.failed(CodingSessionDecodeReason.badTags);
  }
  final signature = _checkSignature(event, verifier);
  if (signature != null) return CodingSessionDecoded.failed(signature);

  final content = event.content;
  if (!boundedNonempty(content, maxBytes) ||
      (singleLine && (content.contains('\n') || content.contains('\r')))) {
    return const CodingSessionDecoded.failed(
      CodingSessionDecodeReason.malformedPayload,
    );
  }
  return CodingSessionDecoded.ok(
    _AddressableText(ref: ref, sessionRef: tags[1], content: content),
  );
}

bool _adoptsValid(Object? value) {
  if (!isPlainRecord(value)) return false;
  final record = value! as Map<String, dynamic>;
  return hasExactKeys(record, ['createEventId', 'receiptEventId']) &&
      isHex64(record['createEventId']) &&
      isHex64(record['receiptEventId']);
}

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

final RegExp _roleSlugPattern = RegExp(r'^[a-z0-9-]{1,64}$');

/// A role slug exactly as buzz-core accepts it.
bool _isRoleSlug(Object? value) =>
    value is String && _roleSlugPattern.hasMatch(value);
