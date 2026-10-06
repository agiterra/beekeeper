import * as React from "react";

import { resolveProjectScope } from "@/features/projects/lib/projectScopeSelection";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import {
  useDisplayProjectContainers,
  useRepoContainerId,
} from "@/features/projects-container/hooks";
import { ProjectsManagePanel } from "@/features/projects-container/ui/ProjectsManagePanel";
import {
  ProjectsScreenCreateDialogs,
  type ProjectsScreenCreateKind,
} from "@/features/projects-container/ui/ProjectsScreenCreateDialogs";
import { useUsersBatchQuery } from "@/features/profile/hooks";
import {
  type Project,
  type ProjectIssue,
  type ProjectPullRequest,
  type Repository,
  useProjectActivitySummariesQuery,
  useProjectLocalRepositoriesQuery,
  useProjectsQuery,
  useProjectsWorkItemsQuery,
} from "@/features/projects/hooks";
import { useRepositoryActivitySummariesQuery } from "@/features/projects/repositoryActivityHooks";
import { useProjectsRepoSnapshotsQuery } from "@/features/projects/useProjectsRepoSnapshots";
import { useMemberChannelIds } from "@/features/projects/useRepositoryAccess";
import {
  projectRepoHostForProject,
  projectRepoHostForRepository,
} from "@/features/projects/lib/projectRepoHost";
import { ProjectsActivityFeed } from "@/features/projects/ui/ProjectsActivityFeed";
import {
  EmptyFilteredState,
  EmptyState,
} from "@/features/projects/ui/ProjectCards";
import { CreateProjectIssueDialog } from "@/features/projects/ui/CreateProjectIssueDialog";
import { CreatePullRequestDialog } from "@/features/projects/ui/CreatePullRequestDialog";
import { ProjectsCreateMenu } from "@/features/projects/ui/ProjectsCreateMenu";
import { ProjectsIssuesList } from "@/features/projects/ui/ProjectsIssuesList";
import { ProjectsOverviewPanel } from "@/features/projects/ui/ProjectsOverviewPanel";
import { ProjectsOverviewRail } from "@/features/projects/ui/ProjectsOverviewRail";
import { ProjectsPullRequestsList } from "@/features/projects/ui/ProjectsPullRequestsList";
import { ProjectsWorkItemsLoadNotice } from "@/features/projects/ui/ProjectsWorkItemsLoadNotice";
import { ProjectsListScopeDropdown } from "@/features/projects/ui/ProjectsListScopeDropdown";
import { PROJECT_LIST_CONTAINER_CLASS } from "@/features/projects/ui/projectListRowStyles";
import {
  ProjectsToolbar,
  ProjectsViewModeToggle,
} from "@/features/projects/ui/ProjectsToolbar";
import { hasLocalRepositoryCheckout } from "@/features/projects/lib/projectLocalRepos";
import {
  RepositoryGridCard,
  RepositoryListRow,
} from "@/features/projects/ui/RepositoryCards";
import {
  ISSUE_SCOPE_OPTIONS,
  isRepositoryAccessibleToViewer,
  projectPeople,
  PULL_REQUEST_SCOPE_OPTIONS,
  REPOSITORY_SCOPE_OPTIONS,
  type ProjectsFilter,
  type ProjectsRepositoryScope,
  type ProjectsSort,
  type ProjectsViewMode,
  type ProjectsWorkItemScope,
  readStoredFilter,
  readStoredIssueScope,
  readStoredProjectScope,
  readStoredPullRequestScope,
  readStoredRepositoryScope,
  readStoredSort,
  readStoredViewMode,
  writeStoredFilter,
  writeStoredIssueScope,
  writeStoredProjectScope,
  writeStoredPullRequestScope,
  writeStoredRepositoryScope,
  writeStoredSort,
  writeStoredViewMode,
} from "@/features/projects/lib/projectsViewHelpers";
import { useOpenProjectTerminal } from "@/features/projects/ui/useOpenProjectTerminal";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";
import { useCommunities } from "@/features/communities/useCommunities";
import { useIdentityQuery } from "@/shared/api/hooks";
import { topChromeInset } from "@/shared/layout/chromeLayout";
import { cn } from "@/shared/lib/cn";
import { normalizePubkey } from "@/shared/lib/pubkey";
import { useRelayOrigin } from "@/shared/lib/useRelayOrigin";
import { Button } from "@/shared/ui/button";
import { PageHeader } from "@/shared/ui/PageHeader";

