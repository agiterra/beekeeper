import 'package:nostr/nostr.dart' as nostr;

import '../../../shared/relay/nostr_models.dart';

/// Verdict for one inbound event's signature.
enum CodingSessionSignatureVerdict {
  /// The event id recomputes and the BIP-340 signature checks out.
  valid,

  /// The event id or the signature failed to check.
  invalid,

  /// No verification API was reachable on this device.
  ///
  /// The observer must disclose this rather than trusting silently — see
  /// [CodingSessionSignatureVerifier.available].
  unavailable,
}

/// Verifies the signature of an inbound coding-session event.
///
/// Kept as an interface so the read pipeline can be tested without doing
/// BIP-340 work per event, and so a build on a platform without a verification
/// API can report [available] `== false` instead of silently trusting.
abstract interface class CodingSessionSignatureVerifier {
  /// Whether this device can verify signatures at all.
  bool get available;

  /// Verify one event.
  CodingSessionSignatureVerdict verify(NostrEvent event);
}

/// The default verifier, backed by the `nostr` package.
///
/// `Event(..., verify: false).isValid()` recomputes the canonical event id
/// (sha256 over `[0, pubkey, created_at, kind, tags, content]`) and then runs
/// `Schnorr.verify` (BIP-340) over it — the same two checks the desktop's
/// `hasValidSignature` makes. Both are reachable on mobile, so the observer's
/// "Signatures not verified on this device" disclosure is never needed with
/// this verifier installed; it exists for the case where that stops being
/// true.
final class NostrPackageSignatureVerifier
    implements CodingSessionSignatureVerifier {
  const NostrPackageSignatureVerifier();

  @override
  bool get available => true;

  @override
  CodingSessionSignatureVerdict verify(NostrEvent event) {
    try {
      final candidate = nostr.Event(
        event.id,
        event.pubkey,
        event.createdAt,
        event.kind,
        event.tags,
        event.content,
        event.sig,
        verify: false,
      );
      return candidate.isValid()
          ? CodingSessionSignatureVerdict.valid
          : CodingSessionSignatureVerdict.invalid;
    } on Object {
      // A hostile event (non-hex id, wrong-length signature, a timestamp the
      // package rejects outright) must read as invalid, never as verified.
      return CodingSessionSignatureVerdict.invalid;
    }
  }
}

/// A verifier for platforms with no reachable verification API.
///
/// Every event reads [CodingSessionSignatureVerdict.unavailable] and
/// [available] is `false`, which is what drives the session page's persistent
/// "Signatures not verified on this device" line.
final class UnavailableSignatureVerifier
    implements CodingSessionSignatureVerifier {
  const UnavailableSignatureVerifier();

  @override
  bool get available => false;

  @override
  CodingSessionSignatureVerdict verify(NostrEvent event) =>
      CodingSessionSignatureVerdict.unavailable;
}
