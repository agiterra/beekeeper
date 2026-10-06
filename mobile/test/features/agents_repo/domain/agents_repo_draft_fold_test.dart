import 'dart:convert';
import 'dart:io';

import 'package:beekeeper/features/agents_repo/domain/agents_repo_draft_fold.dart';
import 'package:beekeeper/features/agents_repo/domain/agents_repo_draft_op.dart';
import 'package:beekeeper/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

/// The Dart fold binds to `conformance/agents-repo-draft-fold/fixtures/
/// fold-vectors.json` byte for byte.
void main() {
  final vectors =
      jsonDecode(
            File(
              '../conformance/agents-repo-draft-fold/fixtures/fold-vectors.json',
            ).readAsStringSync(),
          )
          as Map<String, dynamic>;

  test('vectors carry the expected schema', () {
    expect(vectors['schema'], 'buzz-agents-repo-draft-fold-vectors/v2');
    expect((vectors['cases'] as List<dynamic>).isNotEmpty, isTrue);
  });

  for (final raw in vectors['cases'] as List<dynamic>) {
    final vector = raw as Map<String, dynamic>;
    test(vector['name'] as String, () {
      final events = [
        for (final e in vector['events'] as List<dynamic>)
          NostrEvent.fromJson({...e as Map<String, dynamic>, 'sig': ''}),
      ];
      final digest = foldAgentsRepoDrafts(
        vector['project'] as String,
        vector['repo'] as String,
        events,
      );
      expect(digest.toJson(), equals(vector['expected']));
      expect(jsonEncode(digest.toJson()), jsonEncode(vector['expected']));
    });
  }

  test('the op codec round-trips and refuses what the validator refuses', () {
    final repo = '30617:${'a' * 64}:tank-loop-beekeeper-agents';
    final put = AgentsRepoDraftOp.filePut(
      repo: repo,
      path: 'plans/rpg.md',
      text: '# RPG\n',
      base: null,
      baseCommit: null,
      prev: null,
      message: 'why',
    );
    expect(decodeAgentsRepoDraftOp(put.toContent(), repo), equals(put));
    expect(put.tags('30621:${'a' * 64}:tank-loop').last, [
      'ad-path',
      'plans/rpg.md',
    ]);
    final move = AgentsRepoDraftOp.fileMove(
      repo: repo,
      path: 'roles/poker.md',
      to: 'roles/archive/poker.md',
      base: '1' * 40,
      baseCommit: null,
      prev: null,
    );
    expect(decodeAgentsRepoDraftOp(move.toContent(), repo), equals(move));
    expect(move.namedPaths, ['roles/poker.md', 'roles/archive/poker.md']);
    final bad = jsonDecode(put.toContent()) as Map<String, Object?>;
    bad['mode'] = '100755';
    expect(decodeAgentsRepoDraftOp(jsonEncode(bad), repo), isNull);
    final missing = jsonDecode(put.toContent()) as Map<String, Object?>;
    missing.remove('prev');
    expect(decodeAgentsRepoDraftOp(jsonEncode(missing), repo), isNull);
    expect(draftPathClass('roles/lead.md'), DraftPathClass.role);
    expect(
      draftPathClass('roles/archive/lead.md'),
      DraftPathClass.archivedRole,
    );
    expect(draftPathClass('docs/x.md'), DraftPathClass.document);
    expect(draftPathClass('docs/img/x.png'), DraftPathClass.documentAsset);
    expect(draftPathClass('docs/x/.gitkeep'), DraftPathClass.documentFolder);
    expect(draftPathClass('docs/x.txt'), isNull);
    expect(draftPathClass('roles/../x'), isNull);
    expect(archiveCounterpart('plans/rpg.md'), 'plans/archive/rpg.md');
    expect(archiveCounterpart('team.yml'), isNull);
    expect(draftTextError('a\u0000b'), isNotNull);
    expect(draftTextError('a\n\tb\r\n'), isNull);
  });
}
