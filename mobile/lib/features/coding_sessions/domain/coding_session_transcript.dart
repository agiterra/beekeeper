import 'dart:convert';

import 'coding_session_keys.dart';
import 'coding_session_models.dart';
import 'coding_session_transcript_item.dart';

/// Provider continuity slugs, in the reader's terms.
///
/// Additive by design: a provider may publish a slug this build has never
/// seen, and an unknown one keeps the generic `Status` title. Guessing prose
/// for an unknown slug would be inventing a fact about the session's history.
const codingSessionContinuityStatuses = <String, String>{
  'session_fresh': 'Started fresh — no prior session context',
  'session_rehydrated':
      'Rehydrated — verified session history is available to this agent',
  'session_resumed':
      'Resumed — reconnected to the provider\u2019s native session',
  'session_loaded': 'Loaded — the provider replayed its native session history',
  'session_restarted_without_context': 'Restarted without prior context',
};

/// The title continuity rows carry, distinct from the generic `Status`.
const codingSessionContinuityTitle = 'Session continuity';

/// Project execution boundary statuses, in the reader's terms. Published by
/// the session provider on every open (`execution_scope.rs`
/// `boundary_status_item`); the `reason` is the enforcing backend or, when
/// nothing is enforced, why. A policy file alone never earns "enforced": the
/// provider publishes it only after its self-test passed.
const codingSessionBoundaryStatuses = <String, String>{
  'execution_boundary_enforced':
      'Enforced — this agent and everything it runs can reach only this project\'s files',
  'execution_boundary_not_enforced':
      'Not enforced — only instructions keep this agent to this project\'s files',
};

const codingSessionBoundaryTitle = 'Project boundary';

/// What the provider withheld from the session beyond the project boundary
/// (`session_isolation.rs` `isolation_status_items`). Published only when the
/// provider's setting is on; the `reason` is a fixed slug and adds nothing.
const codingSessionIsolationStatuses = <String, String>{
  'operator_git_withheld':
      'Git credentials from this computer were withheld from this session',
  'network_egress_proxy_only':
      'This session can reach the network only through the provider\u2019s egress proxy',
};

const codingSessionIsolationTitle = 'Session isolation';

const _maxSummaryChars = 200;
const _maxTextChars = 8000;

/// Project decoded transcript envelopes into renderable blocks.
///
/// Ordering is numeric on `eventSeq` — item 10 comes after item 9, never
/// before it as a lexicographic sort would have it — with ties broken by event
/// id. Tool results pair to the pending call with the same `toolId` within the
/// same stream; an unpaired result renders on its own rather than vanishing.
///
/// Turn boundaries come from the envelope's `turnId`. The wire envelope always
/// carries the key, and an explicit `null` means "this item belongs to no
/// turn" — a claim the projector honours rather than overwriting with a guess.
List<CodingSessionTranscriptBlock> projectCodingSessionTranscript(
  Iterable<CodingSessionTranscriptEnvelope> envelopes, {
  Map<String, String> labelsByTargetKey = const {},
}) {
  final grouped = <String, List<CodingSessionTranscriptEnvelope>>{};
  for (final envelope in envelopes) {
    final key = '${envelope.ref.signerPubkey}\u0000${envelope.target.key}';
    grouped.putIfAbsent(key, () => []).add(envelope);
  }

  final blocks = <CodingSessionTranscriptBlock>[];
  for (final entry in grouped.entries) {
    final ordered = [...entry.value]..sort(_byEventSeqThenEventId);
    final items = _projectStream(ordered);
    var lastActivityAt = 0;
    for (final envelope in ordered) {
      if (envelope.ref.createdAt > lastActivityAt) {
        lastActivityAt = envelope.ref.createdAt;
      }
    }
    final target = ordered.first.target;
    blocks.add(
      CodingSessionTranscriptBlock(
        signerPubkey: ordered.first.ref.signerPubkey,
        target: target,
        label: labelsByTargetKey[target.key] ?? target.driver,
        items: List.unmodifiable(items),
        turns: List.unmodifiable(_groupTurns(items)),
        lastActivityAt: lastActivityAt,
      ),
    );
  }
  blocks.sort((left, right) {
    final byStart = left.items.isEmpty || right.items.isEmpty
        ? 0
        : left.items.first.timestamp.compareTo(right.items.first.timestamp);
    return byStart != 0 ? byStart : left.key.compareTo(right.key);
  });
  return List.unmodifiable(blocks);
}

