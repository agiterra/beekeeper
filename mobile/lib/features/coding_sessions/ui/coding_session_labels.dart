import '../../../shared/relay/nostr_models.dart';
import '../domain/coding_sessions_domain.dart';

/// The line shown when this device could not verify signatures (D5).
const codingSessionUnverifiedSignaturesLabel =
    'Signatures not verified on this device';

/// Why the composer is missing, per [CodingSessionSteerStanding].
///
/// Each sentence claims only what this device read. It folds founders but not
/// 44228 operator grants, so "not the founder" is stated as exactly that, and
/// the relay — which does read grants — is named as the authority that may
/// still admit a command.
String codingSessionSteerDisclosure(CodingSessionSteerStanding standing) =>
    switch (standing) {
      CodingSessionSteerStanding.founder ||
      CodingSessionSteerStanding.acceptedOperator => '',
      CodingSessionSteerStanding.noKey =>
        'This device holds no signing key, so it can only observe.',
      CodingSessionSteerStanding.notFounder =>
        'Only the session\'s founder or a granted operator can steer it. '
            'This device is not the founder, and it does not read operator '
            'grants — the relay would still admit a granted key.',
      CodingSessionSteerStanding.founderUnresolved =>
        'The session\'s founder could not be resolved from what this device '
            'read, so it does not offer to steer.',
    };

/// The composer line when no current generation is live enough to address.
const codingSessionNoLiveExecutionLabel =
    'No live execution to send to — every generation is stopped, failed or '
    'disconnected.';

/// What a pending turn row says for each phase (D4: settled by receipt, never
/// by text).
String codingSessionPendingPhaseLabel(CodingSessionPendingTurnView view) =>
    switch (view.phase) {
      CodingSessionPendingPhase.sending => 'Sending…',
      CodingSessionPendingPhase.published =>
        'Sent · waiting for the provider\'s receipt',
      CodingSessionPendingPhase.queued => 'Queued for the next turn',
      CodingSessionPendingPhase.degraded =>
        'Delivery downgraded${view.detail == null ? '' : ' — ${view.detail}'}',
      CodingSessionPendingPhase.started => 'Started',
      CodingSessionPendingPhase.refused =>
        'Refused${view.detail == null ? '' : ' — ${view.detail}'}',
      CodingSessionPendingPhase.dropped =>
        'Dropped${view.detail == null ? '' : ' — ${view.detail}'}',
    };

/// The primary send label, per the desktop's `codingSessionComposerModel`.
///
/// "Steer" is the promise the button makes, so it is offered only when the
/// execution advertised native steering; otherwise a mid-turn send runs at
/// the next boundary, which is "Send next".
String codingSessionSendLabel({
  required bool isWorking,
  required bool canSteer,
}) => !isWorking ? 'Send' : (canSteer ? 'Steer' : 'Send next');

/// The empty state for a channel with no readable coding sessions.
const codingSessionsEmptyLabel = 'No coding sessions in this channel';

/// What a full history page means for the transcript on screen.
const codingSessionTruncatedLabel =
    'History truncated at 1000 events — older activity was not fetched';

/// What this device's own retention cap cost the reader (D10).
///
/// Separate from [codingSessionTruncatedLabel]: that one is history the relay
/// never sent, this one is history it sent and this device threw away to stay
/// bounded. Saying neither would leave a silently shortened transcript looking
/// like a short one.
const codingSessionEvictedLabel =
    'Older events were dropped on this device (kept the newest '
    '$maxCodingSessionEventsPerGeneration)';

/// The chip text for a folded umbrella status (D8).
String codingSessionStatusLabel(CodingSessionFoldedStatus status) =>
    switch (status.kind) {
      CodingSessionFoldedStatusKind.working => 'Working',
      CodingSessionFoldedStatusKind.waiting => 'Waiting',
      CodingSessionFoldedStatusKind.ended => 'Ended',
      CodingSessionFoldedStatusKind.reported => codingSessionStatusWords(
        status.status,
      ),
      CodingSessionFoldedStatusKind.unknown => 'Unknown',
    };

/// A reported status in words, e.g. `waiting_for_input` as `waiting for
/// input`. Never a status the provider did not sign.
String codingSessionStatusWords(CodingSessionStatus? status) {
  if (status == null) return 'unknown';
  return status.wire.replaceAll('_', ' ');
}

/// The same, capitalised for a chip.
String codingSessionStatusTitle(CodingSessionStatus? status) {
  final words = codingSessionStatusWords(status);
  return words.isEmpty
      ? words
      : '${words[0].toUpperCase()}${words.substring(1)}';
}

/// How a founder resolved, in the reader's terms (D7).
String codingSessionFounderLabel(CodingSessionFounder founder) =>
    switch (founder.resolution) {
      CodingSessionFounderResolution.genesis =>
        'Founder ${_short(founder.pubkey)}',
      CodingSessionFounderResolution.legacy =>
        'Founder ${_short(founder.pubkey)} (legacy)',
      CodingSessionFounderResolution.conflict => 'Founder conflict',
      CodingSessionFounderResolution.unresolved => 'Founder unresolved',
    };

