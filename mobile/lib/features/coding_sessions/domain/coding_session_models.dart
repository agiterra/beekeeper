import 'package:flutter/foundation.dart';

import 'coding_session_target.dart';

/// The ten statuses a 44223 metadata payload may report.
enum CodingSessionStatus {
  starting('starting'),
  idle('idle'),
  running('running'),
  waitingForInput('waiting_for_input'),
  completed('completed'),
  stopped('stopped'),
  failed('failed'),
  interrupted('interrupted'),
  disconnected('disconnected'),
  unknown('unknown');

  const CodingSessionStatus(this.wire);

  /// The exact string the provider signs.
  final String wire;

  /// Decode a wire status, or `null` when it is not one of the ten.
  static CodingSessionStatus? fromWire(Object? value) {
    if (value is! String) return null;
    for (final status in CodingSessionStatus.values) {
      if (status.wire == value) return status;
    }
    return null;
  }

  /// True for a status that claims the provider is doing or awaiting work.
  ///
  /// These are the statuses that a stale lease turns into
  /// "No provider answering": a claim about the present, not about the past.
  bool get isLiveSounding =>
      this == CodingSessionStatus.starting ||
      this == CodingSessionStatus.running ||
      this == CodingSessionStatus.waitingForInput;

  /// True for the one status that ends an execution.
  bool get isStopped => this == CodingSessionStatus.stopped;
}

/// Every status a 44224 lifecycle receipt may carry.
///
/// The turn statuses report what happened to one 44220 `thread.turn.start`;
/// they never create, confirm, or end a generation, which is why every fold
/// that reads a receipt to decide a generation's state skips them
/// ([isTurnStage]).
enum CodingSessionReceiptStatus {
  created('created'),
  createdWithFailedInitialTurn('created_with_failed_initial_turn'),
  failed('failed'),
  resumed('resumed'),
  resumedWithoutContext('resumed_without_context'),
  stopped('stopped'),
  continuationRegistered('continuation_registered'),
  turnQueued('turn_queued'),
  turnStarted('turn_started'),

  /// A `deliver: "steer"` input the runtime acknowledged as joined into the
  /// turn already running. Carries that turn's `turnId`; no new turn begins.
  turnInjected('turn_injected'),
  turnDegraded('turn_degraded'),

  /// A native steer was written to the runtime and its delivery could not be
  /// established. Terminal — never replayed by the provider — and neither a
  /// drop nor a refusal: the words may already be inside the running turn.
  turnDeliveryUnknown('turn_delivery_unknown'),
  turnDropped('turn_dropped'),
  turnRefused('turn_refused'),
  interruptDelivered('interrupt_delivered');

  const CodingSessionReceiptStatus(this.wire);

  /// The exact string the provider signs.
  final String wire;

  /// Decode a wire status, or `null` when it is not recognized.
  static CodingSessionReceiptStatus? fromWire(Object? value) {
    if (value is! String) return null;
    for (final status in CodingSessionReceiptStatus.values) {
      if (status.wire == value) return status;
    }
    return null;
  }

  /// True for a per-turn stage rather than a generation lifecycle change.
  bool get isTurnStage =>
      this == CodingSessionReceiptStatus.continuationRegistered ||
      this == CodingSessionReceiptStatus.turnQueued ||
      this == CodingSessionReceiptStatus.turnStarted ||
      this == CodingSessionReceiptStatus.turnInjected ||
      this == CodingSessionReceiptStatus.turnDegraded ||
      this == CodingSessionReceiptStatus.turnDeliveryUnknown ||
      this == CodingSessionReceiptStatus.turnDropped ||
      this == CodingSessionReceiptStatus.turnRefused ||
      this == CodingSessionReceiptStatus.interruptDelivered;

  /// True for the two six-key statuses: `turn_started` names the turn that
  /// began, `turn_injected` the running turn the input joined.
  bool get carriesTurnId =>
      this == CodingSessionReceiptStatus.turnStarted ||
      this == CodingSessionReceiptStatus.turnInjected;

  /// True for the four statuses that bring a generation into existence.
  bool get createsGeneration =>
      this == CodingSessionReceiptStatus.created ||
      this == CodingSessionReceiptStatus.createdWithFailedInitialTurn ||
      this == CodingSessionReceiptStatus.resumed ||
      this == CodingSessionReceiptStatus.resumedWithoutContext;
}