int _byEventSeqThenEventId(
  CodingSessionTranscriptEnvelope left,
  CodingSessionTranscriptEnvelope right,
) {
  final bySeq = left.eventSeq.compareTo(right.eventSeq);
  return bySeq != 0 ? bySeq : left.ref.eventId.compareTo(right.ref.eventId);
}

List<CodingSessionTranscriptItem> _projectStream(
  List<CodingSessionTranscriptEnvelope> ordered,
) {
  final items = <CodingSessionTranscriptItem>[];
  final pendingToolCalls = <String, int>{};
  for (final envelope in ordered) {
    final item = envelope.item;
    final kind = envelope.itemKind;
    if (kind == 'tool_result') {
      final toolId = _stringOrNull(item['toolId']);
      final pendingIndex = toolId == null ? null : pendingToolCalls[toolId];
      if (pendingIndex != null) {
        items[pendingIndex] = _pairToolResult(items[pendingIndex], item);
        pendingToolCalls.remove(toolId);
        continue;
      }
      items.add(_buildItem(envelope));
      continue;
    }
    items.add(_buildItem(envelope));
    if (kind == 'tool_call') {
      final tool = item['tool'];
      final toolId = tool is Map ? _stringOrNull(tool['toolId']) : null;
      if (toolId != null) pendingToolCalls[toolId] = items.length - 1;
    }
  }
  return items;
}

List<CodingSessionTranscriptTurn> _groupTurns(
  List<CodingSessionTranscriptItem> items,
) {
  final turns = <CodingSessionTranscriptTurn>[];
  var current = <CodingSessionTranscriptItem>[];
  String? currentTurnId;
  for (final item in items) {
    if (current.isEmpty || item.turnId == currentTurnId) {
      current.add(item);
      currentTurnId = item.turnId;
      continue;
    }
    turns.add(
      CodingSessionTranscriptTurn(
        turnId: currentTurnId,
        items: List.unmodifiable(current),
      ),
    );
    current = [item];
    currentTurnId = item.turnId;
  }
  if (current.isNotEmpty) {
    turns.add(
      CodingSessionTranscriptTurn(
        turnId: currentTurnId,
        items: List.unmodifiable(current),
      ),
    );
  }
  return turns;
}

CodingSessionTranscriptItem _buildItem(
  CodingSessionTranscriptEnvelope envelope,
) {
  final item = envelope.item;
  return switch (envelope.itemKind) {
    'user_prompt' => _base(
      envelope,
      type: CodingSessionItemType.message,
      role: CodingSessionItemRole.user,
      title: item['steered'] == true ? 'Steered prompt' : 'Prompt',
      text: _userPromptText(item),
      steered: item['steered'] == true,
      operatorPubkey: _pubkeyOrNull(item['operatorPubkey']),
      commandId: _stringOrNull(item['commandId']),
    ),
    'assistant_text' => _base(
      envelope,
      type: CodingSessionItemType.message,
      role: CodingSessionItemRole.assistant,
      title: 'Response',
      text: _boundedText(item['text']),
    ),
    'tool_call' => _toolCall(envelope),
    'tool_result' => _unpairedToolResult(envelope),
    'result' => _base(
      envelope,
      type: CodingSessionItemType.lifecycle,
      title: 'Turn result',
      text: _boundedText(item['result']),
      result: CodingSessionTurnResult(
        outcome: _stringOrNull(item['subtype']),
        durationMs: item['durationMs'] is int
            ? item['durationMs'] as int
            : null,
        costUsd: item['costUsd'] is num
            ? (item['costUsd'] as num).toDouble()
            : null,
        isError: item['isError'] == true,
      ),
    ),
    'status' => _status(envelope),
    'plan' => _base(
      envelope,
      type: CodingSessionItemType.plan,
      title: 'Plan',
      text: _planText(item),
    ),
    'reasoning' => _base(
      envelope,
      type: CodingSessionItemType.thought,
      title: 'Reasoning',
      text: _boundedText(item['text']),
      foldedByDefault: true,
    ),
    'elided' => _base(
      envelope,
      type: CodingSessionItemType.lifecycle,
      title: 'Content elided',
      text: _elidedText(item),
    ),
    'interrupted' => _lifecycle(envelope, 'Interrupted'),
    'compact_boundary' => _lifecycle(envelope, 'Context compact boundary'),
    'compact_summary' => _base(
      envelope,
      type: CodingSessionItemType.lifecycle,
      title: 'Context compacted',
      text: _boundedText(item['summary']),
    ),
    'context_cleared' => _lifecycle(envelope, 'Context cleared'),
    'system_init' => _lifecycle(envelope, 'Session started'),
    'account_info' => _lifecycle(envelope, 'Account'),
    'context_window_updated' => _lifecycle(envelope, 'Context window updated'),
    _ => _unknown(envelope),
  };
}

