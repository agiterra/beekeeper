import 'dart:convert';

import 'package:flutter/foundation.dart';

import 'coding_session_fold.dart';
import 'coding_session_models.dart';
import 'coding_session_target.dart';

/// The two item kinds whose pieces join into one message (NIP-CST amendment
/// 3, **Join key**; `conformance/transcript-prose-join/CONTRACT.md` rules
/// 1–6). Each joins only with its own kind.
const codingSessionJoinedProseKinds = {'assistant_text', 'reasoning'};

/// The title of an `assistant_text` message a subagent wrote
/// (`parentToolId` present). Attribution is invariant 5: a subagent's words
/// must never read as the agent's own answer.
const codingSessionSubagentResponseTitle = 'Subagent response';

/// The title of `reasoning` a subagent wrote.
const codingSessionSubagentReasoningTitle = 'Subagent reasoning';

/// The muted line under a message whose answer is still arriving.
const codingSessionWritingLabel = 'Writing…';

/// The latest kind:44223 metadata statuses that end the session itself
/// (CONTRACT rule 7). `interrupted` and `failed` end a turn, not the session,
/// and are deliberately absent: that turn's own `result`/`interrupted` item
/// already ends its message.
const codingSessionSessionEndingStatuses = {
  CodingSessionStatus.completed,
  CodingSessionStatus.stopped,
  CodingSessionStatus.disconnected,
};

/// What this reader knows about whether one exact target (signer + target)
/// can still be writing: its latest metadata status, and whether it holds an
/// unexpired `live` kind:24223 lease for it.
///
/// A target this reader has no entry for has no lease it holds, so nothing in
/// it is arriving (CONTRACT rule 7): no lease, no evidence anyone is writing,
/// and the reader never falls back to turn state alone.
@immutable
class CodingSessionProseLiveness {
  /// The target's latest metadata status, or `null` for none read.
  final CodingSessionStatus? status;

  /// True only for an unexpired `live` lease this reader holds.
  final bool leaseLive;

  const CodingSessionProseLiveness({
    required this.status,
    required this.leaseLive,
  });

  /// True when nothing of this target's answer can still be arriving.
  bool get ended =>
      !leaseLive || codingSessionSessionEndingStatuses.contains(status);
}

/// The stream key a [CodingSessionProseLiveness] is looked up by: the same
/// `signer \0 target.key` a transcript block carries.
String codingSessionProseStreamKey(
  String signerPubkey,
  CodingSessionTarget target,
) => '$signerPubkey\u0000${target.key}';

/// Liveness per exact target, from the executions the fold produced and the
/// leases the trust gate accepted, resolved against [now].
///
/// Reuses [deriveCodingSessionReachability] — the same lease fold the header
/// reads — so "Writing…" and "provider answering" can never disagree about a
/// lease. Only a [CodingSessionReachabilityKind.reachable] verdict counts as
/// live: an unknown read (no lease query back yet, or a tie) is not evidence
/// that anyone is writing. A generation that is not the current one is
/// superseded and never live.
Map<String, CodingSessionProseLiveness> codingSessionProseLivenessFor({
  required Iterable<CodingSessionExecution> executions,
  required Iterable<CodingSessionLease> leases,
  required DateTime now,
  bool leasesRead = true,
}) {
  final leaseList = leases.toList(growable: false);
  return {
    for (final execution in executions)
      codingSessionProseStreamKey(
        execution.signerPubkey,
        execution.target,
      ): CodingSessionProseLiveness(
        status: execution.status,
        leaseLive:
            execution.isCurrentGeneration &&
            deriveCodingSessionReachability(
                  leases: leaseList,
                  currentTarget: execution.target,
                  acceptedCommandId: execution.commandId,
                  authorityPubkey: execution.authority.pubkey,
                  now: now,
                  leasesRead: leasesRead,
                ).kind ==
                CodingSessionReachabilityKind.reachable,
      ),
  };
}

/// One run of joined prose pieces, built while a stream is walked.
///
/// Pieces are kept raw and joined by plain concatenation (rule 3); the
/// caller bounds the *joined* text, never each piece.
class CodingSessionProseRun {
  /// The first piece: the message's identity (rule 4).
  final CodingSessionTranscriptEnvelope first;

  /// The last piece so far: where the message currently ends.
  CodingSessionTranscriptEnvelope last;

