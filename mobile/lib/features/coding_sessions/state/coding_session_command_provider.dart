import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/relay/relay.dart';
import '../domain/coding_sessions_domain.dart';
import 'pending_creates_provider.dart';
import 'pending_turns_provider.dart';

/// A publish the relay or this device refused, with the reason verbatim.
///
/// The relay's OK message (`restricted: …`, `rate-limited: …`, `invalid: …`)
/// is the authority on why; it is never rewritten, only shown.
class CodingSessionPublishException implements Exception {
  final String message;

  const CodingSessionPublishException(this.message);

  @override
  String toString() => message;
}

/// What a sent turn left behind: the row's store key, so the page can watch
/// it settle.
class CodingSessionSentTurn {
  final String pendingKey;
  final String commandId;

  const CodingSessionSentTurn({
    required this.pendingKey,
    required this.commandId,
  });
}

/// What asking for a new session left behind: the pending row's store key
/// and the umbrella it founded, so the project list can watch it settle.
class CodingSessionRequestedSession {
  final String pendingKey;
  final String commandId;
  final String sessionRef;

  const CodingSessionRequestedSession({
    required this.pendingKey,
    required this.commandId,
    required this.sessionRef,
  });
}

/// The member-signed commands one channel's session pages may publish.
///
/// Every method is `async` so a refusal raised before the relay is reached —
/// a builder bound, a missing genesis — fails the returned future the same
/// way a relay refusal does; callers handle one shape.
///
/// Every method builds the exact event the desktop would (see
/// `coding_session_commands.dart`), signs it with this device's key, and
/// publishes it over the relay WebSocket. None of them wait for a provider:
/// the relay's `OK` is acceptance, and what the provider does with the
/// command is read back as receipts by the observer.
class CodingSessionCommands {
  final String channelId;
  final SignedEventRelay _relay;
  final bool Function() _isDeliveryValid;
  final PendingTurnsNotifier _pending;
  final PendingCreatesNotifier _pendingCreates;
  final DateTime Function() _now;

  CodingSessionCommands({
    required this.channelId,
    required SignedEventRelay relay,
    required bool Function() isDeliveryValid,
    required PendingTurnsNotifier pending,
    required PendingCreatesNotifier pendingCreates,
    DateTime Function() now = DateTime.now,
  }) : _relay = relay,
       _isDeliveryValid = isDeliveryValid,
       _pending = pending,
       _pendingCreates = pendingCreates,
       _now = now;

