import 'package:flutter/foundation.dart';

import '../../../shared/relay/nostr_models.dart';
import 'coding_session_decode_result.dart';
import 'coding_session_decoders.dart';
import 'coding_session_models.dart';
import 'coding_session_session_decoders.dart';
import 'coding_session_signature.dart';
import 'coding_session_wire.dart';

/// Who may speak for one execution target.
@immutable
class CodingSessionAuthority {
  /// The provider pubkey whose 44223/44224/44225 are accepted for the target.
  final String pubkey;

  /// True when a receipt-joined 44221 create named [pubkey].
  ///
  /// False means this is the fallback: the first-seen metadata signer for the
  /// target, used when no create is readable. The session page must mark such
  /// an execution "authority unverified" — the reader is being shown facts
  /// nobody signed a mandate for.
  final bool verified;

  const CodingSessionAuthority({required this.pubkey, required this.verified});

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is CodingSessionAuthority &&
          pubkey == other.pubkey &&
          verified == other.verified;

  @override
  int get hashCode => Object.hash(pubkey, verified);
}

/// What the read had to throw away, and why.
///
/// These are shown, not swallowed: a transcript missing three items because
/// two providers contradicted each other is a different thing from a
/// transcript that is simply short.
@immutable
class CodingSessionReadCounts {
  /// Events of a coding-session kind that failed their own strict decoder.
  final int malformed;

  /// Correctly shaped facts signed by someone with no authority over them.
  final int rejectedAuthor;

  /// Distinct payloads that collided on one identity and so render neither.
  final int conflicts;

  /// Events whose signature or event id failed to verify.
  final int invalidSignature;

  /// Repeats of an event id already seen; collapsed, never counted as facts.
  final int duplicates;

  const CodingSessionReadCounts({
    this.malformed = 0,
    this.rejectedAuthor = 0,
    this.conflicts = 0,
    this.invalidSignature = 0,
    this.duplicates = 0,
  });

  /// True when nothing was dropped — the only case a UI may stay silent.
  bool get isClean =>
      malformed == 0 &&
      rejectedAuthor == 0 &&
      conflicts == 0 &&
      invalidSignature == 0;
}

/// Every fact that survived the trust gate, plus what did not.
@immutable
class CodingSessionTrustedFacts {
  final String channelId;

  /// Accepted 44223 metadata, in relay order.
  final List<CodingSessionMetadata> metadata;

  /// Accepted 44224 receipts, lifecycle and turn stages alike.
  final List<CodingSessionReceipt> receipts;

  /// Accepted 44225 transcript envelopes.
  final List<CodingSessionTranscriptEnvelope> transcripts;

  /// Decoded 44221 creates (signed by members, not by providers).
  final List<CodingSessionCreate> creates;

  /// Decoded 44226 geneses, keyed by event id — the only way a create names
  /// one.
  final Map<String, CodingSessionGenesis> genesesByEventId;

  final List<CodingSessionName> names;
  final List<CodingSessionGoal> goals;
  final List<CodingSessionClosure> closures;
  final List<CodingSessionLease> leases;

  /// Resolved authority per `cs-target` key.
  final Map<String, CodingSessionAuthority> authorityByTarget;

  /// The execution target each create minted, per its named provider.
  final Map<String, String> targetKeyByCommandId;

  final CodingSessionReadCounts counts;

  /// False when no signature verification API was reachable on this device.
  ///
  /// The session page shows one persistent
  /// "Signatures not verified on this device" line when this is false. It is
  /// never silently ignored.
  final bool signaturesVerified;

  const CodingSessionTrustedFacts({
    required this.channelId,
    required this.metadata,
    required this.receipts,
    required this.transcripts,
    required this.creates,
    required this.genesesByEventId,
    required this.names,
    required this.goals,
    required this.closures,
    required this.leases,
    required this.authorityByTarget,
    required this.targetKeyByCommandId,
    required this.counts,
    required this.signaturesVerified,
  });
}

