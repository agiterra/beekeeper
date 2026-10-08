import 'dart:convert';

import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/shared/relay/nostr_models.dart';

/// Deterministic fixture builders for signed coding-session events.
///
/// The tag envelopes here are the producer's exact ones — order, count and
/// semantic keys included — because a fixture that skips them would let a
/// decoder regression pass.

const channelId = 'c0ffee00-0000-4000-8000-000000000001';
const providerPubkey =
    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const otherProviderPubkey =
    'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
const founderPubkey =
    'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc';
const otherFounderPubkey =
    'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd';

/// The seat actor a hired create names, and the kind:44221 hire it answers.
const seatActorPubkey =
    'f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1';
const hireEventId =
    'a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9';
const sessionRefA = '11111111-1111-4111-8111-111111111111';
const sessionRefB = '22222222-2222-4222-8222-222222222222';
const genesisEventIdA =
    'e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1';
const genesisEventIdB =
    'e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2';

var _idCounter = 0;

/// A deterministic, syntactically valid 64-hex event id.
String nextEventId() {
  _idCounter += 1;
  return _idCounter.toRadixString(16).padLeft(64, '0');
}

/// Reset the id counter so a test can pin exact ids.
void resetEventIds() => _idCounter = 0;

NostrEvent event({
  required int kind,
  required List<List<String>> tags,
  required String content,
  String pubkey = providerPubkey,
  int createdAt = 1000,
  String? id,
}) => NostrEvent(
  id: id ?? nextEventId(),
  pubkey: pubkey,
  createdAt: createdAt,
  kind: kind,
  tags: tags,
  content: content,
  sig: '0' * 128,
);

CodingSessionTarget target({
  String driver = 'claude-agent-acp',
  String instanceId = 'instance-1',
  String sessionId = 'session-1',
  int generation = 1,
}) => CodingSessionTarget(
  driver: driver,
  instanceId: instanceId,
  sessionId: sessionId,
  generation: generation,
);

Map<String, Object?> _capabilities() => {
  'threadTurnStart': true,
  'threadTurnInterrupt': true,
  'threadSteer': false,
  'context': true,
  'diff': false,
  'plan': false,
};

/// A signed 44223 metadata event.
NostrEvent metadataEvent({
  CodingSessionTarget? forTarget,
  String status = 'running',
  String pubkey = providerPubkey,
  int createdAt = 1000,
  String? id,
  String? sessionRef,
  String? title,
  String runtime = 'claude-agent-acp',
  String model = 'opus',
  String? agentRef,
  String? role,
  Map<String, Object?>? turnBudget,
  Map<String, Object?>? routing,
}) {
  final resolved = forTarget ?? target();
  final payload = <String, Object?>{
    'schema': 'buzz-coding-session-metadata/v1',
    'session': resolved.toJson(),
    'projectRef': null,
    'repoRef': null,
    'title': title,
    'agentRef': agentRef,
    'provider': 'buzz-session-provider',
    'runtime': runtime,
    'model': model,
    'status': status,
    'branch': null,
    'capabilities': _capabilities(),
    'sessionRef': ?sessionRef,
    'role': ?role,
    'turnBudget': ?turnBudget,
    'routing': ?routing,
  };
  return event(
    kind: EventKind.codingSessionMetadata,
    pubkey: pubkey,
    createdAt: createdAt,
    id: id,
    tags: [
      ['h', channelId],
      ['csm-v', 'csm1-1'],
      ['cs-target', resolved.key],
      ['csm-key', resolved.metadataSemanticKey],
    ],
    content: jsonEncode(payload),
  );
}

/// A signed 44224 receipt event.
NostrEvent receiptEvent({
  required String commandId,
  required String status,
  CodingSessionTarget? forTarget,
  String pubkey = providerPubkey,
  int createdAt = 900,
  String? id,
  Map<String, Object?>? error,
  String? turnId,
  Map<String, Object?>? rewind,
}) {
  final resolved = status == 'failed' ? null : (forTarget ?? target());
  final payload = <String, Object?>{
    'schema': 'buzz-coding-session-lifecycle-receipt/v1',
    'commandId': commandId,
    'status': status,
    'session': resolved?.toJson(),
    'error': error,
    'turnId': ?turnId,
    'rewind': ?rewind,
  };
  final receiptStatus = CodingSessionReceiptStatus.fromWire(status)!;
  return event(
    kind: EventKind.codingSessionLifecycleReceipt,
    pubkey: pubkey,
    createdAt: createdAt,
    id: id,
    tags: [
      ['h', channelId],
      ['cslr-v', 'cslr1-1'],
      ['csl-command', commandId],
      ['csl-key', codingSessionReceiptSemanticKey(commandId, receiptStatus)],
    ],
    content: jsonEncode(payload),
  );
}