  /// Ask [providerAuthorityPubkey]'s [providerInstanceRef] to create a session
  /// in this channel — the desktop's own single-session launch, signed here.
  ///
  /// Three publishes in the desktop's order (`prepareNewCodingSessionCreate`):
  /// the 44226 genesis that founds the umbrella and makes this key its
  /// founder; a 44229 name when [title] is given; then the 44221 create
  /// naming the genesis. The pending row is recorded before the first and
  /// forgotten only if a publish fails — then the caller gets the relay's
  /// words. A genesis that went out before a create that did not is
  /// disclosed in that message: the umbrella exists on the wire, seatless,
  /// which is the same hazard the desktop carries at this step.
  ///
  /// Nothing here waits for the provider. Where the session ends up — or
  /// why it never starts, such as no working directory recorded for the
  /// project on that computer — is read back as its 44224 receipt.
  Future<CodingSessionRequestedSession> createSession({
    required String? projectRef,
    required String? repoRef,
    required String providerInstanceRef,
    required String providerAuthorityPubkey,
    required String? model,
    required String? title,
    required String? initialTurn,
  }) async {
    final sessionRef = createCodingSessionSessionRef();
    final commandId = createCodingSessionLifecycleCommandId();
    final trimmedTitle = title?.trim();
    final namedTitle = trimmedTitle == null || trimmedTitle.isEmpty
        ? null
        : trimmedTitle;
    final trimmedTurn = initialTurn?.trim();
    final firstTurn = trimmedTurn == null || trimmedTurn.isEmpty
        ? null
        : initialTurn;
    final genesis = _build(
      () => buildCodingSessionGenesisEvent(
        channelId: channelId,
        sessionRef: sessionRef,
      ),
    );
    final name = namedTitle == null
        ? null
        : _build(
            () => buildCodingSessionNameEvent(
              channelId: channelId,
              sessionRef: sessionRef,
              name: namedTitle,
            ),
          );
    // Refuse the create's own bounds before anything is signed, so a
    // too-long first prompt never leaves a founded, seatless umbrella.
    _build(
      () => buildCodingSessionCreateEvent(
        channelId: channelId,
        commandId: commandId,
        projectRef: projectRef,
        repoRef: repoRef,
        sessionRef: sessionRef,
        genesisRef: '0' * 64,
        providerInstanceRef: providerInstanceRef,
        providerAuthorityPubkey: providerAuthorityPubkey,
        model: model,
        title: namedTitle,
        initialTurn: firstTurn,
      ),
    );
    final row = CodingSessionPendingCreate(
      channelId: channelId,
      commandId: commandId,
      sessionRef: sessionRef,
      genesisRef: null,
      title: namedTitle,
      providerAuthorityPubkey: providerAuthorityPubkey,
      recordedAt: _now().millisecondsSinceEpoch,
      published: false,
    );
    _pendingCreates.record(row);
    String? genesisRef;
    try {
      genesisRef = await _publish(genesis);
      _pendingCreates.markGenesis(row.key, genesisRef);
      if (name != null) await _publish(name);
      await _publish(
        _build(
          () => buildCodingSessionCreateEvent(
            channelId: channelId,
            commandId: commandId,
            projectRef: projectRef,
            repoRef: repoRef,
            sessionRef: sessionRef,
            genesisRef: genesisRef!,
            providerInstanceRef: providerInstanceRef,
            providerAuthorityPubkey: providerAuthorityPubkey,
            model: model,
            title: namedTitle,
            initialTurn: firstTurn,
          ),
        ),
      );
    } on CodingSessionPublishException catch (error) {
      _pendingCreates.forget(row.key);
      if (genesisRef == null) rethrow;
      throw CodingSessionPublishException(
        '${error.message} The session was founded on the relay but no '
        'provider was asked to run it; nothing will start.',
      );
    }
    _pendingCreates.markPublished(row.key);
    return CodingSessionRequestedSession(
      pendingKey: row.key,
      commandId: commandId,
      sessionRef: sessionRef,
    );
  }

  /// Send a turn to [execution]'s current generation.
  ///
  /// The pending row is recorded *before* the relay is awaited, and forgotten
  /// again only if the publish fails — the one outcome where the words never
  /// left this device. Throws [CodingSessionPublishException] in that case
  /// so the composer can put [draft] back.
  Future<CodingSessionSentTurn> sendTurn({
    required CodingSessionExecution execution,
    required String text,
    required String draft,
    CodingSessionTurnDelivery deliver = CodingSessionTurnDelivery.boundary,
  }) async {
    final commandId = createCodingSessionCommandId();
    final event = _build(
      () => buildCodingSessionTurnStartEvent(
        channelId: channelId,
        commandId: commandId,
        target: execution.target,
        text: text,
        deliver: deliver,
      ),
    );
    final turn = CodingSessionPendingTurn(
      channelId: channelId,
      executionKey: execution.executionKey,
      generation: execution.target.generation,
      commandId: commandId,
      text: text,
      draft: draft,
      recordedAt: _now().millisecondsSinceEpoch,
      published: false,
    );
    _pending.record(turn);
    try {
      await _publish(event);
    } catch (error) {
      _pending.forget(turn.key);
      rethrow;
    }
    _pending.markPublished(turn.key);
    return CodingSessionSentTurn(pendingKey: turn.key, commandId: commandId);
  }

  /// Cancel the turn [execution] is running.
  Future<void> interrupt(CodingSessionExecution execution) async => _publish(
    _build(
      () => buildCodingSessionInterruptEvent(
        channelId: channelId,
        commandId: createCodingSessionCommandId(),
        target: execution.target,
      ),
    ),
  );

