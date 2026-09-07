import 'package:buzz/features/channels/channel.dart';
import 'package:buzz/features/channels/channels_provider.dart';
import 'package:buzz/features/profile/user_cache_provider.dart';
import 'package:buzz/features/profile/user_profile.dart';
import 'package:buzz/features/projects/domain/project_models.dart';
import 'package:buzz/features/projects/state/projects_provider.dart';
import 'package:buzz/features/projects/ui/project_tree.dart';
import 'package:buzz/features/terminals/domain/terminals_domain.dart';
import 'package:buzz/features/terminals/state/terminals_index_provider.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:buzz/shared/relay/relay_provider.dart';
import 'package:hooks_riverpod/misc.dart';

const testOwner =
    'aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd';
const testViewer =
    '11111111222222223333333344444444555555556666666677777777dddddddd';
const testProjectAddress = '30621:$testOwner:beekeeper';

Project testProject({
  String dtag = 'beekeeper',
  String name = 'Beekeeper',
  List<String> channelIds = const [],
  bool isPrivate = false,
}) => Project(
  address: '30621:$testOwner:$dtag',
  owner: testOwner,
  dtag: dtag,
  name: name,
  description: 'The fork',
  isPrivate: isPrivate,
  channelIds: channelIds,
  memberPubkeys: const [testViewer],
  createdAt: 100,
);

Channel testChannel(
  String id, {
  String name = 'general',
  String type = 'stream',
  String? projectRef,
}) => Channel(
  id: id,
  name: name,
  channelType: type,
  visibility: 'open',
  description: '',
  createdBy: testOwner,
  createdAt: DateTime(2025),
  memberCount: 2,
  isMember: true,
  projectRef: projectRef,
);

ChannelData testChannelData(String id, {String type = 'transport'}) =>
    ChannelData(
      id: id,
      name: 'meta $id',
      channelType: type,
      visibility: 'open',
      description: '',
      projectRef: testProjectAddress,
    );

RemoteTerminal testTerminal({
  String sessionId = 's1',
  String title = 'build shell',
  String owner = testOwner,
  List<ShellRosterEntry> roster = const [],
  String projectRef = testProjectAddress,
}) => RemoteTerminal(
  sessionId: sessionId,
  ownerPubkey: owner,
  title: title,
  projectRef: projectRef,
  dims: const ShellDims(rows: 24, cols: 80),
  roster: roster,
  announcedAt: 100,
);

TerminalsIndex testTerminalsIndex(
  List<RemoteTerminal> terminals, {
  TerminalsConnection connection = TerminalsConnection.open,
  bool hasRead = true,
  String? lastError,
}) => TerminalsIndex(
  terminals: terminals,
  byProject: terminalsByProject(terminals),
  connection: connection,
  lastError: lastError,
  hasRead: hasRead,
);

ProjectsRead testProjectsRead(
  List<Project> projects, {
  Map<String, ChannelData> referenced = const {},
  ProjectsConnection connection = ProjectsConnection.open,
  bool hasRead = true,
  String? lastError,
}) => ProjectsRead(
  projects: projects,
  referencedChannels: referenced,
  connection: connection,
  lastError: lastError,
  hasRead: hasRead,
);

class FakeProjectsNotifier extends ProjectsNotifier {
  final ProjectsRead read;
  int refreshCount = 0;

  FakeProjectsNotifier(this.read);

  @override
  ProjectsRead build() => read;

  @override
  Future<void> refresh() async {
    refreshCount++;
  }
}

class FakeTerminalsIndexNotifier extends TerminalsIndexNotifier {
  final TerminalsIndex index;
  int refreshCount = 0;

  FakeTerminalsIndexNotifier(this.index);

  @override
  TerminalsIndex build() => index;

  @override
  Future<void> refresh() async {
    refreshCount++;
  }
}

class FakeChannelsNotifier extends ChannelsNotifier {
  final List<Channel> channels;

  FakeChannelsNotifier(this.channels);

  @override
  Future<List<Channel>> build() async => channels;
}

class FakeUserCacheNotifier extends UserCacheNotifier {
  final Map<String, UserProfile> users;

  FakeUserCacheNotifier(this.users);

  @override
  Map<String, UserProfile> build() => users;
}

/// Overrides for the project pages: projects, terminals, channels, profiles
/// and the viewer's key.
List<Override> projectOverrides({
  required ProjectsRead projects,
  TerminalsIndex? terminals,
  List<Channel> channels = const [],
  Map<String, UserProfile> users = const {},
  String? viewerPubkey = testViewer,
  ProjectTerminalOpener? terminalOpener,
}) => [
  projectsProvider.overrideWith(() => FakeProjectsNotifier(projects)),
  terminalsIndexProvider.overrideWith(
    () => FakeTerminalsIndexNotifier(terminals ?? testTerminalsIndex(const [])),
  ),
  channelsProvider.overrideWith(() => FakeChannelsNotifier(channels)),
  userCacheProvider.overrideWith(() => FakeUserCacheNotifier(users)),
  myPubkeyProvider.overrideWithValue(viewerPubkey),
  projectTerminalOpenerProvider.overrideWithValue(terminalOpener),
];
