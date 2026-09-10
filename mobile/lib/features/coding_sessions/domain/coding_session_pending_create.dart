import 'package:flutter/foundation.dart';

import 'coding_session_fold.dart';
import 'coding_session_models.dart';

/// A session this device asked a provider to create and has not yet seen
/// answered.
///
/// A row exists from the moment the sheet closes until the provider's 44224
/// names its [commandId] — `created` (or `created_with_failed_initial_turn`)
/// settles it, `failed` reports why in the provider's own words — or until
/// an umbrella claiming its [sessionRef] shows up in the channel's read,
/// whichever comes first. Nothing here is a session: the relay holding the
/// create is not a provider having started one.
@immutable
class CodingSessionPendingCreate {
  final String channelId;

  /// The `commandId` inside the signed 44221.
  final String commandId;

  /// The umbrella the genesis founded and the create names.
  final String sessionRef;

  /// The genesis event id the create names; `null` while the genesis is
  /// still being published.
  final String? genesisRef;

  /// The title the create carried, or `null` for none.
  final String? title;

  /// The provider the create addressed — the only signer whose receipt
  /// answers it.
  final String providerAuthorityPubkey;

  /// When the row was recorded, in epoch milliseconds.
  final int recordedAt;

  /// True once the relay acknowledged the create with `OK true`.
  final bool published;

  const CodingSessionPendingCreate({
    required this.channelId,
    required this.commandId,
    required this.sessionRef,
    required this.genesisRef,
    required this.title,
    required this.providerAuthorityPubkey,
    required this.recordedAt,
    required this.published,
  });

  /// Store key: one row per create in its channel.
  String get key => '$channelId $commandId';

  CodingSessionPendingCreate copyWith({String? genesisRef, bool? published}) =>
      CodingSessionPendingCreate(
        channelId: channelId,
        commandId: commandId,
        sessionRef: sessionRef,
        genesisRef: genesisRef ?? this.genesisRef,
        title: title,
        providerAuthorityPubkey: providerAuthorityPubkey,
        recordedAt: recordedAt,
        published: published ?? this.published,
      );
}

/// Where a pending create stands, read from the channel's accepted facts.
enum CodingSessionPendingCreateKind {
  /// The genesis or the create is still on its way to the relay.
  sending,

  /// The relay holds the create; no provider has answered it.
  awaitingProvider,

  /// The provider minted the execution; the session row takes over.
  created,

  /// The provider refused to create it — [CodingSessionPendingCreatePhase]
  /// carries the code and message it signed.
  failed,
}

/// A pending create's settled phase.
@immutable
class CodingSessionPendingCreatePhase {
  final CodingSessionPendingCreateKind kind;

  /// The provider's refusal, for [CodingSessionPendingCreateKind.failed].
  final CodingSessionReceiptError? error;

  const CodingSessionPendingCreatePhase(this.kind, {this.error});

  bool get isSettled =>
      kind == CodingSessionPendingCreateKind.created ||
      kind == CodingSessionPendingCreateKind.failed;
}

/// Settle [pending] against the channel's accepted lifecycle receipts and
/// its folded sessions.
///
/// Only a receipt signed by the provider the create addressed counts, and
/// only one that names the create's own `commandId` — never a match on
/// title or timing. The umbrella check is the belt to that brace: a provider
/// whose receipt this device missed still shows the session it made. Only an
/// umbrella with an execution counts: this device publishes the genesis
/// before the create, and the founded umbrella that genesis folds into is
/// the create still unanswered, not the session it asked for.
CodingSessionPendingCreatePhase settleCodingSessionPendingCreate(
  CodingSessionPendingCreate pending, {
  required Iterable<CodingSessionReceipt> receipts,
  required Iterable<CodingSessionUmbrella> sessions,
}) {
  for (final session in sessions) {
    if (!session.isFounded && session.sessionRef == pending.sessionRef) {
      return const CodingSessionPendingCreatePhase(
        CodingSessionPendingCreateKind.created,
      );
    }
  }
  CodingSessionReceiptError? refusal;
  for (final receipt in receipts) {
    if (receipt.commandId != pending.commandId ||
        receipt.ref.signerPubkey != pending.providerAuthorityPubkey) {
      continue;
    }
    switch (receipt.status) {
      case CodingSessionReceiptStatus.created:
      case CodingSessionReceiptStatus.createdWithFailedInitialTurn:
        return const CodingSessionPendingCreatePhase(
          CodingSessionPendingCreateKind.created,
        );
      case CodingSessionReceiptStatus.failed:
        refusal = receipt.error;
      default:
        break;
    }
  }
  if (refusal != null) {
    return CodingSessionPendingCreatePhase(
      CodingSessionPendingCreateKind.failed,
      error: refusal,
    );
  }
  return CodingSessionPendingCreatePhase(
    pending.published
        ? CodingSessionPendingCreateKind.awaitingProvider
        : CodingSessionPendingCreateKind.sending,
  );
}
