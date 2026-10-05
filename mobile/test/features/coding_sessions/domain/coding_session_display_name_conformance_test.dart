import 'dart:convert';
import 'dart:io';

import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

/// The mobile display-name rule binds to `conformance/session-display-name/
/// fixtures/vectors.json` (NIP-CSG § Generated title; CONTRACT.md). Every
/// envelope goes through the real 44252 decoder; every vector's events go
/// through the real 44229/44252 decoders and the real resolver, forwards and
/// reversed, and must produce exactly the expected name, origin, model,
/// signer and diagnostics.
void main() {
  final fixture =
      jsonDecode(
            File(
              '../conformance/session-display-name/fixtures/vectors.json',
            ).readAsStringSync(),
          )
          as Map<String, dynamic>;

  test('vectors carry the expected schema and this build\'s constants', () {
    expect(fixture['schema'], 'buzz.conformance/session-display-name@1');
    final constants = fixture['constants'] as Map<String, dynamic>;
    expect(constants['kindName'], EventKind.codingSessionName);
    expect(
      constants['kindGeneratedTitle'],
      EventKind.codingSessionGeneratedTitle,
    );
    expect(constants['tagVersion'], codingSessionTitleTagVersion);
    expect(constants['payloadSchema'], codingSessionTitleSchema);
    expect(constants['maxTitleBytes'], maxCodingSessionNameBytes);
    expect(constants['maxContentBytes'], maxCodingSessionTitleContentBytes);
    expect(constants['maxModelBytes'], maxCodingSessionTitleModelBytes);
    expect(constants['untitled'], codingSessionUntitledName);
    expect((fixture['vectors'] as List<dynamic>).isNotEmpty, isTrue);
    expect((fixture['envelopes'] as List<dynamic>).isNotEmpty, isTrue);
  });

  group('envelopes', () {
    for (final raw in fixture['envelopes'] as List<dynamic>) {
      final envelope = raw as Map<String, dynamic>;
      test(envelope['name'] as String, () {
        final event = _event(envelope['event'] as Map<String, dynamic>);
        final decoded = decodeCodingSessionGeneratedTitle(event);
        expect(
          decoded.isValid,
          envelope['valid'],
          reason: '${envelope['description']} (${decoded.reason})',
        );
      });
    }
  });

  group('vectors', () {
    for (final raw in fixture['vectors'] as List<dynamic>) {
      final vector = raw as Map<String, dynamic>;
      test(vector['name'] as String, () {
        final scope = _scope(vector['scope'] as Map<String, dynamic>);
        final events = [
          for (final event in vector['events'] as List<dynamic>)
            _event(event as Map<String, dynamic>),
        ];
        final expected = vector['expected'] as Map<String, dynamic>;
        for (final (order, ordered) in [
          ('forwards', events),
          ('reversed', events.reversed.toList()),
        ]) {
          final resolved = resolveCodingSessionDisplayNameFromEvents(
            scope: scope,
            events: ordered,
          );
          final diagnostics = expected['diagnostics'] as Map<String, dynamic>;
          expect(
            {
              'name': resolved.name,
              'origin': resolved.origin.wire,
              'model': resolved.model,
              'signerPubkey': resolved.signerPubkey,
              'foreignNames': resolved.diagnostics.foreignNames,
              'foreignTitles': resolved.diagnostics.foreignTitles,
              'malformed': resolved.diagnostics.malformed,
            },
            {
              'name': expected['name'],
              'origin': expected['origin'],
              'model': expected['model'],
              'signerPubkey': expected['signerPubkey'],
              'foreignNames': diagnostics['foreignNames'],
              'foreignTitles': diagnostics['foreignTitles'],
              'malformed': diagnostics['malformed'],
            },
            reason: '${vector['description']} ($order)',
          );
        }
      });
    }
  });
}

CodingSessionDisplayNameScope _scope(Map<String, dynamic> scope) =>
    CodingSessionDisplayNameScope(
      channelId: scope['channelId'] as String,
      sessionRef: scope['sessionRef'] as String,
      founderPubkey: scope['founderPubkey'] as String?,
      foundingExecutionTitle: scope['foundingExecutionTitle'] as String?,
      executions: [
        for (final raw in scope['executions'] as List<dynamic>)
          CodingSessionTitleAuthority(
            targetKey: (raw as Map<String, dynamic>)['targetKey'] as String,
            providerAuthorityPubkey: raw['providerAuthorityPubkey'] as String,
          ),
      ],
    );

/// A vector event as this app receives one. Vectors carry no `sig`; the rule
/// runs after signature verification, so none is checked here.
NostrEvent _event(Map<String, dynamic> json) =>
    NostrEvent.fromJson({...json, 'sig': ''});