CodingSessionTranscriptItem _toolCall(
  CodingSessionTranscriptEnvelope envelope,
) {
  final tool = envelope.item['tool'];
  final record = tool is Map ? tool : const {};
  final toolName = _stringOrNull(record['toolName']) ?? 'unknown_tool';
  final args = record['input'] is Map<String, dynamic>
      ? record['input']! as Map<String, dynamic>
      : const <String, dynamic>{};
  return _base(
    envelope,
    type: CodingSessionItemType.tool,
    title: toolName,
    text: '',
    foldedByDefault: true,
    tool: CodingSessionToolRow(
      toolName: toolName,
      toolId: _stringOrNull(record['toolId']),
      args: args,
      argsSummary: summarizeToolArgs(args),
      result: null,
      isError: false,
      status: CodingSessionToolStatus.executing,
    ),
  );
}

CodingSessionTranscriptItem _unpairedToolResult(
  CodingSessionTranscriptEnvelope envelope,
) {
  final item = envelope.item;
  final toolId = _stringOrNull(item['toolId']);
  final toolName = _stringOrNull(item['toolName']) ?? toolId ?? 'unknown_tool';
  final isError = item['isError'] == true;
  return _base(
    envelope,
    type: CodingSessionItemType.tool,
    title: toolName,
    text: '',
    foldedByDefault: true,
    tool: CodingSessionToolRow(
      toolName: toolName,
      toolId: toolId,
      args: const {},
      argsSummary: '',
      result: _boundedText(_stringify(item['content'])),
      isError: isError,
      outputGap: CodingSessionToolOutputGap.fromResult(item),
      status: isError
          ? CodingSessionToolStatus.failed
          : CodingSessionToolStatus.completed,
    ),
  );
}

CodingSessionTranscriptItem _pairToolResult(
  CodingSessionTranscriptItem call,
  Map<String, dynamic> result,
) {
  final isError = result['isError'] == true;
  final tool = call.tool!;
  return CodingSessionTranscriptItem(
    id: call.id,
    eventId: call.eventId,
    signerPubkey: call.signerPubkey,
    target: call.target,
    eventSeq: call.eventSeq,
    timestamp: call.timestamp,
    turnId: call.turnId,
    type: call.type,
    title: call.title,
    text: call.text,
    foldedByDefault: true,
    tool: CodingSessionToolRow(
      toolName: tool.toolName,
      toolId: tool.toolId,
      args: tool.args,
      argsSummary: tool.argsSummary,
      result: _boundedText(_stringify(result['content'])),
      isError: isError,
      outputGap: CodingSessionToolOutputGap.fromResult(result),
      status: isError
          ? CodingSessionToolStatus.failed
          : CodingSessionToolStatus.completed,
    ),
  );
}

CodingSessionTranscriptItem _status(CodingSessionTranscriptEnvelope envelope) {
  final slug = _stringOrNull(envelope.item['status']) ?? '';
  final isolation = codingSessionIsolationStatuses[slug];
  if (isolation != null) {
    return _base(
      envelope,
      type: CodingSessionItemType.lifecycle,
      title: codingSessionIsolationTitle,
      text: isolation,
    );
  }
  final boundary = codingSessionBoundaryStatuses[slug];
  if (boundary != null) {
    final reason = _stringOrNull(envelope.item['reason']);
    return _base(
      envelope,
      type: CodingSessionItemType.lifecycle,
      title: codingSessionBoundaryTitle,
      text: reason == null || reason.isEmpty
          ? boundary
          : '$boundary (${_bounded(reason, 80)})',
    );
  }
  final continuity = codingSessionContinuityStatuses[slug];
  if (continuity == null) {
    return _base(
      envelope,
      type: CodingSessionItemType.lifecycle,
      title: 'Status',
      text: _bounded(slug, _maxSummaryChars),
    );
  }
  return _base(
    envelope,
    type: CodingSessionItemType.lifecycle,
    title: codingSessionContinuityTitle,
    text: continuity,
  );
}

CodingSessionTranscriptItem _lifecycle(
  CodingSessionTranscriptEnvelope envelope,
  String title,
) => _base(
  envelope,
  type: CodingSessionItemType.lifecycle,
  title: title,
  text: '',
);

