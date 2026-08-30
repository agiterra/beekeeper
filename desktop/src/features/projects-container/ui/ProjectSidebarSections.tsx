import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate, useParams } from "@tanstack/react-router";
import { FolderGit2, Plus } from "lucide-react";
import { toast } from "sonner";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { useCreateShellSession } from "@/features/builtin-shell/hooks/useCreateShellSession";
import { projectDefaultCwd } from "@/features/builtin-shell/lib/projectShellCwd";
import {
  useRemoteTerminalsIndex,
  type RemoteTerminal,
} from "@/features/builtin-shell/observe/useProjectTerminals";
import { useShellSessionDialogs } from "@/features/builtin-shell/hooks/useShellSessionDialogs";
import { useShellSessions } from "@/features/builtin-shell/hooks/useShellSessions";
import { useCodingSessionClosureDialog } from "@/features/coding-sessions/hooks/useCodingSessionClosureDialog";
import { compareProjectCodingSessionEntries } from "../lib/projectCodingSessionShelf";
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
  unclaimedForums: Channel[];
  /** Streams no project claims yet. Channels must belong to a project, so
   * these display under General until they're claimed or moved. */
  globalChannels: Channel[];
  currentPubkey?: string;
  relayUrl?: string;
}) {
  const { goCodingSession, goNewProjectCodingSession, goProject, goProjects } =
    useAppNavigation();
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
  const activeShellSessionId = useParams({
    strict: false,
    select: (p) => (p as { sessionId?: string }).sessionId,
  });
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

  // Coding sessions are channel-scoped the same way, except a session may also
  // carry a signed projectRef that overrides its channel's project.
  const sessionBuckets = useProjectCodingSessionBuckets(
    channelsQuery.data,
    channelsByProject,
    forumsByProject,
  );

  const queryClient = useQueryClient();
  const [createContainerOpen, setCreateContainerOpen] = React.useState(false);
  // One dialog instance serves every group's "+" menu; the request carries
  // the target project so new channels/forums land inside it.
  const [createRequest, setCreateRequest] = React.useState<{
    kind: ProjectsScreenCreateKind;
    project: ProjectContainer;
  } | null>(null);

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
        onOpenCodingSession={({ channelId, generationId }) =>
          void goCodingSession(channelId, generationId)
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
              onClick={() => void goProjects({ filter: "projects" })}
              tooltip="Projects"
              type="button"
            >
              <FolderGit2 className="h-4 w-4" />
              <SidebarMenuLabel>Projects</SidebarMenuLabel>
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

      <ProjectsScreenCreateDialogs
        kind={createRequest?.kind ?? null}
        targetProject={createRequest?.project ?? null}
        onClose={() => setCreateRequest(null)}
      />

      <CreateProjectContainerDialog
        isCreating={createContainerMutation.isPending}
        onCreate={async (input) => {
          await createContainerMutation.mutateAsync(input);
        }}
        onOpenChange={setCreateContainerOpen}
        open={createContainerOpen}
      />
    </>
  );
}