/// A bounded age phrase: `just now`, `3m ago`, `2h ago`, `4d ago`.
String codingSessionAgo(Duration age) {
  final seconds = age.inSeconds;
  if (seconds < 60) return 'just now';
  if (age.inMinutes < 60) return '${age.inMinutes}m ago';
  if (age.inHours < 24) return '${age.inHours}h ago';
  return '${age.inDays}d ago';
}

/// The same phrase for an epoch-seconds timestamp, measured against [now].
String codingSessionAgoSince(int? epochSeconds, DateTime now) {
  if (epochSeconds == null || epochSeconds <= 0) return 'unknown';
  final elapsed = now.millisecondsSinceEpoch ~/ 1000 - epochSeconds;
  return codingSessionAgo(Duration(seconds: elapsed < 0 ? 0 : elapsed));
}

/// The reachability line for a session header (D8).
///
/// Returns `null` when there is nothing honest to say. A verdict of
/// [CodingSessionReachabilityKind.unknown] never reads as a denial: not having
/// asked is not the same as nobody answering.
String? codingSessionReachabilityLabel({
  required CodingSessionReachability reachability,
  required CodingSessionFoldedStatus status,
  required int? statusAt,
  required DateTime now,
}) {
  switch (reachability.kind) {
    case CodingSessionReachabilityKind.reachable:
      return 'Provider reachable';
    case CodingSessionReachabilityKind.unknown:
      return 'Provider reachability unknown';
    case CodingSessionReachabilityKind.noProviderAnswering:
      final reported = status.status;
      if (reported != null && reported.isLiveSounding) {
        return 'No provider answering · last reported '
            '${codingSessionStatusWords(reported)} '
            '${codingSessionAgoSince(statusAt, now)}';
      }
      return 'No provider answering';
  }
}

/// A one-line account of what the read had to throw away.
///
/// Returns `null` when nothing was dropped — the only case the UI may stay
/// silent about its own losses.
String? codingSessionCountsLabel(CodingSessionReadCounts counts) {
  final parts = <String>[
    if (counts.malformed > 0) '${counts.malformed} malformed',
    if (counts.rejectedAuthor > 0) '${counts.rejectedAuthor} wrong signer',
    if (counts.invalidSignature > 0) '${counts.invalidSignature} bad signature',
    if (counts.conflicts > 0) '${counts.conflicts} conflicting',
  ];
  if (parts.isEmpty) return null;
  return 'Dropped from this read: ${parts.join(', ')}';
}

/// One coding-session kind in the reader's words.
///
/// Never a bare integer where a word exists: "3 transcript rows refused" is
/// something a reader can act on, "3 kind 44225 refused" is not.
String codingSessionKindLabel(int kind) => switch (kind) {
  EventKind.codingSessionMetadata => 'status',
  EventKind.codingSessionLifecycleReceipt => 'receipt',
  EventKind.codingSessionTranscript => 'transcript',
  EventKind.codingSessionLifecycleCommand => 'command',
  EventKind.codingSessionGenesis => 'genesis',
  EventKind.codingSessionName => 'name',
  EventKind.codingSessionGoal => 'goal',
  EventKind.codingSessionClosure => 'closure',
  EventKind.codingSessionLease => 'lease',
  _ => 'kind $kind',
};

/// The same losses as [codingSessionCountsLabel], named per kind and reason.
///
/// Returns `null` when nothing was refused. The summary line says how much a
/// read cost; this says what it cost and why — a refused transcript envelope
/// is a hole in what is on screen, a refused name is a session that may be
/// wearing the wrong one, and "invalid signature" and "unauthorized signer"
/// are different accusations against different parties.
String? codingSessionRefusedByKindLabel(CodingSessionReadCounts counts) {
  final kinds = <int>{
    ...counts.malformedByKind.keys,
    ...counts.rejectedAuthorByKind.keys,
    ...counts.invalidSignatureByKind.keys,
  }.toList()..sort();
  final parts = <String>[];
  for (final kind in kinds) {
    final malformed = counts.malformedByKind[kind] ?? 0;
    final rejected = counts.rejectedAuthorByKind[kind] ?? 0;
    final invalid = counts.invalidSignatureByKind[kind] ?? 0;
    final reasons = <String>[
      if (malformed > 0) '$malformed malformed',
      if (rejected > 0) '$rejected wrong signer',
      if (invalid > 0) '$invalid bad signature',
    ];
    if (reasons.isEmpty) continue;
    parts.add('${codingSessionKindLabel(kind)} ${reasons.join(', ')}');
  }
  if (parts.isEmpty) return null;
  return 'By kind: ${parts.join('; ')}';
}

/// `2 executions · claude-code · sonnet`, built only from signed fields.
String codingSessionExecutionsLabel(List<CodingSessionExecution> executions) {
  final count = executions.length;
  final noun = count == 1 ? '1 execution' : '$count executions';
  final labels = <String>[];
  for (final execution in executions) {
    final label = execution.label;
    if (label.trim().isEmpty || labels.contains(label)) continue;
    labels.add(label);
    if (labels.length >= 2) break;
  }
  return labels.isEmpty ? noun : '$noun · ${labels.join(' · ')}';
}

String _short(String? pubkey) {
  if (pubkey == null || pubkey.isEmpty) return 'unknown';
  return pubkey.length > 12 ? '${pubkey.substring(0, 8)}…' : pubkey;
}