/// Decode a channel's raw coding-session events and keep only what is
/// attributable.
///
/// The rules, in order:
///
/// 1. Byte-identical duplicates collapse by event id.
/// 2. Every event runs its own strict decoder; failures are counted, never
///    guessed at.
/// 3. A 44221 create names the one provider whose facts may answer it. A
///    create is joined to an execution only by a *lifecycle* receipt from
///    that exact signer — never by a turn receipt, which reports a turn stage
///    and creates nothing.
/// 4. A target with no readable create falls back to its first-seen metadata
///    signer, marked [CodingSessionAuthority.verified] `== false`.
/// 5. Facts from any other signer are rejected. Facts from different signers
///    never merge.
/// 6. Two distinct transcript payloads at one `(target, signer, eventSeq)`
///    render neither and count a conflict.
CodingSessionTrustedFacts applyCodingSessionTrustGate({
  required String channelId,
  required Iterable<NostrEvent> events,
  CodingSessionSignatureVerifier? verifier,
}) {
  final seenEventIds = <String>{};
  var duplicates = 0;
  var malformed = 0;
  var invalidSignature = 0;
  var conflicts = 0;

  final metadata = <CodingSessionMetadata>[];
  final receipts = <CodingSessionReceipt>[];
  final transcripts = <CodingSessionTranscriptEnvelope>[];
  final creates = <CodingSessionCreate>[];
  final resumes = <CodingSessionResume>[];
  final geneses = <String, CodingSessionGenesis>{};
  final names = <CodingSessionName>[];
  final goals = <CodingSessionGoal>[];
  final closures = <CodingSessionClosure>[];
  final leases = <CodingSessionLease>[];

  void count(CodingSessionDecodeOutcome outcome) {
    switch (outcome) {
      case CodingSessionDecodeOutcome.malformed:
        malformed += 1;
      case CodingSessionDecodeOutcome.invalidSignature:
        invalidSignature += 1;
      case CodingSessionDecodeOutcome.accepted:
      case CodingSessionDecodeOutcome.irrelevant:
        break;
    }
  }

  for (final event in events) {
    if (event.channelId != channelId) continue;
    if (!seenEventIds.add(event.id)) {
      duplicates += 1;
      continue;
    }
    switch (event.kind) {
      case EventKind.codingSessionMetadata:
        final decoded = decodeCodingSessionMetadata(event, verifier: verifier);
        count(_outcomeOf(decoded.reason));
        if (decoded.value != null) metadata.add(decoded.value!);
      case EventKind.codingSessionLifecycleReceipt:
        final decoded = decodeCodingSessionReceipt(event, verifier: verifier);
        count(_outcomeOf(decoded.reason));
        if (decoded.value != null) receipts.add(decoded.value!);
      case EventKind.codingSessionTranscript:
        final decoded = decodeCodingSessionTranscript(
          event,
          verifier: verifier,
        );
        count(_outcomeOf(decoded.reason));
        if (decoded.value != null) transcripts.add(decoded.value!);
      case EventKind.codingSessionLifecycleCommand:
        // One kind, two commands this reader cares about. A create binds the
        // provider for the generation it opens; a resume binds it for the
        // generation the provider mints in answer. `session.stop` decodes as
        // neither and is counted as irrelevant, not as corruption.
        final decoded = decodeCodingSessionCreate(event, verifier: verifier);
        if (decoded.value != null) {
          count(_outcomeOf(decoded.reason));
          creates.add(decoded.value!);
          break;
        }
        if (decoded.reason != CodingSessionDecodeReason.wrongKind) {
          count(_outcomeOf(decoded.reason));
          break;
        }
        final resume = decodeCodingSessionResume(event, verifier: verifier);
        count(_outcomeOf(resume.reason));
        if (resume.value != null) resumes.add(resume.value!);
      case EventKind.codingSessionGenesis:
        final decoded = decodeCodingSessionGenesis(event, verifier: verifier);
        count(_outcomeOf(decoded.reason));
        if (decoded.value != null) {
          geneses[decoded.value!.ref.eventId] = decoded.value!;
        }
      case EventKind.codingSessionName:
        final decoded = decodeCodingSessionName(event, verifier: verifier);
        count(_outcomeOf(decoded.reason));
        if (decoded.value != null) names.add(decoded.value!);
      case EventKind.codingSessionGoal:
        final decoded = decodeCodingSessionGoal(event, verifier: verifier);
        count(_outcomeOf(decoded.reason));
        if (decoded.value != null) goals.add(decoded.value!);
      case EventKind.codingSessionClosure:
        final decoded = decodeCodingSessionClosure(event, verifier: verifier);
        count(_outcomeOf(decoded.reason));
        if (decoded.value != null) closures.add(decoded.value!);
      case EventKind.codingSessionLease:
        final decoded = decodeCodingSessionLease(event, verifier: verifier);
        count(_outcomeOf(decoded.reason));
        if (decoded.value != null) leases.add(decoded.value!);
    }
  }

  final joins = _joinCreates(creates, resumes, receipts);
  conflicts += joins.conflicts;

  final authorityByTarget = <String, CodingSessionAuthority>{};
  for (final entry in joins.authorityByTarget.entries) {
    authorityByTarget[entry.key] = CodingSessionAuthority(
      pubkey: entry.value,
      verified: true,
    );
  }
  // Fallback authority: the first-seen metadata signer for a target no
  // readable create claims. Deterministic, so two devices reading the same
  // events agree on which signer that is.
  //
  // A *disputed* target is excluded. D5 allows this fallback only when no
  // create is readable; applying it to a target two receipt-joined creates
  // claim for different providers would hand the execution to whoever timed
  // their metadata earliest, which is a forgery anyone in the channel can
  // publish. A contested execution renders no provider facts at all, and the
  // conflict is already counted for the reader.
  final orderedMetadata = [...metadata]
    ..sort((left, right) => _byCreatedAtThenEventId(left.ref, right.ref));
  for (final record in orderedMetadata) {
    if (joins.disputedTargets.contains(record.target.key)) continue;
    authorityByTarget.putIfAbsent(
      record.target.key,
      () => CodingSessionAuthority(
        pubkey: record.ref.signerPubkey,
        verified: false,
      ),
    );
  }

  var rejectedAuthor = 0;
  bool authorized(String targetKey, String signerPubkey) {
    final authority = authorityByTarget[targetKey];
    if (authority == null || authority.pubkey != signerPubkey) {
      rejectedAuthor += 1;
      return false;
    }
    return true;
  }

  final acceptedMetadata = [
    for (final record in metadata)
      if (authorized(record.target.key, record.ref.signerPubkey)) record,
  ];
  final acceptedReceipts = [
    for (final record in receipts)
      if (_receiptAuthorized(record, joins, authorized)) record,
  ];
  final candidateTranscripts = [
    for (final record in transcripts)
      if (authorized(record.target.key, record.ref.signerPubkey)) record,
  ];
  final resolved = _resolveTranscriptConflicts(candidateTranscripts);
  conflicts += resolved.conflicts;

  return CodingSessionTrustedFacts(
    channelId: channelId,
    metadata: List.unmodifiable(acceptedMetadata),
    receipts: List.unmodifiable(acceptedReceipts),
    transcripts: List.unmodifiable(resolved.envelopes),
    creates: List.unmodifiable(creates),
    genesesByEventId: Map.unmodifiable(geneses),
    names: List.unmodifiable(names),
    goals: List.unmodifiable(goals),
    closures: List.unmodifiable(closures),
    leases: List.unmodifiable(leases),
    authorityByTarget: Map.unmodifiable(authorityByTarget),
    targetKeyByCommandId: Map.unmodifiable(joins.targetKeyByCommandId),
    counts: CodingSessionReadCounts(
      malformed: malformed,
      rejectedAuthor: rejectedAuthor,
      conflicts: conflicts,
      invalidSignature: invalidSignature,
      duplicates: duplicates,
    ),
    signaturesVerified: verifier?.available ?? false,
  );
}

