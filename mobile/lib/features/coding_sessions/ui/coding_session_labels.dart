import '../domain/coding_sessions_domain.dart';

/// The line shown when this device could not verify signatures (D5).
const codingSessionUnverifiedSignaturesLabel =
    'Signatures not verified on this device';

/// The line every session page ends with: mobile observes, it does not drive.
const codingSessionReadOnlyLabel = 'Read-only on mobile';

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
