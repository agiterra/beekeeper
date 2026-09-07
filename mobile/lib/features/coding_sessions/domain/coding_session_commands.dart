import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:uuid/uuid.dart';

import '../../../shared/relay/nostr_models.dart';
import 'coding_session_keys.dart';
import 'coding_session_session_decoders.dart';
import 'coding_session_target.dart';
import 'coding_session_wire.dart';

/// Builders for the member-signed coding-session events this app publishes.
///
/// Every builder here produces the exact bytes the desktop's counterpart
/// produces (`desktop/src/features/coding-sessions/lib/codingSessionCommand.ts`
/// and `codingSessionLifecycleCommand.ts`, `codingSessionName.ts`,
/// `codingSessionGoal.ts`, `codingSessionClosure.ts`), because the relay and
/// the provider validate these payloads with `deny_unknown_fields` and a
/// stray key is a refusal, not a warning. Validation happens *before* signing
/// and names the field, so a composer can say which input was wrong.
///
/// Pure Dart: nothing here signs, publishes, or reads a provider.

/// The locked public payload schema for 44220 turn commands.
const codingSessionCommandSchema = 'buzz-coding-session-command/v1';

/// The locked `cs-v` tag version on a 44220 event.
const codingSessionCommandTagVersion = 'csc1-1';

/// Maximum UTF-8 byte length for a command or target identifier.
const maxCodingSessionIdentifierBytes = 256;

/// Maximum UTF-8 byte length of a turn's text.
const maxCodingSessionTextBytes = 12 * 1024;

/// Maximum UTF-8 byte length of a channel reference.
const maxCodingSessionReferenceBytes = 2 * 1024;

/// Maximum UTF-8 byte length of a 44229 name (single line).
const maxCodingSessionNameBytes = 256;

/// Maximum UTF-8 byte length of a 44227 goal.
const maxCodingSessionGoalBytes = 4096;

/// Maximum UTF-8 byte length of a 44221 lifecycle command's content.
const maxCodingSessionLifecycleContentBytes = 16 * 1024;

/// How a turn asks the provider to deliver it (CREW_SESSIONS_PLAN D3).
///
/// [boundary] is the wire default and is *omitted* from the payload: a relay
/// that predates the field validates with `deny_unknown_fields`, so spelling
/// the default out would turn every ordinary turn into a refusal there.
/// [steer] and [interrupt] are written, and require a relay that knows them.
enum CodingSessionTurnDelivery {
  /// Hold; inject when the current turn settles (the default).
  boundary('boundary'),

  /// Native mid-turn injection where the adapter offers it, else downgraded to
  /// [boundary] with a `turn_degraded` receipt saying so.
  steer('steer'),

  /// Founder/lead only: cancel the running turn, then deliver.
  interrupt('interrupt');

  const CodingSessionTurnDelivery(this.wire);

  /// The exact string written under `action.deliver`.
  final String wire;
}

/// Whether a 44230 closure closes or reopens the umbrella.
enum CodingSessionClosureAction {
  closed('closed'),
  open('open');

  const CodingSessionClosureAction(this.wire);

  /// The exact string written under `action`.
  final String wire;
}

/// A refusal raised before signing: [field] names the offending input.
@immutable
class CodingSessionCommandError implements Exception {
  final String field;
  final String message;

  const CodingSessionCommandError(this.field, this.message);

  @override
  String toString() => 'CodingSessionCommandError($field: $message)';
}

/// The unsigned shape of an event this app is about to sign and publish.
///
/// Exactly what `SignedEventRelay.submit` needs, and nothing that only exists
/// after signing.
@immutable
class CodingSessionCommandEvent {
  final int kind;
  final String content;
  final List<List<String>> tags;

  const CodingSessionCommandEvent({
    required this.kind,
    required this.content,
    required this.tags,
  });
}

const _uuid = Uuid();

/// A fresh 44220 command id, `csc-<uuid>`, as the desktop mints them.
String createCodingSessionCommandId() => 'csc-${_uuid.v4()}';

