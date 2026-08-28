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
import { useUsersBatchQuery } from "@/features/profile/hooks";
import type { ShellSessionInfo } from "@/shared/api/tauriShell";
import type { Channel } from "@/shared/api/types";
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
import { buildProjectChildren, projectChildKey } from "../lib/projectChildren";
import {
  PROJECT_SESSION_PAGE_SIZE,
  filterProjectSessions,
  projectSessionFounders,
  type ProjectSessionFilter,
} from "../lib/projectSessionFilter";
import { withoutProjectSessionTransportChannels } from "../lib/projectSessionsChannel";
import { ProjectChildRowItem } from "./ProjectChildRowItem";
import { ProjectSessionFilterMenu } from "./ProjectSessionFilterMenu";

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
 * One collapsible project group in the sidebar: the project's channels, then
 * its sessions (coding sessions and terminals) with a filter beneath them.
 * Repositories, workflows, agents and Pulse are the project page's business.
 */
export function ProjectSidebarGroup({
  project,
  isFallback,
  codingSessions,
  streamChannels,
  forumChannels,
  channelHandlers,
  collapsed,
  onToggleCollapsed,
  onOpenCodingSession,
  onRequestCloseCodingSession,
  onRequestArchiveCodingSession,
  onRequestReopenCodingSession,
  currentPubkey,
  sessionFilter,
  onSessionFilterChange,
  onOpenProject,
  onNewCodingSession,
  onRequestCreate,
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
  /** Trusted sessions this project owns, already in shelf (activity) order. */
  codingSessions?: ProjectCodingSessionShelfEntry[];
  streamChannels: Channel[];
  forumChannels: Channel[];
  channelHandlers: ProjectChannelHandlers;
  collapsed: boolean;
  onToggleCollapsed: () => void;
  onOpenCodingSession?: (
    coordinates: ExactProjectCodingSessionCoordinates,
  ) => void;
  onRequestCloseCodingSession?: (entry: ProjectCodingSessionShelfEntry) => void;
  onRequestArchiveCodingSession?: (
    entry: ProjectCodingSessionShelfEntry,
  ) => void;
  onRequestReopenCodingSession?: (
    entry: ProjectCodingSessionShelfEntry,
  ) => void;
  currentPubkey?: string;
  /** Which sessions to list; owned by the sections parent so it persists. */
  sessionFilter: ProjectSessionFilter;
  onSessionFilterChange: (filter: ProjectSessionFilter) => void;
  onOpenProject: () => void;
  /** Starts the project-scoped create flow from the group's `+` menu. */
  onNewCodingSession?: () => void;
  /** Opens a project-scoped create dialog (hosted by the sections parent so
   * a single dialog instance serves every group). */
  onRequestCreate?: (kind: "channel" | "forum") => void;
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

  // Filter before ordering: the list shows every session that passes, open
  // work first in shelf (activity) order, then the closed ones dimmed.
  const allSessions = codingSessions ?? [];
  const filtered = React.useMemo(
    () => filterProjectSessions(allSessions, sessionFilter, currentPubkey),
    [allSessions, sessionFilter, currentPubkey],
  );
  const sessionFounders = React.useMemo(
    () => projectSessionFounders(allSessions),
    [allSessions],
  );

  const children = React.useMemo(
    () =>
      buildProjectChildren({
        codingSessions: filtered.shown,
        streamChannels: visibleStreamChannels,
        forumChannels,
        shellSessions,
        remoteTerminals,
      }),
    [
      filtered.shown,
      visibleStreamChannels,
      forumChannels,
      shellSessions,
      remoteTerminals,
    ],
  );

  const sessionRows = children.filter((row) => row.type === "coding-session");
  // "Settled" is the shared closure fact (kind 44230), never inferred from a
  // provider's execution status. Stopped executions remain open work until
  // someone with session authority closes the umbrella. Order: open, then
  // closed, then archived — the shelf comparator already sorts them so.
  const openSessionRows = sessionRows.filter((row) => !row.entry.isClosed);
  const settledSessionRows = sessionRows.filter((row) => row.entry.isClosed);

  // Paging: ten session rows at a time, "Show more" adding ten. The page
  // resets whenever the filter changes so a narrower list starts at the top.
  const [pageCount, setPageCount] = React.useState(1);
  const filterKey = JSON.stringify(sessionFilter);
  const [pagedFilterKey, setPagedFilterKey] = React.useState(filterKey);
  if (pagedFilterKey !== filterKey) {
    setPagedFilterKey(filterKey);
    setPageCount(1);
  }
  const visibleSessionLimit = pageCount * PROJECT_SESSION_PAGE_SIZE;
  const visibleOpenRows = openSessionRows.slice(0, visibleSessionLimit);
  const visibleSettledRows = settledSessionRows.slice(
    0,
    Math.max(0, visibleSessionLimit - visibleOpenRows.length),
  );
  const remainingSessions =
    sessionRows.length - visibleOpenRows.length - visibleSettledRows.length;
  const channelRows = children.filter(
    (row) => row.type === "channel" || row.type === "forum",
  );
  // Terminals live with sessions: both are interactive work surfaces.
  const terminalRows = children.filter(
    (row) => row.type === "shell" || row.type === "remote-shell",
  );

  // One batched profile read for every founder on screen; rows never query.
  const founderPubkeys = React.useMemo(
    () => projectSessionFounders(filtered.shown),
    [filtered.shown],
  );
  const founderProfiles = useUsersBatchQuery(founderPubkeys).data?.profiles;

  const renderRow = (row: (typeof children)[number]) => (
    <ProjectChildRowItem
      key={projectChildKey(row)}
      row={row}
      channelHandlers={channelHandlers}
      onOpenCodingSession={onOpenCodingSession}
      onRequestCloseCodingSession={onRequestCloseCodingSession}
      onRequestArchiveCodingSession={onRequestArchiveCodingSession}
      onRequestReopenCodingSession={onRequestReopenCodingSession}
      currentPubkey={currentPubkey}
      founderProfiles={founderProfiles}
      activeShellSessionId={activeShellSessionId}
      onOpenShell={onOpenShell}
      onRequestRenameShell={onRequestRenameShell}
      onRequestCloseShell={onRequestCloseShell}
      onObserveShell={onObserveShell}
    />
  );

  // The filter renders whenever the project has any session at all — even
  // when the current filter hides every one of them, otherwise a "My
  // sessions" choice that matches nothing would be impossible to undo.
  const hasAnySession = allSessions.length > 0 || terminalRows.length > 0;

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
          <div
            className="flex flex-col gap-1 px-2 pb-1"
            data-testid={`project-children-${project.dtag}`}
          >
            {channelRows.length > 0 ? (
              <SidebarMenu
                aria-label={`${project.name} channels`}
                data-testid={`project-channels-${project.dtag}`}
              >
                {channelRows.map(renderRow)}
              </SidebarMenu>
            ) : null}
            {hasAnySession ? (
              <>
                <SidebarMenu
                  aria-label={`${project.name} sessions`}
                  data-testid={`project-sessions-${project.dtag}`}
                >
                  {visibleOpenRows.map(renderRow)}
                  {terminalRows.map(renderRow)}
                  {visibleSettledRows.map(renderRow)}
                  {remainingSessions > 0 ? (
                    <SidebarMenuItem>
                      <SidebarMenuButton
                        className="h-7 text-2xs text-sidebar-foreground/55"
                        data-testid={`project-sessions-show-more-${project.dtag}`}
                        onClick={() => setPageCount((count) => count + 1)}
                        type="button"
                      >
                        <span className="pl-6">
                          Show more (
                          {Math.min(
                            remainingSessions,
                            PROJECT_SESSION_PAGE_SIZE,
                          )}{" "}
                          of {remainingSessions})
                        </span>
                      </SidebarMenuButton>
                    </SidebarMenuItem>
                  ) : null}
                </SidebarMenu>
                <ProjectSessionFilterMenu
                  currentPubkey={currentPubkey}
                  filter={sessionFilter}
                  hiddenByState={filtered.hiddenByState}
                  hiddenUnattributed={filtered.hiddenUnattributed}
                  isFallback={isFallback}
                  onChange={onSessionFilterChange}
                  project={project}
                  sessionFounders={sessionFounders}
                />
              </>
            ) : null}
          </div>
        </SidebarGroupContent>
      )}
    </SidebarGroup>
  );
}
