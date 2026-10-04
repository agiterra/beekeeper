import 'package:flutter/foundation.dart';
import 'package:intl/intl.dart';

import 'coding_session_target.dart';

/// The lane a transcript row belongs to.
enum CodingSessionItemType {
  /// A prompt or an assistant reply.
  message,

  /// A tool call, possibly already paired with its result.
  tool,

  /// A bounded lifecycle marker: turn results, continuity, compaction.
  lifecycle,

  /// The agent's plan snapshot.
  plan,

  /// The agent's reasoning; folded by default.
  thought,
}

/// Who spoke, for [CodingSessionItemType.message] rows.
enum CodingSessionItemRole { user, assistant }

/// How far a tool call got.
enum CodingSessionToolStatus { executing, completed, failed }

/// A tool call row, with its result once one arrives.
@immutable
class CodingSessionToolRow {
  /// The provider's display name for the tool.
  final String toolName;

  /// The provider's id for the call, used to pair a result to it.
  final String? toolId;

  /// The call's arguments, kept opaque.
  final Map<String, dynamic> args;

  /// A bounded one-line rendering of [args] for the folded row.
  final String argsSummary;

  /// The result text, once the call is paired.
  final String? result;

  final bool isError;

  final CodingSessionToolStatus status;

  /// Set only when the result's output may be missing its beginning.
  final CodingSessionToolOutputGap? outputGap;

  const CodingSessionToolRow({
    required this.toolName,
    required this.toolId,
    required this.args,
    required this.argsSummary,
    required this.result,
    required this.isError,
    required this.status,
    this.outputGap,
  });
}

/// The headline said beside a tool output that may be missing its start.
const codingSessionToolOutputGapHeadline =
    'Output may be missing its beginning';

/// A tool result the provider could not verify complete (44225
/// `outputComplete: false`): assembled from streamed chunks, and the adapter
/// is known to drop the beginning of a command's output.
///
/// `contentSource`, `outputComplete` and `outputGap` travel together. Absent
/// keys mean the adapter's final frame as always, and `outputComplete: true`
/// means verified or recovered — neither earns a gap. A malformed `outputGap`
/// costs only the byte counts, never the notice.
@immutable
class CodingSessionToolOutputGap {
  /// Bytes captured from the stream, when the provider reported a count.
  final int? streamedBytes;

  /// Bytes the adapter said the full output had, when it said.
  final int? aggregatedBytes;

  const CodingSessionToolOutputGap({this.streamedBytes, this.aggregatedBytes});

  /// The gap a raw `tool_result` declares, or `null` when it declares none.
  static CodingSessionToolOutputGap? fromResult(Map<String, dynamic> result) {
    if (result['outputComplete'] != false) return null;
    final declared = result['outputGap'];
    final gap = declared is Map ? declared : const {};
    final streamed = _byteCount(gap['streamedBytes']);
    final aggregated = _byteCount(gap['aggregatedBytes']);
    return CodingSessionToolOutputGap(
      streamedBytes: streamed,
      // A total below what was captured contradicts itself; drop it rather
      // than print "captured 900 of 600 bytes".
      aggregatedBytes:
          aggregated != null && (streamed == null || aggregated >= streamed)
          ? aggregated
          : null,
    );
  }

  /// `captured 1,193 of 1,793 bytes`, `captured 1,193 bytes`, or `null` when
  /// the provider reported no count.
  String? get detail {
    final streamed = streamedBytes;
    if (streamed == null) return null;
    final format = NumberFormat.decimalPattern('en_US');
    final aggregated = aggregatedBytes;
    return aggregated == null
        ? 'captured ${format.format(streamed)} bytes'
        : 'captured ${format.format(streamed)} of '
              '${format.format(aggregated)} bytes';
  }

  static int? _byteCount(Object? value) =>
      value is int && value >= 0 ? value : null;

  @override
  bool operator ==(Object other) =>
      other is CodingSessionToolOutputGap &&
      other.streamedBytes == streamedBytes &&
      other.aggregatedBytes == aggregatedBytes;

  @override
  int get hashCode => Object.hash(streamedBytes, aggregatedBytes);
}

/// The structured outcome a `result` item reports.
///
/// Duration and cost travel as fields, never baked into the row's text: a
/// reader can format them, and nobody has to parse prose to find them.
@immutable
class CodingSessionTurnResult {
  final String? outcome;
  final int? durationMs;
  final double? costUsd;
  final bool isError;