/// The two states a 24223 lease may declare.
enum CodingSessionLeaseState {
  live('live'),
  released('released');

  const CodingSessionLeaseState(this.wire);

  final String wire;

  static CodingSessionLeaseState? fromWire(Object? value) {
    if (value is! String) return null;
    for (final state in CodingSessionLeaseState.values) {
      if (state.wire == value) return state;
    }
    return null;
  }
}

/// Envelope facts every decoded coding-session record carries.
@immutable
class CodingSessionEventRef {
  /// The channel the event was published in (`h` tag).
  final String channelId;

  /// The event id, lowercase hex.
  final String eventId;

  /// The signer, lowercase hex.
  final String signerPubkey;

  /// `created_at`, in epoch seconds.
  final int createdAt;

  const CodingSessionEventRef({
    required this.channelId,
    required this.eventId,
    required this.signerPubkey,
    required this.createdAt,
  });

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is CodingSessionEventRef &&
          channelId == other.channelId &&
          eventId == other.eventId &&
          signerPubkey == other.signerPubkey &&
          createdAt == other.createdAt;

  @override
  int get hashCode => Object.hash(channelId, eventId, signerPubkey, createdAt);
}

/// A decoded 44223 per-generation metadata fact.
@immutable
class CodingSessionMetadata {
  final CodingSessionEventRef ref;
  final CodingSessionTarget target;
  final CodingSessionStatus status;

  /// When the provider reported [status], in epoch seconds.
  ///
  /// Derived from the event's `created_at`: the payload carries no timestamp
  /// of its own, and the desktop reads exactly the same field
  /// (`sessionCoordinationFold.ts`, `statusAt: observed?.event.created_at`).
  int get statusAt => ref.createdAt;

  final String? projectRef;
  final String? repoRef;
  final String? title;
  final String? agentRef;
  final String? provider;
  final String? runtime;
  final String? model;
  final String? branch;
  final String? sessionRef;
  final String? contextSummary;
  final String? diffSummary;
  final String? planSummary;

  /// The provider's capability flags, read as opaque booleans.
  final Map<String, bool> capabilities;

  /// The whole validated payload in canonical (key-sorted) JSON form.
  ///
  /// Captured at decode so D6's same-second conflict test compares what the
  /// provider actually signed rather than the subset of keys this class
  /// models. Fields the decoder validates and then deliberately drops — the
  /// B1 code coordinates, for one — still differ here, which no
  /// hand-maintained list of field names could guarantee.
  final String canonicalPayload;

  const CodingSessionMetadata({
    required this.ref,
    required this.target,
    required this.status,
    required this.capabilities,
    required this.canonicalPayload,
    this.projectRef,
    this.repoRef,
    this.title,
    this.agentRef,
    this.provider,
    this.runtime,
    this.model,
    this.branch,
    this.sessionRef,
    this.contextSummary,
    this.diffSummary,
    this.planSummary,
  });

  /// A short human label for the execution this metadata describes.
  ///
  /// Derived, not signed: the wire payload has no `label` key. Preference
  /// order is the provider's own `agentRef`, then `runtime · model`, then
  /// whichever of the two is present, then the target's driver — every one of
  /// them a fact the provider signed, never an invented name.
  String get label {
    final agent = agentRef;
    if (agent != null && agent.trim().isNotEmpty) return agent;
    final parts = [
      if (runtime != null && runtime!.trim().isNotEmpty) runtime!,
      if (model != null && model!.trim().isNotEmpty) model!,
    ];
    if (parts.isNotEmpty) return parts.join(' · ');
    return target.driver;
  }
}

/// The `{code, message}` an error-carrying receipt reports.
@immutable
class CodingSessionReceiptError {
  final String code;
  final String message;

  const CodingSessionReceiptError({required this.code, required this.message});
}

/// A decoded 44224 lifecycle or turn receipt.
@immutable
class CodingSessionReceipt {
  final CodingSessionEventRef ref;
  final String commandId;
  final CodingSessionReceiptStatus status;

  /// The execution the receipt names; `null` only for `failed`.
  final CodingSessionTarget? session;

  final CodingSessionReceiptError? error;

  /// The provider's own turn id; present only for `turn_started` and
  /// `turn_injected` ([CodingSessionReceiptStatus.carriesTurnId]).
  final String? turnId;

