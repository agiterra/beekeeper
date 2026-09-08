import 'package:nostr/nostr.dart' as nostr;

import 'nostr_models.dart';
import 'relay_session.dart';

/// Signs and submits Nostr events through the relay WebSocket connection.
class SignedEventRelay {
  final RelaySessionNotifier _session;
  final String? _nsec;

  SignedEventRelay({
    required RelaySessionNotifier session,
    required String? nsec,
  }) : _session = session,
       _nsec = nsec;

  /// The hex pubkey derived from the signing key, or null if no key.
  String? get pubkey {
    final nsec = _nsec;
    if (nsec == null || nsec.isEmpty) return null;
    final privkeyHex = nostr.Nip19.decode(payload: nsec).data;
    if (privkeyHex.isEmpty) return null;
    return nostr.Keys(privkeyHex).public;
  }

  /// Sign and submit an event. Returns the relay's OK response as a [NostrEvent]
  /// whose `content` field contains the OK message (e.g. `"response:{...}"`
  /// for command kinds).
  Future<NostrEvent> submit({
    required int kind,
    required String content,
    required List<List<String>> tags,
    int? createdAt,
    void Function(NostrEvent event)? onSigned,
  }) async {
    final nsec = _nsec;
    if (nsec == null || nsec.isEmpty) {
      throw Exception('Cannot submit event: no signing key available');
    }

    final privkeyHex = nostr.Nip19.decode(payload: nsec).data;
    if (privkeyHex.isEmpty) {
      throw Exception('Invalid nsec');
    }

    final event = nostr.Event.from(
      kind: kind,
      content: content,
      tags: tags,
      secretKey: privkeyHex,
      createdAt: createdAt,
      verify: false,
    );

    final nostrEvent = NostrEvent.fromJson(event.toMap());
    onSigned?.call(nostrEvent);
    return _session.publish(nostrEvent);
  }

  /// Sign an ephemeral event and hand it to the session's droppable path.
  ///
  /// Returns `false` — and sends nothing — when there is no signing key, the
  /// key does not parse, or [RelaySessionNotifier.sendEphemeral] declined
  /// (socket down, rate-limit gate active, or the ephemeral lane dry). Never
  /// throws and never waits for an OK: this is for beats and indicators the
  /// next tick will simply try again.
  bool sendEphemeral({
    required int kind,
    required String content,
    required List<List<String>> tags,
  }) {
    final nsec = _nsec;
    if (nsec == null || nsec.isEmpty) return false;
    try {
      final privkeyHex = nostr.Nip19.decode(payload: nsec).data;
      if (privkeyHex.isEmpty) return false;
      final event = nostr.Event.from(
        kind: kind,
        content: content,
        tags: tags,
        secretKey: privkeyHex,
        verify: false,
      );
      return _session.sendEphemeral(NostrEvent.fromJson(event.toMap()));
    } catch (_) {
      return false;
    }
  }
}