/// A fresh 44221 command id, `csl-<uuid>`, as the desktop mints them.
String createCodingSessionLifecycleCommandId() => 'csl-${_uuid.v4()}';

/// Build a generation-fenced `thread.turn.start` (kind 44220).
///
/// Mirrors `buildCodingSessionTurnStartEvent` in the desktop's
/// `codingSessionCommand.ts`: `deliver` is written only when it is not the
/// default, so a boundary turn is byte-identical to what every client has
/// always published.
CodingSessionCommandEvent buildCodingSessionTurnStartEvent({
  required String channelId,
  required String commandId,
  required CodingSessionTarget target,
  required String text,
  CodingSessionTurnDelivery deliver = CodingSessionTurnDelivery.boundary,
}) {
  _requireBounded(text, 'action.text', maxCodingSessionTextBytes);
  return _buildTurnActionEvent(
    channelId: channelId,
    commandId: commandId,
    target: target,
    action: {
      'type': 'thread.turn.start',
      'text': text,
      if (deliver != CodingSessionTurnDelivery.boundary)
        'deliver': deliver.wire,
    },
  );
}

/// Build a generation-fenced `thread.turn.interrupt` (kind 44220).
CodingSessionCommandEvent buildCodingSessionInterruptEvent({
  required String channelId,
  required String commandId,
  required CodingSessionTarget target,
}) => _buildTurnActionEvent(
  channelId: channelId,
  commandId: commandId,
  target: target,
  action: const {'type': 'thread.turn.interrupt'},
);

CodingSessionCommandEvent _buildTurnActionEvent({
  required String channelId,
  required String commandId,
  required CodingSessionTarget target,
  required Map<String, Object?> action,
}) {
  _requireChannel(channelId);
  _requireBounded(commandId, 'commandId', maxCodingSessionIdentifierBytes);
  _requireTarget(target, 'target');
  final payload = <String, Object?>{
    'schema': codingSessionCommandSchema,
    'commandId': commandId,
    'target': target.toJson(),
    'action': action,
  };
  return CodingSessionCommandEvent(
    kind: EventKind.codingSessionCommand,
    content: jsonEncode(payload),
    tags: [
      ['h', channelId],
      ['cs-v', codingSessionCommandTagVersion],
      ['cs-target', target.key],
    ],
  );
}

/// Build an exact-generation durable `session.stop` (kind 44221).
///
/// [providerAuthorityPubkey] is the signer of the execution's provider facts
/// (`CodingSessionExecution.signerPubkey`); the provider, not the relay,
/// decides whether this identity may stop it (`operator_owns_session`).
CodingSessionCommandEvent buildCodingSessionStopEvent({
  required String channelId,
  required String commandId,
  required CodingSessionTarget target,
  required String providerAuthorityPubkey,
}) {
  _requireChannel(channelId);
  _requireBounded(commandId, 'commandId', maxCodingSessionIdentifierBytes);
  _requireTarget(target, 'action.session');
  if (!isHex64(providerAuthorityPubkey)) {
    throw const CodingSessionCommandError(
      'action.providerAuthorityPubkey',
      'must be a lowercase 64-hex pubkey',
    );
  }
  final payload = <String, Object?>{
    'schema': codingSessionLifecycleCommandSchema,
    'commandId': commandId,
    'action': {
      'type': 'session.stop',
      'session': target.toJson(),
      'providerAuthorityPubkey': providerAuthorityPubkey,
    },
  };
  final content = jsonEncode(payload);
  if (utf8ByteLength(content) > maxCodingSessionLifecycleContentBytes) {
    throw const CodingSessionCommandError(
      'content',
      'lifecycle command exceeds 16 KiB',
    );
  }
  return CodingSessionCommandEvent(
    kind: EventKind.codingSessionLifecycleCommand,
    content: content,
    tags: [
      ['h', channelId],
      ['csl-v', codingSessionLifecycleCommandTagVersion],
      ['csl-command', commandId],
    ],
  );
}