const MANY_PROJECTS_THRESHOLD = 12;

export function ProjectsView({
  initialFilter,
}: {
  /** Overrides the stored tab when set (e.g. `/projects?filter=projects`). */
  initialFilter?: ProjectsFilter;
} = {}) {
  const { goProjectRepo } = useAppNavigation();
  const repoContainerId = useRepoContainerId();
  const { activeCommunity } = useCommunities();
  const relayOrigin = useRelayOrigin();
  const scrollIdleTimerRef = React.useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );
  const scrollIndicatorRef = React.useRef<HTMLDivElement | null>(null);
  // The native scrollbar thumb is permanently transparent (WebKit won't
  // re-resolve ::-webkit-scrollbar styles dynamically), so we paint our own
  // indicator over the gutter and show it only while the area is scrolling.
  const handleContentScroll = React.useCallback(
    (event: React.UIEvent<HTMLDivElement>) => {
      const element = event.currentTarget;
      const indicator = scrollIndicatorRef.current;
      if (!indicator) return;

      const { clientHeight, scrollHeight, scrollTop } = element;
      if (scrollHeight <= clientHeight) {
        indicator.style.opacity = "0";
        return;
      }

      const thumbHeight = Math.max(
        24,
        (clientHeight / scrollHeight) * clientHeight,
      );
      const maxOffset = clientHeight - thumbHeight;
      const offset = (scrollTop / (scrollHeight - clientHeight)) * maxOffset;
      indicator.style.height = `${thumbHeight}px`;
      indicator.style.transform = `translateY(${offset}px)`;
      indicator.style.opacity = "1";

      if (scrollIdleTimerRef.current !== null) {
        globalThis.clearTimeout(scrollIdleTimerRef.current);
      }
      scrollIdleTimerRef.current = globalThis.setTimeout(() => {
        indicator.style.opacity = "0";
        scrollIdleTimerRef.current = null;
      }, 700);
    },
    [],
  );
  const projectsQuery = useProjectsQuery();
  const identityQuery = useIdentityQuery();
  const projects = projectsQuery.data ?? [];
  const localRepositoriesQuery = useProjectLocalRepositoriesQuery(
    activeCommunity?.reposDir,
  );
  const [filter, setFilter] = React.useState<ProjectsFilter>(() => {
    if (initialFilter) return initialFilter;
    const storedFilter = readStoredFilter();
    return storedFilter === "mine" || storedFilter === "local"
      ? "repositories"
      : storedFilter;
  });
  // Re-apply when the search param changes while the screen is mounted
  // (e.g. clicking the sidebar Projects heading from within /projects).
  React.useEffect(() => {
    if (initialFilter) setFilter(initialFilter);
  }, [initialFilter]);
  const activitySummariesQuery = useProjectActivitySummariesQuery(
    filter === "prs" || filter === "issues" || filter === "repositories"
      ? []
      : projects,
  );
  const repositoryActivitySummariesQuery = useRepositoryActivitySummariesQuery(
    filter === "repositories" ? projects : [],
  );
  const [repositoryScope, setRepositoryScope] =
    React.useState<ProjectsRepositoryScope>(() => {
      const storedScope = readStoredRepositoryScope();
      return filter === "projects" &&
        (storedScope === "buzz" || storedScope === "linked")
        ? "all"
        : storedScope;
    });
  const [projectScope, setProjectScope] = React.useState<string>(() =>
    readStoredProjectScope(),
  );
  const projectContainers = useDisplayProjectContainers();
  const projectScopeOptions = React.useMemo(
    () => [
      { label: "All Projects", value: "all" },
      ...projectContainers.map((container) => ({
        label: container.name,
        value: container.id,
      })),
    ],
    [projectContainers],
  );
  const handleProjectScopeChange = React.useCallback((value: string) => {
    setProjectScope(value);
    writeStoredProjectScope(value);
  }, []);
  // A stored scope pointing at a deleted/unknown project must not silently
  // filter everything out — treat it as "all".
  const effectiveProjectScope = React.useMemo(
    () => resolveProjectScope(projectScope, projectContainers),
    [projectScope, projectContainers],
  );
  /** The container the scope dropdown has selected; null on "All Projects".
   * Creates from the `+` menu land in it (the menu offers only "Project"
   * without one, so every other create always has a concrete target). */
  const selectedContainer = React.useMemo(
    () =>
      projectContainers.find(
        (container) => container.id === effectiveProjectScope,
      ) ?? null,
    [projectContainers, effectiveProjectScope],
  );
  const [pullRequestScope, setPullRequestScope] =
    React.useState<ProjectsWorkItemScope>(() => readStoredPullRequestScope());
  const [issueScope, setIssueScope] = React.useState<ProjectsWorkItemScope>(
    () => readStoredIssueScope(),
  );
  const projectsWorkItemsQuery = useProjectsWorkItemsQuery(
    filter === "all" || filter === "prs" || filter === "issues" ? projects : [],
  );
  // One blobless clone per primary Buzz repository, only while the overview
  // header is visible.
  const snapshotProjects = React.useMemo(
    () =>
      filter === "all"
        ? projects.filter(
            (project) =>
              projectRepoHostForProject(project, relayOrigin).kind === "buzz",
          )
        : [],
    [filter, projects, relayOrigin],
  );
  const repoSnapshotsQuery = useProjectsRepoSnapshotsQuery(
    snapshotProjects,
    activeCommunity?.reposDir,
  );
  const memberChannelIds = useMemberChannelIds();
  const [screenCreateKind, setScreenCreateKind] =
    React.useState<ProjectsScreenCreateKind | null>(null);
  const [createIssueOpen, setCreateIssueOpen] = React.useState(false);
  const [createPullRequestOpen, setCreatePullRequestOpen] =
    React.useState(false);
  const [storedViewMode, setStoredViewMode] =
    React.useState<ProjectsViewMode | null>(() => readStoredViewMode());
  const [sort, setSort] = React.useState<ProjectsSort>(() => readStoredSort());
  const viewMode =
    storedViewMode ??
    (projects.length > MANY_PROJECTS_THRESHOLD ? "list" : "grid");

  const projectPubkeys = React.useMemo(
    () => [
      ...new Set(
        [
          ...projects.flatMap((project) =>
            projectPeople(project, activitySummariesQuery.data?.[project.id]),
          ),
          ...(projectsWorkItemsQuery.data?.pullRequests.items.flatMap(
            ({ pullRequest }) => [
              pullRequest.author,
              ...pullRequest.recipients,
              ...pullRequest.reviewers,
              ...pullRequest.approvals.map((approval) => approval.author),
              ...pullRequest.updates.map((update) => update.author),
              ...pullRequest.comments.map((comment) => comment.author),
            ],
          ) ?? []),
          ...(projectsWorkItemsQuery.data?.issues.items.flatMap(({ issue }) => [
            issue.author,
            ...issue.recipients,
            ...issue.assignees,
            ...issue.comments.map((comment) => comment.author),
          ]) ?? []),
        ].map(normalizePubkey),
      ),
    ],
    [activitySummariesQuery.data, projects, projectsWorkItemsQuery.data],
  );
  const profilesQuery = useUsersBatchQuery(projectPubkeys, {
    enabled: projectPubkeys.length > 0,
  });
  const profiles = profilesQuery.data?.profiles;
  const currentPubkey = identityQuery.data?.pubkey;

  const handleViewModeChange = React.useCallback(
    (nextViewMode: ProjectsViewMode) => {
      setStoredViewMode(nextViewMode);
      writeStoredViewMode(nextViewMode);
    },
    [],
  );

  const handleFilterChange = React.useCallback(
    (nextFilter: ProjectsFilter) => {
      if (
        nextFilter === "projects" &&
        (repositoryScope === "buzz" || repositoryScope === "linked")
      ) {
        setRepositoryScope("all");
        writeStoredRepositoryScope("all");
      }
      setFilter(nextFilter);
      writeStoredFilter(nextFilter);
    },
    [repositoryScope],
  );

  const handleRepositoryScopeChange = React.useCallback(
    (scope: ProjectsRepositoryScope) => {
      setRepositoryScope(scope);
      writeStoredRepositoryScope(scope);
    },
    [],
  );

  const handlePullRequestScopeChange = React.useCallback(
    (scope: ProjectsWorkItemScope) => {
      setPullRequestScope(scope);
      writeStoredPullRequestScope(scope);
    },
    [],
  );

  const handleIssueScopeChange = React.useCallback(
    (scope: ProjectsWorkItemScope) => {
      setIssueScope(scope);
      writeStoredIssueScope(scope);
    },
    [],
  );

  const handleSortChange = React.useCallback((nextSort: ProjectsSort) => {
    setSort(nextSort);
    writeStoredSort(nextSort);
  }, []);

  const localRepoNames = React.useMemo(
    () =>
      new Set(
        (localRepositoriesQuery.data ?? []).map(
          (repository) => repository.name,
        ),
      ),
    [localRepositoriesQuery.data],
  );

  const repositoryAccessInput = React.useMemo(
    () => ({
      currentPubkey,
      localRepoNames,
      memberChannelIds,
      relayOrigin,
    }),
    [currentPubkey, localRepoNames, memberChannelIds, relayOrigin],
  );

  // Count projects with a checkout on this machine — matches what the
  // "Local" filter actually lists, not every directory in the repos folder.
  /** Container for a multi-repo project — resolved from its primary (first)
   * repository; projects with no repositories fall back to General. */
  const projectContainerId = React.useCallback(
    (project: Project) => repoContainerId(project.repositories[0]),
    [repoContainerId],
  );
  /** True when the project belongs to the selected container (or no scope set). */
  const inProjectScope = React.useCallback(
    (project: Project) =>
      effectiveProjectScope === "all" ||
      projectContainerId(project) === effectiveProjectScope,
    [effectiveProjectScope, projectContainerId],
  );
  const projectScopedRepos = React.useMemo(
    () => projects.filter(inProjectScope),
    [projects, inProjectScope],
  );

  const visibleRepositories = React.useMemo(() => {
    if (filter !== "repositories") return [];
    const repositories = [
      ...new Map(
        projects
          .flatMap((project) =>
            project.repositories.map((repository) => ({
              project,
              repository,
            })),
          )
          .map((item) => [item.repository.repoAddress, item]),
      ).values(),
    ];
    return repositories
      .filter(({ repository }) => {
        if (repositoryScope === "accessible") {
          return isRepositoryAccessibleToViewer(
            repository,
            repositoryAccessInput,
          );
        }
        if (repositoryScope === "mine") {
          if (!currentPubkey) return false;
          const normalizedCurrentPubkey = normalizePubkey(currentPubkey);
          return (
            normalizePubkey(repository.owner) === normalizedCurrentPubkey ||
            repository.contributors.some(
              (pubkey) => normalizePubkey(pubkey) === normalizedCurrentPubkey,
            )
          );
        }
        if (repositoryScope === "local") {
          return hasLocalRepositoryCheckout(repository, localRepoNames);
        }
        if (repositoryScope === "buzz") {
          return (
            projectRepoHostForRepository(repository, relayOrigin).kind ===
            "buzz"
          );
        }
        if (repositoryScope === "linked") {
          return (
            projectRepoHostForRepository(repository, relayOrigin).kind ===
            "external"
          );
        }
        return true;
      })
      .sort((left, right) => {
        if (sort === "name") {
          return left.repository.name.localeCompare(right.repository.name);
        }
        if (sort === "created") {
          return right.repository.createdAt - left.repository.createdAt;
        }
        const leftUpdatedAt =
          repositoryActivitySummariesQuery.data?.[left.repository.repoAddress]
            ?.updatedAt ?? left.repository.createdAt;
        const rightUpdatedAt =
          repositoryActivitySummariesQuery.data?.[right.repository.repoAddress]
            ?.updatedAt ?? right.repository.createdAt;
        return rightUpdatedAt - leftUpdatedAt;
      });
  }, [
    currentPubkey,
    filter,
    localRepoNames,
    projects,
    relayOrigin,
    repositoryAccessInput,
    repositoryActivitySummariesQuery.data,
    repositoryScope,
    sort,
  ]);

  const visiblePullRequests = React.useMemo(() => {
    const pullRequests = projectsWorkItemsQuery.data?.pullRequests.items ?? [];
    const projectScoped =
      effectiveProjectScope === "all"
        ? pullRequests
        : pullRequests.filter(
            ({ project }) =>
              projectContainerId(project) === effectiveProjectScope,
          );
    const scopedPullRequests =
      pullRequestScope === "mine" && currentPubkey
        ? projectScoped.filter(
            ({ pullRequest }) =>
              normalizePubkey(pullRequest.author) ===
              normalizePubkey(currentPubkey),
          )
        : projectScoped;
    return [...scopedPullRequests].sort((left, right) => {
      if (sort === "name") {
        return left.pullRequest.title.localeCompare(right.pullRequest.title);
      }
      if (sort === "created") {
        return right.pullRequest.createdAt - left.pullRequest.createdAt;
      }
      return right.pullRequest.updatedAt - left.pullRequest.updatedAt;
    });
  }, [
    currentPubkey,
    effectiveProjectScope,
    projectsWorkItemsQuery.data,
    pullRequestScope,
    projectContainerId,
    sort,
  ]);

  const visibleIssues = React.useMemo(() => {
    const issues = projectsWorkItemsQuery.data?.issues.items ?? [];
    const projectScoped =
      effectiveProjectScope === "all"
        ? issues
        : issues.filter(
            ({ project }) =>
              projectContainerId(project) === effectiveProjectScope,
          );
    const viewer = currentPubkey ? normalizePubkey(currentPubkey) : null;
    const scopedIssues =
      issueScope === "mine" && viewer
        ? projectScoped.filter(
            ({ issue }) => normalizePubkey(issue.author) === viewer,
          )
        : issueScope === "assigned" && viewer
          ? projectScoped.filter(({ issue }) =>
              issue.assignees.some(
                (assignee) => normalizePubkey(assignee) === viewer,
              ),
            )
          : projectScoped;
    return [...scopedIssues].sort((left, right) => {
      if (sort === "name") {
        return left.issue.title.localeCompare(right.issue.title);
      }
      if (sort === "created") {
        return right.issue.createdAt - left.issue.createdAt;
      }
      return right.issue.updatedAt - left.issue.updatedAt;
    });
  }, [
    currentPubkey,
    effectiveProjectScope,
    issueScope,
    projectsWorkItemsQuery.data,
    projectContainerId,
    sort,
  ]);

  // Route by the canonical `owner:dtag` repo ID under its containing
  // project — a bare dtag is ambiguous across owners (forks can share the
  // same dtag).
  const handleOpenProject = React.useCallback(
    (project: Project) => {
      void goProjectRepo(projectContainerId(project), project.id);
    },
    [goProjectRepo, projectContainerId],
  );

  const handleOpenRepository = React.useCallback(
    (project: Project, repository: Repository) => {
      void goProjectRepo(projectContainerId(project), project.id, {
        repositoryId: repository.id,
      });
    },
    [goProjectRepo, projectContainerId],
  );

  const handleOpenCommit = React.useCallback(
    (project: Project, commitHash: string) => {
      void goProjectRepo(projectContainerId(project), project.id, {
        commitHash,
      });
    },
    [goProjectRepo, projectContainerId],
  );

  const handleOpenPullRequest = React.useCallback(
    (
      project: Project,
      repository: Repository,
      pullRequest: ProjectPullRequest,
    ) => {
      void goProjectRepo(projectContainerId(project), project.id, {
        pullRequestId: pullRequest.id,
        repositoryId: repository.id,
      });
    },
    [goProjectRepo, projectContainerId],
  );

  const handleOpenIssue = React.useCallback(
    (project: Project, repository: Repository, issue: ProjectIssue) => {
      void goProjectRepo(projectContainerId(project), project.id, {
        issueId: issue.id,
        repositoryId: repository.id,
      });
    },
    [goProjectRepo, projectContainerId],
  );

  const openTerminal = useOpenProjectTerminal(activeCommunity?.reposDir);
  const handleOpenRepositoryTerminal = React.useCallback(
    (repository: Repository) =>
      openTerminal(repository, {
        hasLocalCheckout: hasLocalRepositoryCheckout(
          repository,
          localRepoNames,
        ),
      }),
    [localRepoNames, openTerminal],
  );

  if (projectsQuery.isLoading) {
    return <ViewLoadingFallback kind="projects" />;
  }

  if (projectsQuery.isError) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-2 text-muted-foreground">
        <p className="text-sm text-red-400">Failed to load projects</p>
        <Button
          onClick={() => void projectsQuery.refetch()}
          size="sm"
          variant="outline"
        >
          Retry
        </Button>
      </div>
    );
  }

  if (projects.length === 0 && filter !== "projects") {
    return <EmptyState />;
  }

  const repositoryItems =
    visibleRepositories.length === 0 ? (
      <EmptyFilteredState />
    ) : viewMode === "grid" ? (
      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        {visibleRepositories.map(({ project, repository }) => (
          <RepositoryGridCard
            hasLocal={hasLocalRepositoryCheckout(repository, localRepoNames)}
            key={repository.repoAddress}
            onOpen={handleOpenRepository}
            onOpenTerminal={handleOpenRepositoryTerminal}
            profiles={profiles}
            project={project}
            repository={repository}
            summary={
              repositoryActivitySummariesQuery.data?.[repository.repoAddress]
            }
          />
        ))}
      </div>
    ) : (
      <div className={PROJECT_LIST_CONTAINER_CLASS}>
        {visibleRepositories.map(({ project, repository }) => (
          <RepositoryListRow
            hasLocal={hasLocalRepositoryCheckout(repository, localRepoNames)}
            key={repository.repoAddress}
            onOpen={handleOpenRepository}
            onOpenTerminal={handleOpenRepositoryTerminal}
            profiles={profiles}
            project={project}
            repository={repository}
            summary={
              repositoryActivitySummariesQuery.data?.[repository.repoAddress]
            }
          />
        ))}
      </div>
    );

  const listControls = (
    <div className="flex flex-wrap items-center gap-2 sm:justify-end">
      <label className="flex items-center gap-2 text-xs text-muted-foreground">
        <span className="sr-only">Sort projects</span>
        <select
          className="h-8 rounded-md bg-transparent px-2 text-xs text-foreground outline-hidden hover:bg-muted/50 focus:ring-1 focus:ring-ring"
          onChange={(event) =>
            handleSortChange(event.target.value as ProjectsSort)
          }
          value={sort}
        >
          <option value="updated">Recent activity</option>
          <option value="created">Created date</option>
          <option value="name">Name</option>
        </select>
      </label>
      <ProjectsViewModeToggle
        onViewModeChange={handleViewModeChange}
        viewMode={viewMode}
      />
    </div>
  );

  const workItemFailedSections = [
    ...new Set([
      ...(projectsWorkItemsQuery.data?.issues.failedSections ?? []),
      ...(projectsWorkItemsQuery.data?.pullRequests.failedSections ?? []),
    ]),
  ];
  const activityFeed = (
    <>
      <ProjectsWorkItemsLoadNotice
        error={projectsWorkItemsQuery.error}
        failedSections={workItemFailedSections}
        isRetrying={
          projectsWorkItemsQuery.isFetching && !projectsWorkItemsQuery.isLoading
        }
        onRetry={() => void projectsWorkItemsQuery.refetch()}
        subject="project activity"
      />
      <ProjectsActivityFeed
        isLoading={
          repoSnapshotsQuery.isLoading || projectsWorkItemsQuery.isLoading
        }
        issues={(projectsWorkItemsQuery.data?.issues.items ?? []).filter(
          ({ project }) => inProjectScope(project),
        )}
        onOpenCommit={handleOpenCommit}
        onOpenIssue={handleOpenIssue}
        onOpenProject={handleOpenProject}
        onOpenPullRequest={handleOpenPullRequest}
        profiles={profiles}
        projects={projectScopedRepos}
        pullRequests={(
          projectsWorkItemsQuery.data?.pullRequests.items ?? []
        ).filter(({ project }) => inProjectScope(project))}
        snapshots={repoSnapshotsQuery.data?.snapshots}
      />
    </>
  );

  // Without a selected project ("All Projects") the menu offers only
  // "Project" — every other kind is created into the selected project, so
  // hiding them makes the target unambiguous.
  const createMenu = (
    <ProjectsCreateMenu
      onCreateIssue={
        selectedContainer ? () => setCreateIssueOpen(true) : undefined
      }
      onCreatePullRequest={
        selectedContainer ? () => setCreatePullRequestOpen(true) : undefined
      }
      onCreateRepository={
        selectedContainer ? () => setScreenCreateKind("repo") : undefined
      }
      onImportRepository={
        selectedContainer ? () => setScreenCreateKind("repo-import") : undefined
      }
      onCreateChannel={
        selectedContainer ? () => setScreenCreateKind("channel") : undefined
      }
      onCreateForum={
        selectedContainer ? () => setScreenCreateKind("forum") : undefined
      }
      onCreateWorkflow={
        selectedContainer ? () => setScreenCreateKind("workflow") : undefined
      }
      onCreateProject={() => setScreenCreateKind("project")}
    />
  );

  const projectsHeader = (
    <PageHeader
      className="pointer-events-auto mb-4"
      description="Set up and manage your projects."
      title="Projects"
    />
  );

  const projectsNavigation = (
    <div className="flex h-[3.25rem] min-w-0 items-center">
      <div className="h-full min-w-0 flex-1 overflow-hidden">
        <ProjectsToolbar filter={filter} onFilterChange={handleFilterChange} />
      </div>
      <div className="shrink-0 pl-4">
        <ProjectsListScopeDropdown
          label="Filter by project"
          onChange={handleProjectScopeChange}
          options={projectScopeOptions}
          value={effectiveProjectScope}
        />
      </div>
      {createMenu}
    </div>
  );

  return (
    <div
      className={cn(
        "relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-tl-xl",
        topChromeInset.divider,
      )}
    >
      {/* Scroll indicator painted over the scrollbar gutter; only visible
          while scrolling (native thumb is transparent). */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute right-[3px] top-0 z-50 w-1 rounded-full bg-border/80 opacity-0 transition-opacity duration-200"
        ref={scrollIndicatorRef}
      />
      <ProjectsScreenCreateDialogs
        kind={screenCreateKind}
        onClose={() => setScreenCreateKind(null)}
        onRepoCreated={() => {
          // Land on the list that actually shows the new repo — the
          // Overview only surfaces the top few most-active repositories.
          handleRepositoryScopeChange("all");
          handleFilterChange("projects");
        }}
        targetProject={selectedContainer}
      />
      {createPullRequestOpen ? (
        <CreatePullRequestDialog
          onCreated={async (
            createdProject,
            createdRepository,
            pullRequestId,
          ) => {
            await goProjectRepo(
              projectContainerId(createdProject),
              createdProject.id,
              {
                pullRequestId,
                repositoryId: createdRepository.id,
              },
            );
          }}
          onOpenChange={setCreatePullRequestOpen}
          open
          projects={projectScopedRepos}
          reposDir={activeCommunity?.reposDir}
        />
      ) : null}
      <CreateProjectIssueDialog
        onCreated={async (createdProject, createdRepository, issueId) => {
          await goProjectRepo(
            projectContainerId(createdProject),
            createdProject.id,
            {
              issueId,
              repositoryId: createdRepository.id,
            },
          );
        }}
        onOpenChange={setCreateIssueOpen}
        open={createIssueOpen}
        projects={projectScopedRepos}
      />
      <div
        className="beekeeper-content-scrollbar min-h-0 min-w-0 flex-1 overflow-x-hidden overflow-y-scroll"
        onScroll={handleContentScroll}
      >
        <div className="px-4 pb-7 pt-7 sm:px-6 sm:pb-8 sm:pt-8">
          <div className="mx-auto w-full max-w-6xl">{projectsHeader}</div>
          <div className="sticky top-0 z-30 -mx-4 bg-background/80 backdrop-blur-xl supports-backdrop-filter:bg-background/65 dark:bg-background/75 dark:supports-backdrop-filter:bg-background/60 sm:-mx-6">
            <div className="px-4 sm:px-6">
              <div className="mx-auto w-full max-w-6xl">
                {projectsNavigation}
              </div>
            </div>
          </div>
          <div className="mx-auto w-full max-w-6xl">
            <div className="w-full min-w-0 pb-4 pt-4">
              {filter === "all" ? (
                <div className="space-y-3">
                  <ProjectsOverviewPanel
                    metadata={
                      <ProjectsOverviewRail
                        profiles={profiles}
                        projects={projectScopedRepos}
                        summaries={activitySummariesQuery.data}
                      />
                    }
                    onSelectSection={(section) => {
                      handleFilterChange(section);
                    }}
                    projects={projectScopedRepos}
                    summaries={activitySummariesQuery.data}
                  >
                    <section className="space-y-3">{activityFeed}</section>
                  </ProjectsOverviewPanel>
                </div>
              ) : filter === "projects" ? (
                <ProjectsManagePanel />
              ) : (
                <section className="space-y-3">
                  <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                    <div className="flex flex-wrap items-center gap-3">
                      {filter === "prs" ? (
                        <ProjectsListScopeDropdown
                          label="Filter pull requests"
                          onChange={handlePullRequestScopeChange}
                          options={PULL_REQUEST_SCOPE_OPTIONS}
                          value={pullRequestScope}
                        />
                      ) : filter === "issues" ? (
                        <ProjectsListScopeDropdown
                          label="Filter issues"
                          onChange={handleIssueScopeChange}
                          options={ISSUE_SCOPE_OPTIONS}
                          value={issueScope}
                        />
                      ) : (
                        <ProjectsListScopeDropdown
                          label="Filter repositories"
                          onChange={handleRepositoryScopeChange}
                          options={REPOSITORY_SCOPE_OPTIONS}
                          value={repositoryScope}
                        />
                      )}
                    </div>
                    {listControls}
                  </div>
                  {filter === "prs" ? (
                    <ProjectsPullRequestsList
                      error={projectsWorkItemsQuery.error}
                      failedSections={
                        projectsWorkItemsQuery.data?.pullRequests
                          .failedSections ?? []
                      }
                      isLoading={projectsWorkItemsQuery.isLoading}
                      isRetrying={
                        projectsWorkItemsQuery.isFetching &&
                        !projectsWorkItemsQuery.isLoading
                      }
                      onOpen={handleOpenPullRequest}
                      onRetry={() => void projectsWorkItemsQuery.refetch()}
                      profiles={profiles}
                      pullRequests={visiblePullRequests}
                      viewMode={viewMode}
                    />
                  ) : filter === "issues" ? (
                    <ProjectsIssuesList
                      error={projectsWorkItemsQuery.error}
                      failedSections={
                        projectsWorkItemsQuery.data?.issues.failedSections ?? []
                      }
                      isLoading={projectsWorkItemsQuery.isLoading}
                      isRetrying={
                        projectsWorkItemsQuery.isFetching &&
                        !projectsWorkItemsQuery.isLoading
                      }
                      issues={visibleIssues}
                      onOpen={handleOpenIssue}
                      onRetry={() => void projectsWorkItemsQuery.refetch()}
                      profiles={profiles}
                      viewMode={viewMode}
                    />
                  ) : (
                    repositoryItems
                  )}
                </section>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