  /// The row index the message occupies in the projected items.
  final int index;

  final StringBuffer _text = StringBuffer();
  final Object? _parentToolId;
  String? _messageId;

  CodingSessionProseRun._(this.first, this.index)
    : last = first,
      _parentToolId = first.item['parentToolId'],
      _messageId = _messageIdOf(first) {
    _text.write(_textOf(first));
  }

  /// Start a run at [envelope], which will occupy row [index].
  factory CodingSessionProseRun.start(
    CodingSessionTranscriptEnvelope envelope,
    int index,
  ) => CodingSessionProseRun._(envelope, index);

  /// The joined text so far, unbounded.
  String get text => _text.toString();

  /// Whether [envelope] continues this message (rules 1, 5, 6). The caller
  /// guarantees adjacency: any other item between them closes the run.
  bool accepts(CodingSessionTranscriptEnvelope envelope) {
    if (envelope.itemKind != first.itemKind) return false;
    if (envelope.turnId != first.turnId) return false;
    if (!_sameJson(envelope.item['parentToolId'], _parentToolId)) return false;
    final theirs = _messageIdOf(envelope);
    final ours = _messageId;
    return ours == null || theirs == null || ours == theirs;
  }

  /// Append [envelope] (already [accepts]-checked) to this message.
  void add(CodingSessionTranscriptEnvelope envelope) {
    _text.write(_textOf(envelope));
    last = envelope;
    _messageId ??= _messageIdOf(envelope);
  }

  static String _textOf(CodingSessionTranscriptEnvelope envelope) {
    final text = envelope.item['text'];
    return text is String ? text : '';
  }

  static String? _messageIdOf(CodingSessionTranscriptEnvelope envelope) {
    final id = envelope.item['messageId'];
    return id is String ? id : null;
  }
}

/// `parentToolId` as the row carries it: the string the provider signed, or
/// `null` for the agent's own words.
String? codingSessionParentToolId(Map<String, dynamic> item) {
  final value = item['parentToolId'];
  if (value == null) return null;
  if (value is String) return value;
  // A non-string id is still an attribution; name it rather than dropping it
  // and letting subagent prose read as the agent's own.
  return jsonEncode(value);
}

/// Event ids of every joined message still arriving in one exact target's
/// ordered, de-duplicated stream (CONTRACT rule 7, turn half): the message's
/// last piece is the last item of its turn, and that turn has published no
/// `result` or `interrupted`. Items outside a turn are never arriving. The
/// target half — superseded, session ended, no live lease — is the caller's.
Set<String> codingSessionOpenTurnTails(
  List<CodingSessionTranscriptEnvelope> ordered,
) {
  final lastByTurn = <String, String>{};
  final endedTurns = <String>{};
  for (final envelope in ordered) {
    final turnId = envelope.turnId;
    if (turnId == null) continue;
    lastByTurn[turnId] = envelope.ref.eventId;
    final kind = envelope.itemKind;
    if (kind == 'result' || kind == 'interrupted') endedTurns.add(turnId);
  }
  return {
    for (final entry in lastByTurn.entries)
      if (!endedTurns.contains(entry.key)) entry.value,
  };
}

/// Stream keys of targets some later generation of the same signer +
/// execution has superseded (a crash, a host restart, a resume): nothing in
/// them can still be arriving.
Set<String> codingSessionSupersededStreams(
  Iterable<CodingSessionTranscriptEnvelope> envelopes,
) {
  final highest = <String, int>{};
  final streams = <String, CodingSessionTranscriptEnvelope>{};
  for (final envelope in envelopes) {
    final execution =
        '${envelope.ref.signerPubkey}\u0000${envelope.target.executionKey}';
    final generation = envelope.target.generation;
    if ((highest[execution] ?? 0) < generation) highest[execution] = generation;
    streams[codingSessionProseStreamKey(
          envelope.ref.signerPubkey,
          envelope.target,
        )] =
        envelope;
  }
  return {
    for (final entry in streams.entries)
      if (entry.value.target.generation <
          (highest['${entry.value.ref.signerPubkey}\u0000'
                  '${entry.value.target.executionKey}'] ??
              0))
        entry.key,
  };
}

bool _sameJson(Object? left, Object? right) {
  if (left == null || right == null) return left == right;
  if (left is String || right is String) return left == right;
  return jsonEncode(left) == jsonEncode(right);
}
