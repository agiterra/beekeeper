import * as React from "react";
import {
  CheckCheck,
  ChevronDown,
  FileText,
  FolderKanban,
  Hash,
  Lock,
  Plus,
  Terminal,
} from "lucide-react";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

import { cn } from "@/shared/lib/cn";
import { deferMenuAction } from "@/features/sidebar/ui/sidebarMenuHelpers";
import type { ActiveChannelTurnSummary } from "@/features/agents/activeAgentTurnsStore";
import type { RemoteTerminal } from "@/features/builtin-shell/observe/useProjectTerminals";
import type { ShellSessionInfo } from "@/shared/api/tauriShell";
import type { Channel } from "@/shared/api/types";
import type { Workflow } from "@/shared/api/workflowTypes";
import type { Repository as CodeRepo } from "@/features/projects/hooks";
import { useFeatureEnabled } from "@/shared/features";
import {
  SidebarGroup,
  SidebarGroupContent,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/shared/ui/sidebar";

import type { ProjectContainer } from "../hooks";
import type {
  ExactProjectCodingSessionCoordinates,
  ProjectCodingSessionShelfEntry,
} from "../lib/projectCodingSessionShelf";
import {
  buildProjectChildren,
  projectChildKey,
  type ProjectAgentRow,
} from "../lib/projectChildren";
import { withoutProjectSessionTransportChannels } from "../lib/projectSessionsChannel";
import { ProjectChildRowItem } from "./ProjectChildRowItem";

/**
 * How many session rows a project shows inline. Sessions accumulate faster
 * than any other child, and a project with a year of history must not push its
 * channels off the bottom of the sidebar — the rest stay on the project screen.
 */
export const PROJECT_SIDEBAR_SESSION_LIMIT = 5;

/** Channel-row handlers shared by every project group, lifted once from
 * AppSidebar so each group can render real channel rows. */
export type ProjectChannelHandlers = {
  isActiveChannel: boolean;
  activeWorkingByChannelId?: ReadonlyMap<string, ActiveChannelTurnSummary>;
  selectedChannelId: string | null;
  unreadChannelCounts: ReadonlyMap<string, number>;
  unreadChannelIds: ReadonlySet<string>;
  mutedChannelIds?: ReadonlySet<string>;
  starredChannelIds?: ReadonlySet<string>;
  onSelectChannel: (channelId: string) => void;
  onMarkChannelRead: (
    channelId: string,
    lastMessageAt: string | null | undefined,
  ) => void;
  onMarkChannelUnread: (channelId: string) => void;
  onMuteChannel?: (channelId: string) => void;
  onUnmuteChannel?: (channelId: string) => void;
  onStarChannel?: (channelId: string) => void;
  onUnstarChannel?: (channelId: string) => void;
  onDeleteChannel?: (channel: Channel) => void;
  onLeaveChannel?: (channel: Channel) => void;
};

/**
 * One collapsible project group in the sidebar: a single flat list of every
 * child the project owns — channels, forums, repos, workflows, agents, and
 * shells — type-ranked and identified by icon. Collapsing the header hides
 * all of it.
 */
export function ProjectSidebarGroup({
  project,
  isFallback,
  agents,
  codingSessions,
  streamChannels,
  forumChannels,
  repos,
  channelHandlers,
  collapsed,
  onToggleCollapsed,
  onOpenAgents,
  onOpenCodingSession,
  onOpenProject,
  onOpenRepo,
  onNewCodingSession,
  onRequestCreate,
  workflows,
  onOpenWorkflow,
  shellSessions,
  activeShellSessionId,
  onOpenShell,
  onRequestRenameShell,
  onRequestCloseShell,
  onNewShell,
  remoteTerminals,
  onObserveShell,
}: {
  project: ProjectContainer;
  /** True for the locally-synthesized General bucket that exists before the
   * workspace owner has published a real `general` project event. */
  isFallback?: boolean;
  agents: ProjectAgentRow[];
  /** Trusted sessions this project owns, already in shelf (activity) order. */
  codingSessions?: ProjectCodingSessionShelfEntry[];
  streamChannels: Channel[];
  forumChannels: Channel[];
  repos: CodeRepo[];
  channelHandlers: ProjectChannelHandlers;
  collapsed: boolean;
  onToggleCollapsed: () => void;
  onOpenAgents: () => void;
  onOpenCodingSession?: (
    coordinates: ExactProjectCodingSessionCoordinates,
  ) => void;
  onOpenProject: () => void;
  onOpenRepo: (repo: CodeRepo) => void;
  /** Starts the project-scoped create flow from the group's `+` menu. */
  onNewCodingSession?: () => void;
  /** Opens a project-scoped create dialog (hosted by the sections parent so
   * a single dialog instance serves every group). */
  onRequestCreate?: (kind: "channel" | "forum") => void;
  /** Workflows whose trigger channel belongs to this project (derived — a
   * kind:30620 def is always channel-scoped via its `h` tag). */
  workflows?: Workflow[];
  onOpenWorkflow?: (workflow: Workflow) => void;
  shellSessions: ShellSessionInfo[];
  activeShellSessionId?: string;
  onOpenShell: (sessionId: string) => void;
  onRequestRenameShell: (session: ShellSessionInfo) => void;
  onRequestCloseShell: (session: ShellSessionInfo) => void;
  /** Undefined when the builtin-shell experiment is off — hides the shell
   * item in the create menu. */
  onNewShell?: () => void;
  /** Other members' shared terminals in this project (NIP-ST announces). */
  remoteTerminals?: RemoteTerminal[];
  onObserveShell?: (terminal: RemoteTerminal) => void;
}) {
  const { unreadChannelIds, onMarkChannelRead } = channelHandlers;
  const forumEnabled = useFeatureEnabled("forum");
  const visibleStreamChannels = React.useMemo(
    () =>
      withoutProjectSessionTransportChannels({
        projectName: project.name,
        channels: streamChannels,
        codingSessions: codingSessions ?? [],
      }),
    [codingSessions, project.name, streamChannels],
  );

  const hasUnread = React.useMemo(
    () =>
      [...visibleStreamChannels, ...forumChannels].some((channel) =>
        unreadChannelIds.has(channel.id),
      ),
    [visibleStreamChannels, forumChannels, unreadChannelIds],
  );

  const markAllRead = React.useCallback(() => {
    for (const channel of [...visibleStreamChannels, ...forumChannels]) {
      onMarkChannelRead(channel.id, channel.lastMessageAt);
    }
  }, [visibleStreamChannels, forumChannels, onMarkChannelRead]);

  const children = React.useMemo(
    () =>
      buildProjectChildren({
        codingSessions: (codingSessions ?? []).slice(
          0,
          PROJECT_SIDEBAR_SESSION_LIMIT,
        ),
        streamChannels: visibleStreamChannels,
        forumChannels,
        repos,
        workflows: workflows ?? [],
        agents,
        shellSessions,
        remoteTerminals,
      }),
    [
      codingSessions,
      visibleStreamChannels,
      forumChannels,
      repos,
      workflows,
      agents,
      shellSessions,
      remoteTerminals,
    ],
  );

  const childRows = children.map((row) => (
    <ProjectChildRowItem
      key={projectChildKey(row)}
      row={row}
      channelHandlers={channelHandlers}
      onOpenAgents={onOpenAgents}
      onOpenCodingSession={onOpenCodingSession}
      onOpenRepo={onOpenRepo}
      onOpenWorkflow={onOpenWorkflow}
      activeShellSessionId={activeShellSessionId}
      onOpenShell={onOpenShell}
      onRequestRenameShell={onRequestRenameShell}
      onRequestCloseShell={onRequestCloseShell}
      onObserveShell={onObserveShell}
    />
  ));

  return (
    <SidebarGroup
      className="group/sidebar-section py-0 pl-4"
      data-testid={`project-group-${project.dtag}`}
    >
      <div className="relative flex items-center">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              type="button"
              onClick={onOpenProject}
              className="pr-14"
              data-testid={`project-open-${project.dtag}`}
              tooltip={project.name}
            >
              <FolderKanban />
              <span className="truncate">{project.name}</span>
              {project.visibility === "private" ? (
                <span className="flex shrink-0 items-center">
                  <Lock
                    aria-label="Private project"
                    className="size-3 text-sidebar-foreground/45"
                    data-testid={`project-lock-${project.dtag}`}
                  />
                </span>
              ) : null}
              {isFallback ? (
                <span className="text-2xs text-sidebar-foreground/45">
                  (local)
                </span>
              ) : null}
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
        <div className="absolute right-1 top-1/2 flex -translate-y-1/2 items-center">
          {onRequestCreate || onNewShell || onNewCodingSession || hasUnread ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  aria-label={`Create in ${project.name}`}
                  data-testid={`project-create-${project.dtag}`}
                  className="flex size-6 items-center justify-center rounded-md text-sidebar-foreground/45 opacity-0 transition-colors hover:text-sidebar-foreground focus-visible:opacity-100 group-hover/sidebar-section:opacity-100 data-[state=open]:opacity-100"
                >
                  <Plus className="size-4" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                {onRequestCreate ? (
                  <DropdownMenuItem
                    data-testid={`project-new-channel-${project.dtag}`}
                    onSelect={() =>
                      deferMenuAction(() => onRequestCreate("channel"))
                    }
                  >
                    <Hash />
                    New channel
                  </DropdownMenuItem>
                ) : null}
                {onRequestCreate && forumEnabled ? (
                  <DropdownMenuItem
                    data-testid={`project-new-forum-${project.dtag}`}
                    onSelect={() =>
                      deferMenuAction(() => onRequestCreate("forum"))
                    }
                  >
                    <FileText />
                    New forum
                  </DropdownMenuItem>
                ) : null}
                {onNewCodingSession ? (
                  <DropdownMenuItem
                    data-testid={`project-new-coding-session-${project.dtag}`}
                    onSelect={() => deferMenuAction(onNewCodingSession)}
                  >
                    <Terminal />
                    New coding session
                  </DropdownMenuItem>
                ) : null}
                {onNewShell ? (
                  <DropdownMenuItem
                    data-testid={`project-new-shell-${project.dtag}`}
                    onSelect={() => deferMenuAction(onNewShell)}
                  >
                    <Terminal />
                    New terminal
                  </DropdownMenuItem>
                ) : null}
                {hasUnread ? (
                  <>
                    {onRequestCreate || onNewShell || onNewCodingSession ? (
                      <DropdownMenuSeparator />
                    ) : null}
                    <DropdownMenuItem
                      data-testid={`project-mark-all-read-${project.dtag}`}
                      onSelect={() => deferMenuAction(markAllRead)}
                    >
                      <CheckCheck />
                      Mark all as read
                    </DropdownMenuItem>
                  </>
                ) : null}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : null}
          <button
            type="button"
            onClick={onToggleCollapsed}
            aria-expanded={!collapsed}
            aria-label={`${collapsed ? "Expand" : "Collapse"} ${project.name}`}
            data-testid={`project-group-toggle-${project.dtag}`}
            className="flex size-6 items-center justify-center rounded-md text-sidebar-foreground/45 transition-colors hover:text-sidebar-foreground"
          >
            <ChevronDown
              className={cn(
                "size-4 transition-transform",
                collapsed ? "-rotate-90" : "rotate-0",
              )}
            />
          </button>
        </div>
      </div>
      {collapsed ? null : (
        <SidebarGroupContent className="pl-2">
          <SidebarMenu
            className="px-2"
            data-testid={`project-children-${project.dtag}`}
          >
            {childRows}
          </SidebarMenu>
        </SidebarGroupContent>
      )}
    </SidebarGroup>
  );
}