/// Coarse disposition of one decode, for counting.
enum CodingSessionDecodeOutcome {
  accepted,
  irrelevant,
  malformed,
  invalidSignature,
}

CodingSessionDecodeOutcome _outcomeOf(CodingSessionDecodeReason? reason) =>
    switch (reason) {
      null => CodingSessionDecodeOutcome.accepted,
      CodingSessionDecodeReason.wrongKind =>
        CodingSessionDecodeOutcome.irrelevant,
      CodingSessionDecodeReason.badSignature =>
        CodingSessionDecodeOutcome.invalidSignature,
      CodingSessionDecodeReason.badTags ||
      CodingSessionDecodeReason.malformedPayload =>
        CodingSessionDecodeOutcome.malformed,
    };

class _CreateJoins {
  const _CreateJoins({
    required this.authorityByTarget,
    required this.targetKeyByCommandId,
    required this.authorityByCommandId,
    required this.disputedTargets,
    required this.conflicts,
  });

  /// `cs-target` key -> the provider pubkey a create or resume named for it.
  final Map<String, String> authorityByTarget;

  /// Targets two receipt-joined commands claimed for different providers.
  ///
  /// Carried out of the join because the D5 fallback must skip them: a
  /// disputed target is a contradiction between readable creates, not the
  /// "no create is readable" case the fallback exists for.
  final Set<String> disputedTargets;