/// Build a 44229 display-name revision for an umbrella session.
///
/// The name is trimmed and must be one non-empty line of at most 256 bytes —
/// the same rule the desktop's `codingSessionName.ts` applies before signing.
CodingSessionCommandEvent buildCodingSessionNameEvent({
  required String channelId,
  required String sessionRef,
  required String name,
}) {
  _requireChannel(channelId);
  _requireSessionRef(sessionRef);
  final content = name.trim();
  _requireBounded(content, 'name', maxCodingSessionNameBytes);
  if (content.contains('\n') || content.contains('\r')) {
    throw const CodingSessionCommandError('name', 'must be a single line');
  }
  return CodingSessionCommandEvent(
    kind: EventKind.codingSessionName,
    content: content,
    tags: [
      ['h', channelId],
      ['d', sessionRef],
      ['csnm-v', codingSessionNameTagVersion],
    ],
  );
}

/// Build a 44227 goal revision for an umbrella session.
CodingSessionCommandEvent buildCodingSessionGoalEvent({
  required String channelId,
  required String sessionRef,
  required String goal,
}) {
  _requireChannel(channelId);
  _requireSessionRef(sessionRef);
  final content = goal.trim();
  _requireBounded(content, 'goal', maxCodingSessionGoalBytes);
  return CodingSessionCommandEvent(
    kind: EventKind.codingSessionGoal,
    content: content,
    tags: [
      ['h', channelId],
      ['d', sessionRef],
      ['csgl-v', codingSessionGoalTagVersion],
    ],
  );
}

/// Build a 44230 closure (close or reopen) for an umbrella session.
///
/// [genesisRef] is the event id of the umbrella's 44226 genesis — the relay
/// admits `closed` only from that genesis's signer.
CodingSessionCommandEvent buildCodingSessionClosureEvent({
  required String channelId,
  required String sessionRef,
  required String genesisRef,
  required CodingSessionClosureAction action,
}) {
  _requireChannel(channelId);
  _requireSessionRef(sessionRef);
  if (!isHex64(genesisRef)) {
    throw const CodingSessionCommandError(
      'genesisRef',
      'must be a lowercase 64-hex event id',
    );
  }
  final payload = <String, Object?>{
    'action': action.wire,
    'genesisRef': genesisRef,
    'sessionRef': sessionRef,
    'v': codingSessionClosureSchemaVersion,
  };
  return CodingSessionCommandEvent(
    kind: EventKind.codingSessionClosure,
    content: jsonEncode(payload),
    tags: [
      ['h', channelId],
      ['d', sessionRef],
      ['cscl-v', codingSessionClosureTagVersion],
      ['cscl-genesis', genesisRef],
    ],
  );
}

void _requireChannel(String channelId) =>
    _requireBounded(channelId, 'channelId', maxCodingSessionReferenceBytes);

void _requireSessionRef(String sessionRef) {
  if (!isCodingSessionSessionRef(sessionRef)) {
    throw const CodingSessionCommandError(
      'sessionRef',
      'must be a canonical lowercase hyphenated UUID',
    );
  }
}

void _requireTarget(CodingSessionTarget target, String field) {
  for (final (name, value) in [
    ('driver', target.driver),
    ('instanceId', target.instanceId),
    ('sessionId', target.sessionId),
  ]) {
    _requireBounded(value, '$field.$name', maxCodingSessionIdentifierBytes);
  }
  if (target.generation <= 0) {
    throw CodingSessionCommandError(
      '$field.generation',
      'must be a positive integer',
    );
  }
}

void _requireBounded(String value, String field, int maxBytes) {
  if (value.trim().isEmpty) {
    throw CodingSessionCommandError(field, 'must not be empty');
  }
  if (utf8ByteLength(value) > maxBytes) {
    throw CodingSessionCommandError(field, 'exceeds $maxBytes bytes');
  }
}