/// A signed 44225 transcript envelope event.
NostrEvent transcriptEvent({
  required int eventSeq,
  required Map<String, Object?> item,
  CodingSessionTarget? forTarget,
  String pubkey = providerPubkey,
  int createdAt = 1100,
  int? timestamp,
  String? turnId,
  String? id,
}) {
  final resolved = forTarget ?? target();
  final payload = <String, Object?>{
    'schema': 'buzz-coding-session-transcript/v1',
    'session': resolved.toJson(),
    'eventSeq': eventSeq,
    'timestamp': timestamp ?? (1700000000000 + eventSeq),
    'turnId': turnId,
    'item': item,
  };
  return event(
    kind: EventKind.codingSessionTranscript,
    pubkey: pubkey,
    createdAt: createdAt,
    id: id,
    tags: [
      ['h', channelId],
      ['cst-v', 'cst1-1'],
      ['cs-target', resolved.key],
      ['cst-seq', '$eventSeq'],
      ['cst-key', resolved.transcriptSemanticKey(eventSeq)],
    ],
    content: jsonEncode(payload),
  );
}

/// A signed 44221 `session.create` event.
NostrEvent createEvent({
  required String commandId,
  String pubkey = founderPubkey,
  String authority = providerPubkey,
  String? sessionRef,
  String? genesisRef,
  int createdAt = 800,
  String? id,
  String? actor,
  String? role,
  String? hireRef,
  Map<String, Object?>? routing,
}) {
  final action = <String, Object?>{
    'type': 'session.create',
    'projectRef': null,
    'repoRef': null,
    if (sessionRef != null || genesisRef != null) 'sessionRef': sessionRef,
    'genesisRef': ?genesisRef,
    'providerInstanceRef': 'provider-instance-1',
    'providerAuthorityPubkey': authority,
    'model': 'opus',
    'title': null,
    'initialTurn': null,
    'actor': ?actor,
    'role': ?role,
    'hireRef': ?hireRef,
    'routing': ?routing,
  };
  return event(
    kind: EventKind.codingSessionLifecycleCommand,
    pubkey: pubkey,
    createdAt: createdAt,
    id: id,
    tags: [
      ['h', channelId],
      ['csl-v', 'csl1-1'],
      ['csl-command', commandId],
    ],
    content: jsonEncode({
      'schema': 'buzz-coding-session-lifecycle-command/v1',
      'commandId': commandId,
      'action': action,
    }),
  );
}

/// A signed 44221 `session.resume` event, or a `session.restart` via [type].
///
/// A resume names the generation it is reattaching to and the provider that
/// may answer it; the provider's receipt mints the next generation. A restart
/// has the same shape and mints the same way.
NostrEvent resumeEvent({
  required String commandId,
  CodingSessionTarget? forTarget,
  String pubkey = founderPubkey,
  String authority = providerPubkey,
  int createdAt = 800,
  String? id,
  String type = 'session.resume',
  Map<String, Object?> extraAction = const {},
}) {
  final resolved = forTarget ?? target();
  return event(
    kind: EventKind.codingSessionLifecycleCommand,
    pubkey: pubkey,
    createdAt: createdAt,
    id: id,
    tags: [
      ['h', channelId],
      ['csl-v', 'csl1-1'],
      ['csl-command', commandId],
    ],
    content: jsonEncode({
      'schema': 'buzz-coding-session-lifecycle-command/v1',
      'commandId': commandId,
      'action': {
        'type': type,
        'session': resolved.toJson(),
        'providerAuthorityPubkey': authority,
        ...extraAction,
      },
    }),
  );
}

/// A signed 44226 genesis event.
NostrEvent genesisEvent({
  required String eventId,
  String sessionRef = sessionRefA,
  String pubkey = founderPubkey,
  int createdAt = 700,
}) => event(
  kind: EventKind.codingSessionGenesis,
  pubkey: pubkey,
  createdAt: createdAt,
  id: eventId,
  tags: [
    ['h', channelId],
    ['csg-v', 'csg1-1'],
    ['csg-session', sessionRef],
  ],
  content: jsonEncode({'sessionRef': sessionRef, 'v': 1}),
);