/// An item kind this build does not know: name it, surface nothing else.
CodingSessionTranscriptItem _unknown(CodingSessionTranscriptEnvelope envelope) {
  final kind = _bounded(envelope.itemKind, 80);
  return _base(
    envelope,
    type: CodingSessionItemType.lifecycle,
    title: 'Unrecognized item kind: $kind',
    text: '',
    unknownKind: kind,
  );
}

CodingSessionTranscriptItem _base(
  CodingSessionTranscriptEnvelope envelope, {
  required CodingSessionItemType type,
  required String title,
  required String text,
  CodingSessionItemRole? role,
  bool steered = false,
  String? operatorPubkey,
  String? commandId,
  CodingSessionToolRow? tool,
  CodingSessionTurnResult? result,
  bool foldedByDefault = false,
  String? unknownKind,
}) => CodingSessionTranscriptItem(
  id: encodeStructuredKey('coding-session-transcript-item/v1', [
    envelope.ref.signerPubkey,
    envelope.target.key,
    '${envelope.eventSeq}',
  ]),
  eventId: envelope.ref.eventId,
  signerPubkey: envelope.ref.signerPubkey,
  target: envelope.target,
  eventSeq: envelope.eventSeq,
  timestamp: DateTime.fromMillisecondsSinceEpoch(
    envelope.timestamp,
    isUtc: true,
  ),
  turnId: envelope.turnId,
  type: type,
  role: role,
  title: title,
  text: text,
  steered: steered,
  operatorPubkey: operatorPubkey,
  commandId: commandId,
  tool: tool,
  result: result,
  foldedByDefault: foldedByDefault,
  unknownKind: unknownKind,
);

/// A bounded one-line rendering of a tool call's arguments.
String summarizeToolArgs(Map<String, dynamic> args) {
  if (args.isEmpty) return '';
  final parts = <String>[];
  for (final entry in args.entries) {
    parts.add('${entry.key}=${_stringify(entry.value)}');
    if (parts.length >= 6) break;
  }
  final joined = parts
      .join(' ')
      .replaceAll('\n', ' ')
      .replaceAll('\r', ' ')
      .trim();
  return _bounded(joined, _maxSummaryChars);
}

String _planText(Map<String, dynamic> item) {
  final declared = _boundedText(item['text']);
  if (declared.trim().isNotEmpty) return declared;
  final entries = item['entries'];
  if (entries is! List) return '';
  final lines = <String>[];
  for (final entry in entries) {
    if (entry is! Map) continue;
    final content = _stringOrNull(entry['content']);
    if (content == null) continue;
    final status = _stringOrNull(entry['status']) ?? '';
    final checkbox = status == 'completed' ? '[x]' : '[ ]';
    final suffix = status == 'in_progress' ? ' (in progress)' : '';
    lines.add('- $checkbox ${_bounded(content, _maxSummaryChars)}$suffix');
    if (lines.length >= 50) break;
  }
  return lines.join('\n');
}

/// An item the producer had to drop whole: say so, and how much.
String _elidedText(Map<String, dynamic> item) {
  final reason = _stringOrNull(item['reason']) ?? 'unknown';
  final byteCount = item['byteCount'] is int
      ? '${item['byteCount']}'
      : 'unknown';
  return 'reason: ${_bounded(reason, 80)} · byteCount: $byteCount';
}

String? _stringOrNull(Object? value) =>
    value is String && value.trim().isNotEmpty ? value : null;

String? _pubkeyOrNull(Object? value) {
  if (value is! String) return null;
  final normalized = value.trim().toLowerCase();
  return RegExp(r'^[0-9a-f]{64}$').hasMatch(normalized) ? normalized : null;
}

/// A prompt's text, with the attachment count the provider signed.
///
/// Mirrors the desktop's `buildUserPromptMessage`
/// (`codingSessionTranscriptItems.ts`): a turn that carried three files must
/// not read exactly like one that carried none. Only a positive integer count
/// is shown, and it is stated as a count — the files themselves are not on
/// this device and are never implied to be.
String _userPromptText(Map<String, Object?> item) {
  final text = _boundedText(item['content']);
  final attachments = item['attachmentCount'];
  if (attachments is! int || attachments <= 0) return text;
  final noun = attachments == 1 ? 'attachment' : 'attachments';
  return '$text\n\n($attachments $noun)';
}

String _boundedText(Object? value) =>
    value is String ? _bounded(value, _maxTextChars) : '';

String _bounded(String value, int maxChars) =>
    value.length <= maxChars ? value : '${value.substring(0, maxChars)}…';

String _stringify(Object? value) {
  if (value == null) return '';
  if (value is String) return value;
  try {
    return jsonEncode(value);
  } on JsonUnsupportedObjectError {
    return '';
  }
}