  const CodingSessionTurnResult({
    required this.outcome,
    required this.durationMs,
    required this.costUsd,
    required this.isError,
  });
}

/// One projected transcript row.
@immutable
class CodingSessionTranscriptItem {
  /// Stable per-item identity: `(generation, eventSeq)`.
  final String id;

  /// The event this row came from — for joined prose, its **first** piece,
  /// so the row keeps its identity as later paragraphs arrive.
  final String eventId;

  /// The event the row currently ends at: [eventId] for a single-event row,
  /// the last joined piece for prose (NIP-CST amendment 3, Join key).
  final String lastEventId;

  /// The signer of that event.
  final String signerPubkey;

  /// The generation this row belongs to.
  final CodingSessionTarget target;

  /// The provider's per-generation sequence; the ordering key.
  final int eventSeq;

  /// The provider's timestamp for the item.
  final DateTime timestamp;

  /// The turn this row belongs to, or `null` for "belongs to no turn".
  final String? turnId;

  final CodingSessionItemType type;
  final CodingSessionItemRole? role;

  /// The row's heading, e.g. `Prompt`, `Steered prompt`, `Turn result`.
  final String title;

  /// The row's body. Empty for rows that deliberately surface no payload.
  final String text;

  /// True when the prompt was a steer into a running turn.
  final bool steered;

  /// The operator the provider verified before running the turn.
  final String? operatorPubkey;

  /// The 44220 command id this prompt echoes, when the provider stamped one.
  final String? commandId;

  final CodingSessionToolRow? tool;
  final CodingSessionTurnResult? result;

  /// True for rows a reader should have to open: reasoning and tool detail.
  final bool foldedByDefault;

  /// Set when the item's `kind` is one this build does not know.
  ///
  /// Such a row names the kind and carries no payload — surfacing an unknown
  /// provider payload verbatim is how a transcript starts lying.
  final String? unknownKind;

  /// The subagent tool call this prose belongs to, or `null` for the agent's
  /// own words. Prose with one is titled as a subagent's, never as the
  /// agent's answer (attribution, invariant 5).
  final String? parentToolId;

  /// True while this joined message is still being written: its last piece
  /// is the last item of a turn with no `result`/`interrupted` yet, and its
  /// exact target is neither superseded, nor ended by status, nor without a
  /// live lease this reader holds (CONTRACT rule 7). Derived from wire facts
  /// only — never from a provider claiming it is streaming.
  final bool arriving;

  const CodingSessionTranscriptItem({
    required this.id,
    required this.eventId,
    String? lastEventId,
    required this.signerPubkey,
    required this.target,
    required this.eventSeq,
    required this.timestamp,
    required this.turnId,
    required this.type,
    required this.title,
    required this.text,
    this.role,
    this.steered = false,
    this.operatorPubkey,
    this.commandId,
    this.tool,
    this.result,
    this.foldedByDefault = false,
    this.unknownKind,
    this.parentToolId,
    this.arriving = false,
  }) : lastEventId = lastEventId ?? eventId;
}

/// A contiguous run of rows sharing one turn id.
@immutable
class CodingSessionTranscriptTurn {
  /// The turn's id, or `null` for the rows that belong to no turn.
  final String? turnId;

  final List<CodingSessionTranscriptItem> items;

  const CodingSessionTranscriptTurn({
    required this.turnId,
    required this.items,
  });
}

/// One `(signer, target)` stream of transcript rows.
///
/// A session with several executions interleaves *blocks*, never individual
/// items: two providers' sequence numbers are unrelated, so merging their rows
/// would invent an ordering neither of them signed.
@immutable
class CodingSessionTranscriptBlock {
  final String signerPubkey;
  final CodingSessionTarget target;

  /// What to call this execution: `runtime · model`, or its agent reference.
  final String label;

  /// Every row in the block, ordered by `eventSeq` then event id.
  final List<CodingSessionTranscriptItem> items;

  /// The same rows grouped into contiguous turns.
  final List<CodingSessionTranscriptTurn> turns;

  /// Newest `created_at` in the block, in epoch seconds.
  final int lastActivityAt;

  const CodingSessionTranscriptBlock({
    required this.signerPubkey,
    required this.target,
    required this.label,
    required this.items,
    required this.turns,
    required this.lastActivityAt,
  });

  /// The block's stream identity.
  String get key => '$signerPubkey\u0000${target.key}';
}