  /// Durably stop [execution]. The provider decides whether this identity may.
  Future<void> stop(CodingSessionExecution execution) async => _publish(
    _build(
      () => buildCodingSessionStopEvent(
        channelId: channelId,
        commandId: createCodingSessionLifecycleCommandId(),
        target: execution.target,
        providerAuthorityPubkey: execution.signerPubkey,
      ),
    ),
  );

  /// Publish a new display name for [session].
  Future<void> rename(CodingSessionUmbrella session, String name) async =>
      _publish(
        _build(
          () => buildCodingSessionNameEvent(
            channelId: channelId,
            sessionRef: _sessionRef(session),
            name: name,
          ),
        ),
      );

  /// Publish a new goal for [session].
  Future<void> setGoal(CodingSessionUmbrella session, String goal) async =>
      _publish(
        _build(
          () => buildCodingSessionGoalEvent(
            channelId: channelId,
            sessionRef: _sessionRef(session),
            goal: goal,
          ),
        ),
      );

  /// Mark [session] closed.
  Future<void> close(CodingSessionUmbrella session) =>
      _closure(session, CodingSessionClosureAction.closed);

  /// Mark [session] open again.
  Future<void> reopen(CodingSessionUmbrella session) =>
      _closure(session, CodingSessionClosureAction.open);

  Future<void> _closure(
    CodingSessionUmbrella session,
    CodingSessionClosureAction action,
  ) async {
    final genesisRef = session.founder.genesisRef;
    if (genesisRef == null) {
      throw const CodingSessionPublishException(
        'This session has no readable genesis, so a closure cannot name '
        'the anchor the relay requires.',
      );
    }
    await _publish(
      _build(
        () => buildCodingSessionClosureEvent(
          channelId: channelId,
          sessionRef: _sessionRef(session),
          genesisRef: genesisRef,
          action: action,
        ),
      ),
    );
  }

  String _sessionRef(CodingSessionUmbrella session) {
    final ref = session.sessionRef;
    if (ref == null) {
      throw const CodingSessionPublishException(
        'This session claimed no umbrella, so it has no name, goal or '
        'closure to publish.',
      );
    }
    return ref;
  }

  CodingSessionCommandEvent _build(CodingSessionCommandEvent Function() f) {
    try {
      return f();
    } on CodingSessionCommandError catch (error) {
      throw CodingSessionPublishException('${error.field} ${error.message}');
    }
  }

  /// Sign and publish [event]; returns the signed event's id.
  Future<String> _publish(CodingSessionCommandEvent event) async {
    if (!_isDeliveryValid()) {
      throw const CodingSessionPublishException(
        'The community changed while this was being sent; nothing was '
        'published.',
      );
    }
    String? id;
    try {
      await _relay.submit(
        kind: event.kind,
        content: event.content,
        tags: event.tags,
        onSigned: (signed) => id = signed.id,
      );
    } on CodingSessionPublishException {
      rethrow;
    } catch (error) {
      throw CodingSessionPublishException(_message(error));
    }
    return id!;
  }

  static String _message(Object error) {
    final text = error.toString();
    const prefix = 'Exception: ';
    return text.startsWith(prefix) ? text.substring(prefix.length) : text;
  }
}

/// The commands for one channel, bound to the active community's key.
///
/// Mirrors `sendMessageProvider`: the relay and key are captured when the
/// provider builds, and a community switch mid-send is refused rather than
/// signed under the wrong identity.
final codingSessionCommandsProvider =
    Provider.family<CodingSessionCommands, String>((ref, channelId) {
      final config = ref.watch(relayConfigProvider);
      return CodingSessionCommands(
        channelId: channelId,
        relay: SignedEventRelay(
          session: ref.read(relaySessionProvider.notifier),
          nsec: config.nsec,
        ),
        pending: ref.read(pendingTurnsProvider.notifier),
        pendingCreates: ref.read(pendingCreatesProvider.notifier),
        isDeliveryValid: () {
          final current = ref.read(relayConfigProvider);
          return current.baseUrl == config.baseUrl &&
              current.nsec == config.nsec;
        },
      );
    });
