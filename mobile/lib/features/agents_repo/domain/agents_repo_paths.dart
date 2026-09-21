import 'agents_repo_draft_op.dart';

/// Grouping and naming the agents repository's files for the page. The
/// grammar itself lives in `agents_repo_draft_op.dart`; this is
/// presentation, in the same order Desktop's Files tab uses.
enum AgentsRepoGroup {
  plans('Plans'),
  roles('Roles'),
  skills('Skills'),
  team('Team'),
  actions('Actions'),
  readme('README'),
  archive('Archive (not in force)'),
  other('Other files (read-only)');

  const AgentsRepoGroup(this.label);
  final String label;
}

AgentsRepoGroup groupOf(String path) => switch (draftPathClass(path)) {
  DraftPathClass.plan => AgentsRepoGroup.plans,
  DraftPathClass.role => AgentsRepoGroup.roles,
  DraftPathClass.roleSkill ||
  DraftPathClass.sharedSkill => AgentsRepoGroup.skills,
  DraftPathClass.archivedRole ||
  DraftPathClass.archivedPlan => AgentsRepoGroup.archive,
  DraftPathClass.rootFile =>
    path == 'team.yml'
        ? AgentsRepoGroup.team
        : path == 'actions.yml'
        ? AgentsRepoGroup.actions
        : AgentsRepoGroup.readme,
  null => AgentsRepoGroup.other,
};

bool isMarkdownPath(String path) => path.endsWith('.md');

bool isGitkeep(String path) => path.endsWith('/.gitkeep') || path == '.gitkeep';

/// Whether a path may be drafted at all.
bool isDraftablePath(String path) =>
    !isGitkeep(path) && draftPathClass(path) != null;

/// The short name shown in the list.
String displayName(String path) {
  final tail = path.substring(path.lastIndexOf('/') + 1);
  final group = groupOf(path);
  if (group == AgentsRepoGroup.plans ||
      group == AgentsRepoGroup.roles ||
      group == AgentsRepoGroup.archive) {
    return tail.endsWith('.md') ? tail.substring(0, tail.length - 3) : tail;
  }
  if (group == AgentsRepoGroup.skills && tail == 'SKILL.md') {
    final segments = path.split('/');
    return segments.length >= 2 ? segments[segments.length - 2] : tail;
  }
  return tail;
}

/// Group paths into ordered sections, each sorted by path.
List<({AgentsRepoGroup group, List<T> entries})> groupPaths<T>(
  Iterable<T> entries,
  String Function(T) pathOf,
) {
  final buckets = <AgentsRepoGroup, List<T>>{};
  for (final entry in entries) {
    buckets.putIfAbsent(groupOf(pathOf(entry)), () => []).add(entry);
  }
  return [
    for (final group in AgentsRepoGroup.values)
      if (buckets[group] case final list? when list.isNotEmpty)
        (
          group: group,
          entries: list..sort((a, b) => pathOf(a).compareTo(pathOf(b))),
        ),
  ];
}
