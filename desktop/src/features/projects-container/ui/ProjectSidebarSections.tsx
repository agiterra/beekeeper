import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";
import {
  useLocation,
  useNavigate,
  useSearch,
  useParams,
  useRouter,
} from "@tanstack/react-router";
import { FolderGit2, Plus } from "lucide-react";
import { toast } from "sonner";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { navigateToRememberedRoute } from "@/app/navigation/rememberedRoute";
import { useCreateShellSession } from "@/features/builtin-shell/hooks/useCreateShellSession";
import { projectDefaultCwd } from "@/features/builtin-shell/lib/projectShellCwd";
import {
  useRemoteTerminalsIndex,
  type RemoteTerminal,
} from "@/features/builtin-shell/observe/useProjectTerminals";
import { useShellSessionDialogs } from "@/features/builtin-shell/hooks/useShellSessionDialogs";
import { useShellSessions } from "@/features/builtin-shell/hooks/useShellSessions";
import { useCodingSessionClosureDialog } from "@/features/coding-sessions/hooks/useCodingSessionClosureDialog";
import { useDeleteCodingSessionDialog } from "@/features/coding-sessions/hooks/useDeleteCodingSessionDialog";
import { activeCodingSessionKey } from "../lib/activeCodingSession";
import { useProjectCapabilitiesMap } from "../lib/projectPermissions";
import { registerHotkeyTargets } from "@/features/hotkeys/lib/hotkeyTargetRegistry";
import { NAV_HOTKEY_MAX_POSITIONS } from "@/features/hotkeys/lib/navHotkeyBindings";
import { ScopeActionBadge } from "@/features/hotkeys/ui/HotkeyBadge";
import { getRememberedRoute } from "../lib/projectRouteMemoryStore";
import { compareProjectCodingSessionEntries } from "../lib/projectCodingSessionShelf";
import { resolveProjectCodingSessionOpenTarget } from "../lib/projectFoundedCodingSessionShelf";
import { channelsQueryKey, useChannelsQuery } from "@/features/channels/hooks";
import type { Channel } from "@/shared/api/types";
import type { Repository as CodeRepo } from "@/features/projects/hooks";
import {
  SidebarGroup,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/shared/ui/sidebar";
import { SidebarMenuLabel } from "@/shared/ui/sidebar-menu-label";

import { useFeatureEnabled } from "@/shared/features";
import { joinChannel } from "@/shared/api/tauriChannels";

import {
  useProjectCodingSessionBuckets,
  type ProjectContainer,
} from "../hooks";
import {
  GENERAL_PROJECT_DTAG,
  LOCAL_GENERAL_ID,
  displayProjectsWithGeneral,
} from "../lib/projectContainerModel";
import { useProjectCollapse } from "../lib/projectCollapseStorage";
import { useProjectOrder } from "../lib/projectOrderStore";
import {
  ProjectSidebarDndContext,
  SortableProjectShell,
} from "./ProjectSidebarDnd";
import { useProjectSessionFilters } from "../lib/projectSessionFilterStorage";
import { useCreateProjectContainerMutation } from "../useCreateProjectContainer";
import { toastCreateProjectOutcome } from "../lib/toastCreateProjectOutcome";
import { publishTodoOp } from "@/features/project-todos/lib/todoMutations";
import { newTodoId } from "@/features/project-todos/lib/todoOp";
import { usePinnedTodoListsIndex } from "@/features/project-todos/lib/todoSidebarIndex";
import {
  CreateTodoListDialog,
  type CreateTodoListInput,
} from "@/features/project-todos/ui/CreateTodoListDialog";
import { useGeneralProjectMigration } from "../useGeneralProjectMigration";
import { CreateProjectContainerDialog } from "./CreateProjectContainerDialog";
import {
  ProjectsScreenCreateDialogs,
  type ProjectsScreenCreateKind,
} from "./ProjectsScreenCreateDialogs";
import {
  ProjectSidebarGroup,
  type ProjectChannelHandlers,
} from "./ProjectSidebarGroup";

/**
 * The per-project collapsible sidebar groups (Projects experiment). Renders
 * one `ProjectSidebarGroup` per project container; channels, forums and
 * sessions that no project claims yet are shown under General — the real
 * General project once the owner has published it, or a local placeholder
 * until then — so nothing disappears when the experiment is enabled.
 *
 * Repositories still arrive here (`reposByProject`) only to pick a new
 * terminal's working directory; the sidebar no longer lists them.
 */
export function ProjectSidebarSections({
  projects,
  reposByProject,
  channelsByProject,
  forumsByProject,
  transportsByProject,
  unclaimedForums,
  globalChannels,
  currentPubkey,
  relayUrl,
  ...channelHandlers
}: ProjectChannelHandlers & {
  projects: ProjectContainer[];
  reposByProject: ReadonlyMap<string, CodeRepo[]>;
  channelsByProject: ReadonlyMap<string, Channel[]>;
  forumsByProject: ReadonlyMap<string, Channel[]>;
  /** Session transports per project: what places a "Not started" row. */
  transportsByProject: ReadonlyMap<string, Channel[]>;
  unclaimedForums: Channel[];
  /** Streams no project claims yet. Channels must belong to a project, so
   * these display under General until they're claimed or moved. */
  globalChannels: Channel[];
  currentPubkey?: string;
  relayUrl?: string;
}) {
  const {
    goCodingSession,
    goFoundedCodingSession,
    goNewProjectCodingSession,
    goProject,
    goProjects,
  } = useAppNavigation();
  const collapse = useProjectCollapse(currentPubkey, relayUrl);
  const sessionFilters = useProjectSessionFilters(currentPubkey, relayUrl);
  // This component only mounts while the Projects experiment is enabled, so
  // the one-shot General migration is anchored here.
  useGeneralProjectMigration(true, relayUrl);
  const createContainerMutation = useCreateProjectContainerMutation();
  // Transports included: the session buckets subscribe to them; display
  // lists filter them back out via withoutProjectSessionTransportChannels.
  const channelsQuery = useChannelsQuery({ includeSessionTransports: true });
  const forumEnabled = useFeatureEnabled("forum");

  // Shells are hoisted once (the store is shared and module-level, so this is
  // cheaper than a hook per project group) and partitioned per project below.
  const builtinShellEnabled = useFeatureEnabled("builtin-shell");
  const { sessions: allShellSessions } = useShellSessions();
  const navigate = useNavigate();
  const router = useRouter();
  const pathname = useLocation({ select: (state) => state.pathname });
  // The sidebar's active indicator needs to name a single row. `/projects`
  // is the list; `/projects/<id>` belongs to that project's own header, not
  // to the list link above it.
  const projectRouteId = pathname.startsWith("/projects/")
    ? decodeURIComponent(pathname.split("/")[2] ?? "")
    : null;
  // The to-do list on screen, when the To-Do tab is open with `?list=`.
  const activeTodoListId = useSearch({
    strict: false,
    select: (search: { list?: string }) =>
      pathname.endsWith("/todos") ? (search.list ?? null) : null,
  });
  const activeShellSessionId = useParams({
    strict: false,
    select: (p) => (p as { sessionId?: string }).sessionId,
  });
  const codingSessionRouteParams = useParams({
    strict: false,
    select: (p) => {
      const params = p as {
        channelId?: string;
        generationId?: string;
        sessionRef?: string;
      };
      return {
        channelId: params.channelId,
        generationId: params.generationId,
        sessionRef: params.sessionRef,
      };
    },
  });
  const activeSessionKey = activeCodingSessionKey(
    pathname,
    codingSessionRouteParams,
  );
  const handleOpenShell = React.useCallback(
    (sessionId: string) => {
      void navigate({ to: "/shell/$sessionId", params: { sessionId } });
    },
    [navigate],
  );
  const { createFor: createShellFor } = useCreateShellSession();
  // Other members' shared terminals, one workspace-wide index bucketed by
  // project address (observable rows carry a right-justified eye).
  const remoteTerminalsIndex = useRemoteTerminalsIndex(builtinShellEnabled);
  const handleObserveShell = React.useCallback(
    (terminal: RemoteTerminal) => {
      void navigate({
        to: "/observe/$owner/$sessionId",
        params: {
          owner: terminal.ownerPubkey,
          sessionId: terminal.sessionId,
        },
        search: { project: terminal.projectRef },
      });
    },
    [navigate],
  );
  // New terminals open in the project's code checkout when one exists.
  const handleNewShell = React.useCallback(
    (project: ProjectContainer, isFallback: boolean) => {
      const repos = reposByProject.get(project.id) ?? [];
      void projectDefaultCwd(repos).then((cwd) =>
        createShellFor(isFallback ? undefined : project.address, cwd),
      );
    },
    [reposByProject, createShellFor],
  );
  const shellDialogs = useShellSessionDialogs();
  const codingSessionClosureDialog = useCodingSessionClosureDialog();
  const codingSessionDeleteDialog = useDeleteCodingSessionDialog();

  // Coding sessions are channel-scoped the same way, except a session may also
  // carry a signed projectRef that overrides its channel's project.
  const sessionBuckets = useProjectCodingSessionBuckets(
    channelsQuery.data,
    channelsByProject,
    forumsByProject,
    transportsByProject,
  );

  const queryClient = useQueryClient();
  const [createContainerOpen, setCreateContainerOpen] = React.useState(false);
  // One dialog instance serves every group's "+" menu; the request carries
  // the target project so new channels/forums land inside it.
  const [createRequest, setCreateRequest] = React.useState<{
    kind: ProjectsScreenCreateKind;
    project: ProjectContainer;
  } | null>(null);

  // One create dialog for to-do lists serves every group's "+" menu.
  const [todoCreateProject, setTodoCreateProject] =
    React.useState<ProjectContainer | null>(null);
  const [todoCreating, setTodoCreating] = React.useState(false);
  const handleCreateTodoList = React.useCallback(
    async (input: CreateTodoListInput) => {
      const project = todoCreateProject;
      if (!project) return;
      setTodoCreating(true);
      try {
        const listId = newTodoId();
        await publishTodoOp(queryClient, project.address, {
          op: "list.create",
          listId,
          title: input.title,
          visibility: input.visibility,
        });
        if (input.pinned) {
          await publishTodoOp(queryClient, project.address, {
            op: "list.pinned",
            listId,
            pinned: true,
            visibility: input.visibility,
          });
        }
        void navigate({
          to: "/projects/$projectId/todos",
          params: { projectId: project.id },
          search: { list: listId, view: "list" },
        });
      } finally {
        setTodoCreating(false);
      }
    },
    [navigate, queryClient, todoCreateProject],
  );

  // Pinned to-do lists for every real project, sharing the tab's cache.
  const todoCoordinates = React.useMemo(
    () =>
      projects
        .filter((project) => project.owner.length > 0)
        .map((project) => project.address),
    [projects],
  );
  const pinnedTodoLists = usePinnedTodoListsIndex(todoCoordinates);

  // One roster read for every project in the sidebar. Close, archive and
  // reopen are founder-only and need no roster; delete is the project's rule,
  // so an Owner reaches any session in their project.
  const { capabilitiesFor } = useProjectCapabilitiesMap(projects);
  const displayProjects = React.useMemo(
    () => displayProjectsWithGeneral(projects),
    [projects],
  );

  // General is the fallback bucket for unclaimed items, not a peer project:
  // it stays pinned above the sortable list and carries no drag handle.
  const { reorderProjects } = useProjectOrder();
  const pinnedProjects = React.useMemo(
    () =>
      displayProjects.filter(
        (project) =>
          project.dtag === GENERAL_PROJECT_DTAG ||
          project.id === LOCAL_GENERAL_ID,
      ),
    [displayProjects],
  );
  const sortableProjects = React.useMemo(
    () =>
      displayProjects.filter(
        (project) =>
          project.dtag !== GENERAL_PROJECT_DTAG &&
          project.id !== LOCAL_GENERAL_ID,
      ),
    [displayProjects],
  );

  const handleOpenProject = React.useCallback(
    (project: ProjectContainer) => {
      void goProject(project.id);
    },
    [goProject],
  );

  /**
   * The projects in the order they are rendered — General first, then the
   * drag-sorted rest — capped at the reachable positions. This is the array
   * the scope hotkey indexes into and the array the badges number from, so
   * "⌥2" and the second row cannot come apart.
   */
  const hotkeyProjects = React.useMemo(
    () =>
      [...pinnedProjects, ...sortableProjects].slice(
        0,
        NAV_HOTKEY_MAX_POSITIONS,
      ),
    [pinnedProjects, sortableProjects],
  );
  const hotkeyPositionByProjectId = React.useMemo(() => {
    const map = new Map<string, number>();
    hotkeyProjects.forEach((project, index) => {
      map.set(project.id, index);
    });
    return map;
  }, [hotkeyProjects]);

  /**
   * Opening a project by chord means returning to where you left it, not to
   * its home page — a project is mostly the session you had open in it. Falls
   * back to the project home when nothing is remembered, or when the
   * remembered route has since stopped resolving.
   */
  const openRememberedProject = React.useEffectEvent(
    (project: ProjectContainer) => {
      navigateToRememberedRoute(
        router,
        getRememberedRoute(project.id),
        () => void goProject(project.id),
      );
    },
  );

  React.useEffect(
    () =>
      registerHotkeyTargets(
        "projects",
        hotkeyProjects.map((project) => ({
          key: project.id,
          label: project.name,
          activate: () => openRememberedProject(project),
        })),
      ),
    [hotkeyProjects],
  );

  // Project groups list every open channel (browse surface), so a row can
  // name a channel the identity hasn't joined yet — join on first open, and
  // only navigate once membership is real (the channel screen assumes it).
  const { onSelectChannel } = channelHandlers;
  const handleSelectChannel = React.useCallback(
    (channelId: string) => {
      const channel = (channelsQuery.data ?? []).find(
        (candidate) => candidate.id === channelId,
      );
      if (channel && !channel.isMember) {
        void joinChannel(channelId)
          .then(() => {
            void queryClient.invalidateQueries({ queryKey: channelsQueryKey });
            onSelectChannel(channelId);
          })
          .catch((error) => {
            toast.error(
              error instanceof Error
                ? error.message
                : "Failed to join the channel.",
            );
          });
        return;
      }
      onSelectChannel(channelId);
    },
    [channelsQuery.data, queryClient, onSelectChannel],
  );
  const groupChannelHandlers = {
    ...channelHandlers,
    onSelectChannel: handleSelectChannel,
  };

  /**
   * One project group. `drag` is supplied only for the sortable projects —
   * General renders through the same path without it, so it gets no grip.
   */
  const renderProject = (
    project: ProjectContainer,
    drag?: {
      dragHandleProps: React.HTMLAttributes<HTMLElement>;
      isDragging: boolean;
    },
  ) => {
    const isGeneral = project.dtag === GENERAL_PROJECT_DTAG;
    const isFallback = project.id === LOCAL_GENERAL_ID;
    // Unclaimed forums/shells always land in General — they must belong
    // to a project, and General is the sweep target.
    const forums = forumEnabled
      ? [
          ...(forumsByProject.get(project.id) ?? []),
          ...(isGeneral ? unclaimedForums : []),
        ]
      : [];
    const shellSessions = builtinShellEnabled
      ? allShellSessions.filter(
          (session) =>
            session.projectRef === project.address ||
            ((isFallback || isGeneral) && !session.projectRef),
        )
      : [];
    const remoteTerminals =
      builtinShellEnabled && !isFallback
        ? (remoteTerminalsIndex.get(project.address) ?? [])
        : [];
    return (
      <ProjectSidebarGroup
        key={project.id}
        dragHandleProps={drag?.dragHandleProps}
        isDragging={drag?.isDragging}
        project={project}
        isFallback={isFallback}
        codingSessions={[
          ...(sessionBuckets.byProject.get(project.id) ?? []),
          ...(isGeneral ? sessionBuckets.unclaimed : []),
          // Each half is sorted, the concat is not: without a re-sort,
          // General's unclaimed sessions always trail claimed ones and a
          // working unclaimed session can be capped out of the shelf.
        ].sort(compareProjectCodingSessionEntries)}
        streamChannels={[
          ...(channelsByProject.get(project.id) ?? []),
          ...(isGeneral ? globalChannels : []),
        ]}
        forumChannels={forums}
        channelHandlers={groupChannelHandlers}
        collapsed={collapse.isProjectCollapsed(project.id)}
        currentPubkey={currentPubkey}
        sessionFilter={sessionFilters.getFilter(project.id)}
        onSessionFilterChange={(filter) =>
          sessionFilters.setFilter(project.id, filter)
        }
        onToggleCollapsed={() => collapse.toggleProject(project.id)}
        onRequestCloseCodingSession={(entry) => {
          if (!entry.sessionRef || !entry.genesisRef) return;
          codingSessionClosureDialog.requestClosure({
            action: "closed",
            channelId: entry.channelId,
            genesisRef: entry.genesisRef,
            label: entry.label,
            sessionRef: entry.sessionRef,
          });
        }}
        onRequestArchiveCodingSession={(entry) => {
          if (!entry.sessionRef || !entry.genesisRef) return;
          codingSessionClosureDialog.requestClosure({
            action: "archived",
            channelId: entry.channelId,
            genesisRef: entry.genesisRef,
            label: entry.label,
            sessionRef: entry.sessionRef,
          });
        }}
        activeCodingSessionKey={activeSessionKey}
        canDeleteCodingSession={(founderPubkey) =>
          capabilitiesFor(project).canDeleteResource(founderPubkey)
        }
        onRequestDeleteCodingSession={(entry) => {
          if (!entry.sessionRef) return;
          codingSessionDeleteDialog.requestDelete({
            channelId: entry.channelId,
            genesisRef: entry.genesisRef,
            label: entry.label,
            sessionRef: entry.sessionRef,
            stops: entry.stopTargets.map((stop) => ({ ...stop })),
            neverStarted: entry.founded === true,
          });
        }}
        onRequestReopenCodingSession={(entry) => {
          if (!entry.sessionRef || !entry.genesisRef) return;
          codingSessionClosureDialog.requestClosure({
            action: "open",
            channelId: entry.channelId,
            genesisRef: entry.genesisRef,
            label: entry.label,
            sessionRef: entry.sessionRef,
          });
        }}
        onOpenCodingSession={(coordinates) => {
          // A founded row sits in the generation slot under the founded row
          // id; it opens the founded route, never a generation that is not.
          const target = resolveProjectCodingSessionOpenTarget(coordinates);
          if (target.kind === "founded") {
            void goFoundedCodingSession(target.channelId, target.sessionRef);
          } else {
            void goCodingSession(target.channelId, target.generationId);
          }
        }}
        hotkeyPosition={hotkeyPositionByProjectId.get(project.id) ?? null}
        isProjectHomeActive={
          projectRouteId !== null &&
          (projectRouteId === project.id || projectRouteId === project.dtag)
        }
        onOpenProject={() => handleOpenProject(project)}
        onNewCodingSession={() => void goNewProjectCodingSession(project.id)}
        onRequestCreate={(kind) => setCreateRequest({ kind, project })}
        shellSessions={shellSessions}
        activeShellSessionId={activeShellSessionId}
        onOpenShell={handleOpenShell}
        onRequestRenameShell={shellDialogs.requestRename}
        onRequestCloseShell={shellDialogs.requestClose}
        onNewShell={
          builtinShellEnabled
            ? () => handleNewShell(project, Boolean(isFallback))
            : undefined
        }
        remoteTerminals={remoteTerminals}
        onObserveShell={handleObserveShell}
        todoLists={
          isFallback ? undefined : pinnedTodoLists.get(project.address)
        }
        activeTodoListId={
          projectRouteId === project.id || projectRouteId === project.dtag
            ? activeTodoListId
            : null
        }
        onOpenTodoList={(listId) =>
          void navigate({
            to: "/projects/$projectId/todos",
            params: { projectId: project.id },
            search: { list: listId, view: "list" },
          })
        }
        onNewTodoList={
          isFallback ? undefined : () => setTodoCreateProject(project)
        }
      />
    );
  };

  return (
    <>
      {/* Pull the Projects heading up against the primary menu above it —
          the scroll column's gap plus the menu's own bottom padding would
          otherwise read as a break between Agents and Projects. */}
      <SidebarGroup className="-mt-3 py-0">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              data-testid="open-projects-view"
              isActive={pathname === "/projects"}
              onClick={() => void goProjects({ filter: "projects" })}
              tooltip="Projects"
              type="button"
            >
              <FolderGit2 className="h-4 w-4" />
              <SidebarMenuLabel>Projects</SidebarMenuLabel>
              <ScopeActionBadge action="projects" />
            </SidebarMenuButton>
            <SidebarMenuAction
              aria-label="New project"
              data-testid="project-container-new"
              onClick={() => setCreateContainerOpen(true)}
              showOnHover
            >
              <Plus />
            </SidebarMenuAction>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarGroup>
      {pinnedProjects.map((project) => renderProject(project))}
      <ProjectSidebarDndContext
        projects={sortableProjects}
        onReorderProjects={reorderProjects}
      >
        {sortableProjects.map((project) => (
          <SortableProjectShell key={project.id} projectId={project.id}>
            {(drag) => renderProject(project, drag)}
          </SortableProjectShell>
        ))}
      </ProjectSidebarDndContext>
      {shellDialogs.dialogs}
      {codingSessionClosureDialog.dialog}
      {codingSessionDeleteDialog.dialog}

      <ProjectsScreenCreateDialogs
        kind={createRequest?.kind ?? null}
        targetProject={createRequest?.project ?? null}
        onClose={() => setCreateRequest(null)}
      />

      <CreateTodoListDialog
        isCreating={todoCreating}
        onCreate={handleCreateTodoList}
        onOpenChange={(open) => {
          if (!open) setTodoCreateProject(null);
        }}
        open={todoCreateProject !== null}
        projectName={todoCreateProject?.name ?? ""}
      />

      <CreateProjectContainerDialog
        isCreating={createContainerMutation.isPending}
        onCreate={async (input) => {
          const outcome = await createContainerMutation.mutateAsync(input);
          toastCreateProjectOutcome(outcome);
        }}
        onOpenChange={setCreateContainerOpen}
        open={createContainerOpen}
      />
    </>
  );
}
