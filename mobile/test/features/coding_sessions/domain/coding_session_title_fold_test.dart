import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/features/coding_sessions/state/coding_session_event_store.dart';
import 'package:buzz/shared/relay/nostr_filters.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

import 'coding_session_fixtures.dart';

/// SV-31 through the whole mobile read: the trust gate decodes 44252, the
/// fold ranks it with the shared rule, and the store and filters keep it.
void main() {
  setUp(resetEventIds);

  /// A founded, started umbrella: the founder's create, its provider's
  /// receipt, and the provider's metadata carrying the operator title.
  List<NostrEvent> started() => [
    createEvent(commandId: 'cmd-1', sessionRef: sessionRefA),
    receiptEvent(commandId: 'cmd-1', status: 'created'),
    metadataEvent(sessionRef: sessionRefA, title: 'Fix the login redirect'),
  ];

  CodingSessionChannelView read(List<NostrEvent> events) =>
      readCodingSessionChannel(
        channelId: channelId,
        verifier: null,
        events: events,
      );

  test('a title its own provider generated names an unnamed session', () {
    final view = read([
      ...started(),
      generatedTitleEvent(title: 'Login redirect fix'),
    ]);
    final session = view.sessions.single;
    expect(session.displayName, 'Login redirect fix');
    expect(session.resolvedName.origin, CodingSessionNameOrigin.generated);
    expect(session.resolvedName.model, 'claude-haiku-4-5');
    expect(session.resolvedName.signerPubkey, providerPubkey);
    expect(session.resolvedName.targetKey, target().key);
    expect(session.name, isNull, reason: 'a model\'s words are no one\'s name');
    expect(view.facts.titles, hasLength(1));
    expect(view.counts.isClean, isTrue);
  });

  test('the founder\'s name wins however old, and is the rename text', () {
    final view = read([
      ...started(),
      nameEvent(content: 'Auth rework', createdAt: 950),
      generatedTitleEvent(title: 'Login redirect fix', createdAt: 1100),
    ]);
    final session = view.sessions.single;
    expect(session.displayName, 'Auth rework');
    expect(session.resolvedName.origin, CodingSessionNameOrigin.person);
    expect(session.resolvedName.model, isNull);
    expect(session.name, 'Auth rework');
  });

  test('a title from a signer that is no provider of the umbrella is '
      'ignored and counted', () {
    final view = read([
      ...started(),
      generatedTitleEvent(
        title: 'Not yours to name',
        pubkey: otherProviderPubkey,
      ),
    ]);
    final session = view.sessions.single;
    expect(session.displayName, 'Fix the login redirect');
    expect(session.resolvedName.origin, CodingSessionNameOrigin.fallback);
    expect(session.resolvedName.diagnostics.foreignTitles, 1);
  });

  test('a title from an unverified fallback authority is foreign: a '
      'stranger\'s made-up execution cannot name the umbrella', () {
    // A channel member invents a cs-target, signs a lifecycle receipt for it
    // under a command no create issued (an execution exists only when a
    // receipt names it), claims the umbrella with a 44223 — no create names
    // that target, so the trust gate's fallback hands it to them, unverified
    // — then titles it. Standing needs a verified authority: a create its
    // own provider receipt-joined. So the umbrella keeps its fallback.
    final forged = target(instanceId: 'forged', sessionId: 'forged');
    final view = read([
      ...started(),
      receiptEvent(
        commandId: 'cmd-forged',
        status: 'created',
        forTarget: forged,
        pubkey: otherProviderPubkey,
        createdAt: 1040,
      ),
      metadataEvent(
        forTarget: forged,
        pubkey: otherProviderPubkey,
        sessionRef: sessionRefA,
        createdAt: 1050,
      ),
      generatedTitleEvent(
        title: 'Pwned',
        pubkey: otherProviderPubkey,
        forTarget: forged,
      ),
    ]);
    final session = view.sessions.single;
    expect(
      session.executions.map((execution) => execution.targetKey),
      contains(forged.key),
      reason: 'the forged execution does join the umbrella',
    );
    final forgedExecution = session.executions.firstWhere(
      (execution) => execution.targetKey == forged.key,
    );
    expect(forgedExecution.authority.verified, isFalse);
    expect(session.displayName, 'Fix the login redirect');
    expect(session.resolvedName.origin, CodingSessionNameOrigin.fallback);
    expect(session.resolvedName.diagnostics.foreignTitles, 1);
  });

  test('a 44229 from someone other than the founder is never the name', () {
    final view = read([
      ...started(),
      nameEvent(content: 'Hijacked', pubkey: otherFounderPubkey),
      generatedTitleEvent(title: 'Login redirect fix'),
    ]);
    final session = view.sessions.single;
    expect(session.displayName, 'Login redirect fix');
    expect(session.name, isNull);
    expect(session.resolvedName.diagnostics.foreignNames, 1);
  });

  test('the earliest title wins, so a shown title never flips', () {
    final view = read([
      ...started(),
      generatedTitleEvent(title: 'Later title', createdAt: 1300),
      generatedTitleEvent(title: 'First title', createdAt: 1100),
    ]);
    expect(view.sessions.single.displayName, 'First title');
  });

  test('nothing names it and no title: the shared fallback', () {
    final view = read([
      createEvent(commandId: 'cmd-1', sessionRef: sessionRefA),
      receiptEvent(commandId: 'cmd-1', status: 'created'),
      metadataEvent(sessionRef: sessionRefA),
    ]);
    expect(view.sessions.single.displayName, codingSessionUntitledName);
  });

  test('a malformed 44252 is refused by the gate and counted by kind', () {
    final bad = generatedTitleEvent(title: 'Two\nlines');
    final view = read([...started(), bad]);
    expect(view.facts.titles, isEmpty);
    expect(
      view.counts.malformedByKind[EventKind.codingSessionGeneratedTitle],
      1,
    );
    expect(view.sessions.single.displayName, 'Fix the login redirect');
  });

  test('the generated-title filter carries explicit kinds and #h', () {
    final filter = NostrFilters.codingSessionGeneratedTitles(channelId);
    expect(filter.kinds, [EventKind.codingSessionGeneratedTitle]);
    expect(filter.kinds, [44252]);
    expect(filter.tags['#h'], [channelId]);
    expect(filter.authors, isNull, reason: 'standing is the fold\'s call');
  });

  test('a transcript flood does not evict its generation\'s title', () {
    final store = CodingSessionEventStore(cap: 3);
    final titleEvent = generatedTitleEvent(
      title: 'Login redirect fix',
      createdAt: 1,
    );
    store.add(titleEvent);
    for (var seq = 1; seq <= 8; seq++) {
      store.add(
        transcriptEvent(
          eventSeq: seq,
          item: const {'kind': 'assistant_text', 'text': 'x'},
        ),
      );
    }
    expect(store.events.map((event) => event.id), contains(titleEvent.id));
  });
}