  /// commandId -> the `cs-target` key its receipts settled on.
  final Map<String, String> targetKeyByCommandId;

  /// commandId -> the provider pubkey the command addressed.
  final Map<String, String> authorityByCommandId;

  final int conflicts;
}

/// One lifecycle command that names a provider, create or resume alike.
///
/// The join below cares about four things — who signed the command, which
/// provider it addressed, which umbrella it claimed and which genesis it
/// anchored to — and a resume simply claims neither of the last two.
class _AuthorityClaim {
  const _AuthorityClaim({
    required this.ref,
    required this.commandId,
    required this.providerAuthorityPubkey,
    required this.sessionRef,
    required this.genesisRef,
  });

  _AuthorityClaim.ofCreate(CodingSessionCreate create)
    : ref = create.ref,
      commandId = create.commandId,
      providerAuthorityPubkey = create.providerAuthorityPubkey,
      sessionRef = create.sessionRef,
      genesisRef = create.genesisRef;

  _AuthorityClaim.ofResume(CodingSessionResume resume)
    : ref = resume.ref,
      commandId = resume.commandId,
      providerAuthorityPubkey = resume.providerAuthorityPubkey,
      sessionRef = null,
      genesisRef = null;

  final CodingSessionEventRef ref;
  final String commandId;
  final String providerAuthorityPubkey;
  final String? sessionRef;
  final String? genesisRef;
}

_CreateJoins _joinCreates(
  List<CodingSessionCreate> creates,
  List<CodingSessionResume> resumes,
  List<CodingSessionReceipt> receipts,
) {
  var conflicts = 0;
  final byCommandId = <String, List<_AuthorityClaim>>{};
  for (final claim in [
    ...creates.map(_AuthorityClaim.ofCreate),
    ...resumes.map(_AuthorityClaim.ofResume),
  ]) {
    byCommandId.putIfAbsent(claim.commandId, () => []).add(claim);
  }
  final authorityByCommandId = <String, String>{};
  final targetKeyByCommandId = <String, String>{};
  final authorityByTarget = <String, String>{};
  final disputedTargets = <String>{};

  for (final entry in byCommandId.entries) {
    final records = [...entry.value]
      ..sort((left, right) => _byCreatedAtThenEventId(left.ref, right.ref));
    // One commandId, one signer, one umbrella claim, one addressed provider.
    // Anything else is a disputed command, and a disputed command binds
    // nothing.
    final disputed =
        records.map((record) => record.ref.signerPubkey).toSet().length > 1 ||
        records.map((record) => record.sessionRef).toSet().length > 1 ||
        records.map((record) => record.genesisRef).toSet().length > 1 ||
        records.map((record) => record.providerAuthorityPubkey).toSet().length >
            1;
    if (disputed) {
      conflicts += 1;
      continue;
    }
    final claim = records.first;
    authorityByCommandId[claim.commandId] = claim.providerAuthorityPubkey;

    // Only *lifecycle* receipts from the command's own named provider settle
    // which execution it minted. A turn receipt names the execution it was
    // addressed to, but joining one here would let a refused turn decide
    // which generation a session opened into.
    final answers = <String>{};
    for (final receipt in receipts) {
      if (receipt.commandId != claim.commandId) continue;
      if (receipt.ref.signerPubkey != claim.providerAuthorityPubkey) continue;
      if (receipt.isTurnStage) continue;
      final target = receipt.session;
      if (target == null) continue;
      answers.add(target.key);
    }
    if (answers.length != 1) {
      if (answers.length > 1) conflicts += 1;
      continue;
    }
    final targetKey = answers.single;
    targetKeyByCommandId[claim.commandId] = targetKey;
    final incumbent = authorityByTarget[targetKey];
    if (incumbent != null && incumbent != claim.providerAuthorityPubkey) {
      // Two commands claiming one execution for different providers. Neither
      // wins: the target falls through to the unverified fallback rather than
      // letting the earlier one pick the winner silently.
      conflicts += 1;
      disputedTargets.add(targetKey);
      continue;
    }
    authorityByTarget[targetKey] = claim.providerAuthorityPubkey;
  }
  for (final targetKey in disputedTargets) {
    authorityByTarget.remove(targetKey);
  }
  return _CreateJoins(
    authorityByTarget: authorityByTarget,
    targetKeyByCommandId: targetKeyByCommandId,
    authorityByCommandId: authorityByCommandId,
    disputedTargets: disputedTargets,
    conflicts: conflicts,
  );
}

