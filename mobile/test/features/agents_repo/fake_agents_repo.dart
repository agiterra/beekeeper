import 'package:buzz/features/agents_repo/data/agents_repo_http_client.dart';
import 'package:buzz/features/agents_repo/domain/agents_repo_draft_fold.dart';
import 'package:buzz/features/agents_repo/domain/agents_repo_draft_op.dart';
import 'package:buzz/features/agents_repo/state/agents_repo_actions.dart';
import 'package:buzz/features/agents_repo/state/agents_repo_drafts_provider.dart';
import 'package:buzz/features/agents_repo/state/agents_repo_main_provider.dart';
import 'package:buzz/features/agents_repo/state/agents_repo_source_provider.dart';
import 'package:buzz/shared/relay/relay.dart';

import '../../helpers/recording_relay_session.dart';
import '../projects/ui/fake_projects.dart';

const repoAddress = '30621:$testOwner:beekeeper';
const repoCoordinate = '30617:$testOwner:beekeeper-beekeeper-agents';
const repoViewer =
    '11111111222222223333333344444444555555556666666677777777dddddddd';
const repoOther =
    '2222222222222222222222222222222222222222222222222222222222222222';
const repoTip = '5c2bf83980041e5a38a0dbbe881788b7f032d3e5';
const roadmapBlob = '29aadf90f27a03d826fb8d9db2852b3a46c4b89b';
const roadmapText = '# Roadmap\n\nOverworld first.\n';

AgentsRepoSource testSource({
  String path = '.',
  String? ref = 'refs/heads/main',
}) => AgentsRepoSource(
  repo: repoCoordinate,
  owner: testOwner,
  id: 'beekeeper-beekeeper-agents',
  ref: ref,
  sha: ref == null ? 'a' * 40 : null,
  path: path,
);

AgentsRepoListing testListing() => AgentsRepoListing(
  commit: repoTip,
  entries: const [
    AgentsRepoEntry(path: 'README.md', kind: 'blob', oid: 'b1', size: 12),
    AgentsRepoEntry(path: 'team.yml', kind: 'blob', oid: 'b2', size: 40),
    AgentsRepoEntry(path: 'roles/lead.md', kind: 'blob', oid: 'b3', size: 60),
    AgentsRepoEntry(
      path: 'plans/roadmap.md',
      kind: 'blob',
      oid: roadmapBlob,
      size: 30,
    ),
    AgentsRepoEntry(path: 'plans/.gitkeep', kind: 'blob', oid: 'b5', size: 0),
  ],
  fetchedAt: DateTime.now(),
);

DraftRow testDraftRow({
  String id = 'a',
  String author = repoViewer,
  int createdAt = 100,
  DraftOpKind op = DraftOpKind.filePut,
  String path = 'plans/roadmap.md',
  String? to,
  String? text = '# Roadmap\n\nsomething new\n',
  String? base = roadmapBlob,
  String? baseCommit = repoTip,
  String? prev,
  String? message,
}) => DraftRow(
  id: id.padRight(64, '0'),
  author: author,
  createdAt: createdAt,
  op: op,
  path: path,
  to: to,
  text: text,
  base: base,
  baseCommit: baseCommit,
  prev: prev,
  message: message,
);

AgentsRepoDraftsRead testDraftsRead({
  List<DraftPath> paths = const [],
  int ignored = 0,
  bool truncated = false,
  String? error,
}) => AgentsRepoDraftsRead(
  digest: AgentsRepoDraftDigest(
    project: repoAddress,
    repo: repoCoordinate,
    ignored: ignored,
    otherRepo: 0,
    paths: paths,
    commits: const [],
  ),
  truncated: truncated,
  loading: false,
  error: error,
  hasRead: true,
);

class FakeAgentsRepoDraftsNotifier extends AgentsRepoDraftsNotifier {
  final AgentsRepoDraftsRead read;
  int refreshCount = 0;

  FakeAgentsRepoDraftsNotifier(super.key, this.read);

  @override
  AgentsRepoDraftsRead build() => read;

  @override
  Future<void> refresh() async {
    refreshCount++;
  }

  @override
  DraftRow? headOf(String path) => read.digest.byPath(path)?.head;
}

class FakeAgentsRepoListingNotifier extends AgentsRepoListingNotifier {
  final AgentsRepoListing listing;
  FakeAgentsRepoListingNotifier(super.address, this.listing);

  @override
  Future<AgentsRepoListing> build() async => listing;

  @override
  Future<void> refresh() async {}
}

/// Records every write the page asks for, in call order.
class FakeAgentsRepoActions extends AgentsRepoActions {
  final List<String> calls = [];
  Exception? failure;

  FakeAgentsRepoActions()
    : super(
        address: repoAddress,
        repo: repoCoordinate,
        relay: SignedEventRelay(
          session: RecordingRelaySessionNotifier(),
          nsec: null,
        ),
        read: FakeAgentsRepoDraftsNotifier((
          address: repoAddress,
          repo: repoCoordinate,
        ), testDraftsRead()),
      );

  Future<void> _record(String line) async {
    calls.add(line);
    final error = failure;
    if (error != null) throw error;
  }

  @override
  Future<void> saveDraft({
    required String path,
    required String text,
    required String? base,
    required String? baseCommit,
    required String? openedOn,
    required String? message,
    required String Function(String pubkey) authorName,
  }) => _record(
    'saveDraft $path base=$base openedOn=$openedOn message=$message text=$text',
  );

  @override
  Future<void> moveDraft({
    required String path,
    required String base,
    required String? baseCommit,
    required String? openedOn,
    required String? message,
    required String Function(String pubkey) authorName,
  }) => _record('moveDraft $path base=$base openedOn=$openedOn');

  @override
  Future<void> withdraw(String draftId) => _record('withdraw $draftId');
}