/// A signed 44229 name event.
NostrEvent nameEvent({
  required String content,
  String sessionRef = sessionRefA,
  String pubkey = founderPubkey,
  int createdAt = 1200,
  String? id,
}) => event(
  kind: EventKind.codingSessionName,
  pubkey: pubkey,
  createdAt: createdAt,
  id: id,
  tags: [
    ['h', channelId],
    ['d', sessionRef],
    ['csnm-v', 'csnm1-1'],
  ],
  content: content,
);

/// A signed 44252 generated title, by default from [providerPubkey] for
/// generation 1 of the default [target].
NostrEvent generatedTitleEvent({
  required String title,
  String sessionRef = sessionRefA,
  String pubkey = providerPubkey,
  CodingSessionTarget? forTarget,
  String model = 'claude-haiku-4-5',
  int createdAt = 1100,
  String? id,
}) => event(
  kind: EventKind.codingSessionGeneratedTitle,
  pubkey: pubkey,
  createdAt: createdAt,
  id: id,
  tags: [
    ['h', channelId],
    ['d', sessionRef],
    ['cstl-v', 'cstl1-1'],
    ['cs-target', (forTarget ?? target()).key],
  ],
  content: jsonEncode({
    'schema': 'buzz-coding-session-title/v1',
    'title': title,
    'model': model,
    'basis': 'first-message',
    'sourceCommand': null,
    'createEventId': 'ca' * 32,
  }),
);

/// The receipt-joined create that makes [founderPubkey] the legacy founder
/// of an umbrella whose execution is the default [target].
///
/// A person's name is the *founder's* 44229 (the shared display-name rule),
/// so a fold test that expects a name has to say whose session it is.
({List<CodingSessionCreate> creates, Map<String, String> targetKeyByCommandId})
foundedByFounder({String commandId = 'cmd-1', String? sessionRef}) => (
  creates: [
    decodeCodingSessionCreate(
      createEvent(commandId: commandId, sessionRef: sessionRef),
    ).value!,
  ],
  targetKeyByCommandId: {commandId: target().key},
);

/// A signed 44227 goal event.
NostrEvent goalEvent({
  required String content,
  String sessionRef = sessionRefA,
  String pubkey = founderPubkey,
  int createdAt = 1200,
}) => event(
  kind: EventKind.codingSessionGoal,
  pubkey: pubkey,
  createdAt: createdAt,
  tags: [
    ['h', channelId],
    ['d', sessionRef],
    ['csgl-v', 'csgl1-1'],
  ],
  content: content,
);

/// A signed 44230 closure event.
NostrEvent closureEvent({
  String sessionRef = sessionRefA,
  String genesisRef = genesisEventIdA,
  bool closed = true,
  String pubkey = founderPubkey,
  int createdAt = 1300,
}) => event(
  kind: EventKind.codingSessionClosure,
  pubkey: pubkey,
  createdAt: createdAt,
  tags: [
    ['h', channelId],
    ['d', sessionRef],
    ['cscl-v', 'cscl1-1'],
    ['cscl-genesis', genesisRef],
  ],
  content: jsonEncode({
    'action': closed ? 'closed' : 'open',
    'genesisRef': genesisRef,
    'sessionRef': sessionRef,
    'v': 1,
  }),
);

/// A signed 24223 lease event.
NostrEvent leaseEvent({
  CodingSessionTarget? forTarget,
  String state = 'live',
  int leaseSequence = 1,
  String commandId = 'cmd-1',
  String pubkey = providerPubkey,
  int createdAt = 1400,
  String? id,
}) {
  final resolved = forTarget ?? target();
  return event(
    kind: EventKind.codingSessionLease,
    pubkey: pubkey,
    createdAt: createdAt,
    id: id,
    tags: [
      ['h', channelId],
      ['cslease-v', 'cslease1-1'],
      ['cs-target', resolved.key],
      ['csl-command', commandId],
      ['cslease-seq', '$leaseSequence'],
    ],
    content: jsonEncode({
      'schema': 'buzz-coding-session-lease/v1',
      'target': resolved.toJson(),
      'state': state,
      'leaseSequence': leaseSequence,
    }),
  );
}