  const CodingSessionReceipt({
    required this.ref,
    required this.commandId,
    required this.status,
    required this.session,
    this.error,
    this.turnId,
  });

  /// True for a per-turn stage; see [CodingSessionReceiptStatus.isTurnStage].
  bool get isTurnStage => status.isTurnStage;
}

/// A decoded 44225 transcript envelope.
@immutable
class CodingSessionTranscriptEnvelope {
  final CodingSessionEventRef ref;
  final CodingSessionTarget target;

  /// Per-generation sequence, strictly positive; ordering is numeric.
  final int eventSeq;

  /// The provider's timestamp for the item, in epoch milliseconds.
  final int timestamp;

  /// The provider's own turn id, or `null` for "belongs to no turn".
  final String? turnId;

  /// The item payload, kept opaque past its bounded `kind`.
  final Map<String, dynamic> item;

  const CodingSessionTranscriptEnvelope({
    required this.ref,
    required this.target,
    required this.eventSeq,
    required this.timestamp,
    required this.turnId,
    required this.item,
  });

  /// The item's `kind` discriminant.
  String get itemKind => item['kind'] as String;
}

/// A decoded 44226 genesis: the immutable anchor of an umbrella session.
@immutable
class CodingSessionGenesis {
  final CodingSessionEventRef ref;
  final String sessionRef;

  /// The signer of the genesis — the umbrella's founder.
  String get founderPubkey => ref.signerPubkey;

  const CodingSessionGenesis({required this.ref, required this.sessionRef});
}

/// A decoded 44221 `session.create` command.
@immutable
class CodingSessionCreate {
  final CodingSessionEventRef ref;
  final String commandId;

  /// The provider this create addressed; the only signer whose facts count.
  final String providerAuthorityPubkey;

  final String? sessionRef;
  final String? genesisRef;
  final String providerInstanceRef;
  final String? projectRef;
  final String? repoRef;
  final String? model;
  final String? title;

  const CodingSessionCreate({
    required this.ref,
    required this.commandId,
    required this.providerAuthorityPubkey,
    required this.providerInstanceRef,
    this.sessionRef,
    this.genesisRef,
    this.projectRef,
    this.repoRef,
    this.model,
    this.title,
  });
}

/// A decoded 44221 `session.resume` command.
///
/// A resume names an existing generation and the provider allowed to answer
/// it; the provider's lifecycle receipt mints the *next* generation. It is
/// read for one reason only: without it the generation a resume produced has
/// no create-backed authority, so any signer whose metadata landed first
/// becomes its fallback authority. It claims no umbrella and founds nothing.
@immutable
class CodingSessionResume {
  final CodingSessionEventRef ref;
  final String commandId;

  /// The provider this resume addressed; the only signer whose facts count.
  final String providerAuthorityPubkey;

  /// The generation the resume asked to reattach to.
  final CodingSessionTarget session;

  const CodingSessionResume({
    required this.ref,
    required this.commandId,
    required this.providerAuthorityPubkey,
    required this.session,
  });
}

/// A decoded 44229 umbrella-session display name.
@immutable
class CodingSessionName {
  final CodingSessionEventRef ref;
  final String sessionRef;
  final String content;

  const CodingSessionName({
    required this.ref,
    required this.sessionRef,
    required this.content,
  });
}

/// A decoded 44227 umbrella-session goal.
@immutable
class CodingSessionGoal {
  final CodingSessionEventRef ref;
  final String sessionRef;
  final String content;

  const CodingSessionGoal({
    required this.ref,
    required this.sessionRef,
    required this.content,
  });
}

/// A decoded 44230 umbrella-session closure marker.
@immutable
class CodingSessionClosure {
  final CodingSessionEventRef ref;
  final String sessionRef;
  final String genesisRef;

  /// `closed` marks the session closed; `open` reopens it.
  final bool closed;

  const CodingSessionClosure({
    required this.ref,
    required this.sessionRef,
    required this.genesisRef,
    required this.closed,
  });
}

/// A decoded 24223 provider lease.
@immutable
class CodingSessionLease {
  final CodingSessionEventRef ref;
  final CodingSessionTarget target;
  final String commandId;
  final CodingSessionLeaseState state;

  /// The provider's monotonic lease sequence; strictly positive.
  final int leaseSequence;

  const CodingSessionLease({
    required this.ref,
    required this.target,
    required this.commandId,
    required this.state,
    required this.leaseSequence,
  });
}