/// A `failed` receipt names no execution, so it is attributed by the command
/// it answers rather than by a target. Every other receipt is attributed the
/// same way every other fact is.
bool _receiptAuthorized(
  CodingSessionReceipt receipt,
  _CreateJoins joins,
  bool Function(String targetKey, String signerPubkey) authorized,
) {
  final target = receipt.session;
  if (target != null) {
    return authorized(target.key, receipt.ref.signerPubkey);
  }
  return joins.authorityByCommandId[receipt.commandId] ==
      receipt.ref.signerPubkey;
}

class _ResolvedTranscripts {
  const _ResolvedTranscripts({
    required this.envelopes,
    required this.conflicts,
  });

  final List<CodingSessionTranscriptEnvelope> envelopes;
  final int conflicts;
}

_ResolvedTranscripts _resolveTranscriptConflicts(
  List<CodingSessionTranscriptEnvelope> envelopes,
) {
  String bucketKey(CodingSessionTranscriptEnvelope envelope) =>
      '${envelope.target.key}\u0000'
      '${envelope.ref.signerPubkey}\u0000'
      '${envelope.eventSeq}';

  final buckets = <String, List<CodingSessionTranscriptEnvelope>>{};
  for (final envelope in envelopes) {
    buckets.putIfAbsent(bucketKey(envelope), () => []).add(envelope);
  }
  var conflicts = 0;
  final conflicted = <String>{};
  for (final entry in buckets.entries) {
    final payloads = entry.value
        .map(
          (envelope) => canonicalJson({
            'timestamp': envelope.timestamp,
            'turnId': envelope.turnId,
            'item': envelope.item,
          }),
        )
        .toSet();
    if (payloads.length > 1) {
      // Two providers, or one provider twice, telling different stories at one
      // sequence. Rendering either would be picking a side; rendering both
      // would be inventing an ordering. Neither renders.
      conflicts += 1;
      conflicted.add(entry.key);
    }
  }
  final emitted = <String>{};
  final result = <CodingSessionTranscriptEnvelope>[];
  for (final envelope in envelopes) {
    final key = bucketKey(envelope);
    if (conflicted.contains(key)) continue;
    // Same facts republished under a second event id: collapse rather than
    // render the item twice.
    if (!emitted.add(key)) continue;
    result.add(envelope);
  }
  return _ResolvedTranscripts(envelopes: result, conflicts: conflicts);
}

int _byCreatedAtThenEventId(
  CodingSessionEventRef left,
  CodingSessionEventRef right,
) {
  final byTime = left.createdAt.compareTo(right.createdAt);
  return byTime != 0 ? byTime : left.eventId.compareTo(right.eventId);
}
