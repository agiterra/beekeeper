import {
  Activity,
  Bot,
  Eye,
  FolderGit2,
  LoaderCircle,
  RotateCcw,
  Square,
  Terminal,
  Zap,
} from "lucide-react";

import type { RemoteTerminal } from "@/features/builtin-shell/observe/useProjectTerminals";
import { ShellSessionRow } from "@/features/builtin-shell/ui/ShellSessionRow";
import { ChannelSidebarRow } from "@/features/sidebar/ui/ChannelSidebarRow";
import type { ShellSessionInfo } from "@/shared/api/tauriShell";
import type { Workflow } from "@/shared/api/workflowTypes";
import type { Repository as CodeRepo } from "@/features/projects/hooks";
import { cn } from "@/shared/lib/cn";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/shared/ui/context-menu";
import { SidebarMenuButton, SidebarMenuItem } from "@/shared/ui/sidebar";

import type {
  ExactProjectCodingSessionCoordinates,
  ProjectCodingSessionShelfEntry,
} from "../lib/projectCodingSessionShelf";
import {
  projectChildLabel,
  type ProjectChildRow,
} from "../lib/projectChildren";
import {
  projectSessionObservationLabel,
  projectSessionObservationTitle,
} from "../lib/projectSessionObservation";
import type { ProjectChannelHandlers } from "./ProjectSidebarGroup";

/**
 * Renders one row of a project's flat child list; the row's type picks the
 * icon and behavior. Channel/forum and shell rows delegate to the shared row
 * components so they behave identically to their non-project counterparts.
 */
export function ProjectChildRowItem({
  row,
  channelHandlers,
  onOpenAgents,
  onOpenCodingSession,
  onOpenPulse,
  onRequestCloseCodingSession,
  onRequestReopenCodingSession,
  currentPubkey,
  onOpenRepo,
  onOpenWorkflow,
  activeShellSessionId,
  onOpenShell,
  onRequestRenameShell,
  onRequestCloseShell,
  onObserveShell,
}: {
  row: ProjectChildRow;
  channelHandlers: ProjectChannelHandlers;
  onOpenAgents: () => void;
  /** Opens this project's Pulse. Absent while the caller has not wired it —
   * the row then renders nothing rather than a control that does nothing. */
  onOpenPulse?: () => void;
  onOpenCodingSession?: (
    coordinates: ExactProjectCodingSessionCoordinates,
  ) => void;
  onRequestCloseCodingSession?: (entry: ProjectCodingSessionShelfEntry) => void;
  onRequestReopenCodingSession?: (
    entry: ProjectCodingSessionShelfEntry,
  ) => void;
  currentPubkey?: string;
  onOpenRepo: (repo: CodeRepo) => void;
  onOpenWorkflow?: (workflow: Workflow) => void;
  activeShellSessionId?: string;
  onOpenShell?: (sessionId: string) => void;
  onRequestRenameShell?: (session: ShellSessionInfo) => void;
  onRequestCloseShell?: (session: ShellSessionInfo) => void;
  onObserveShell?: (terminal: RemoteTerminal) => void;
}) {
  switch (row.type) {
    case "coding-session": {
      if (!onOpenCodingSession) return null;
      const { entry } = row;
      const details = [entry.sourceChannelLabel, entry.runtimeLabel]
        .filter(Boolean)
        .join(" · ");
      // Settled is intent, not activity: only the shared closure fact gets the
      // compact archival styling. Idle sessions keep the full row.
      const settled = entry.isClosed;
      const hasClosureCoordinates = Boolean(
        entry.sessionRef && entry.genesisRef,
      );
      const canClose = Boolean(
        !settled &&
          hasClosureCoordinates &&
          currentPubkey &&
          entry.founderPubkey?.toLowerCase() === currentPubkey.toLowerCase() &&
          onRequestCloseCodingSession,
      );
      const canReopen = Boolean(
        settled &&
          hasClosureCoordinates &&
          currentPubkey &&
          onRequestReopenCodingSession,
      );
      // A pending row stands for a create the provider has not acknowledged
      // yet — there is no generation to open, so the row is presence-only.
      const pending = entry.pending === true;
      const button = (
        <SidebarMenuButton
          aria-label={
            pending
              ? `${entry.label}, starting`
              : `Open ${entry.label}${details ? `, ${details}` : ""}`
          }
          className={cn(
            settled ? "h-8 py-0" : "h-auto min-h-8 py-1.5",
            settled &&
              "text-sidebar-foreground/65 hover:text-sidebar-accent-foreground",
            pending && "cursor-default",
          )}
          data-testid={
            pending
              ? "project-coding-session-row-pending"
              : "project-coding-session-row"
          }
          onClick={
            pending
              ? undefined
              : () =>
                  onOpenCodingSession({
                    channelId: entry.channelId,
                    generationId: entry.generationId,
                  })
          }
          type="button"
          title={
            pending ? "Waiting for the session provider" : details || undefined
          }
        >
          <span
            className={cn(
              "flex shrink-0 items-center justify-center text-sidebar-foreground/65",
              settled
                ? "size-4 opacity-55"
                : "size-6 rounded-full bg-sidebar-accent",
            )}
          >
            {pending ? (
              <LoaderCircle className="size-3.5 animate-spin" />
            ) : (
              <Bot className="size-3.5" />
            )}
          </span>
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="truncate">{entry.label}</span>
            {details && !settled ? (
              <span className="truncate text-2xs font-normal text-sidebar-foreground/50">
                {details}
              </span>
            ) : null}
          </span>
          <span
            className="ml-1 flex shrink-0 items-center gap-1 text-2xs text-sidebar-foreground/45"
            title={
              pending || settled
                ? undefined
                : projectSessionObservationTitle(entry.status)
            }
          >
            {/* A pending row has not produced a provider observation yet. */}
            {pending
              ? "Starting…"
              : settled
                ? "Closed"
                : projectSessionObservationLabel(entry.status)}
          </span>
        </SidebarMenuButton>
      );
      return (
        <SidebarMenuItem
          data-session-closure={settled ? "closed" : "open"}
          data-session-status={entry.status.kind}
        >
          {canClose || canReopen ? (
            <ContextMenu>
              <ContextMenuTrigger asChild>{button}</ContextMenuTrigger>
              <ContextMenuContent>
                {canClose ? (
                  <ContextMenuItem
                    data-testid="project-coding-session-close"
                    onSelect={() => onRequestCloseCodingSession?.(entry)}
                  >
                    <Square />
                    Close session
                  </ContextMenuItem>
                ) : null}
                {canReopen ? (
                  <ContextMenuItem
                    data-testid="project-coding-session-reopen"
                    onSelect={() => onRequestReopenCodingSession?.(entry)}
                  >
                    <RotateCcw />
                    Reopen session
                  </ContextMenuItem>
                ) : null}
              </ContextMenuContent>
            </ContextMenu>
          ) : (
            button
          )}
        </SidebarMenuItem>
      );
    }
    case "pulse": {
      // Presence is not conditional on content: the row stays whether or not
      // Pulse data exists, and the screen renders the confirmed-empty state.
      // A row that vanished when a project went quiet would make "no Pulse"
      // and "no project" look identical in the sidebar.
      if (!onOpenPulse) return null;
      return (
        <SidebarMenuItem>
          <SidebarMenuButton
            data-testid="project-pulse-row"
            onClick={onOpenPulse}
            title="Explicit updates and observed session state"
            type="button"
          >
            <Activity className="size-4 shrink-0" />
            <span className="truncate">{projectChildLabel(row)}</span>
          </SidebarMenuButton>
        </SidebarMenuItem>
      );
    }
    case "channel":
    case "forum": {
      const { channel } = row;
      return (
        <ChannelSidebarRow
          channel={channel}
          activeWorking={channelHandlers.activeWorkingByChannelId?.get(
            channel.id,
          )}
          hasUnread={channelHandlers.unreadChannelIds.has(channel.id)}
          unreadCount={channelHandlers.unreadChannelCounts.get(channel.id) ?? 0}
          isMuted={channelHandlers.mutedChannelIds?.has(channel.id)}
          isStarred={channelHandlers.starredChannelIds?.has(channel.id)}
          isActive={
            channelHandlers.isActiveChannel &&
            channelHandlers.selectedChannelId === channel.id
          }
          onSelectChannel={channelHandlers.onSelectChannel}
          onMarkChannelRead={channelHandlers.onMarkChannelRead}
          onMarkChannelUnread={channelHandlers.onMarkChannelUnread}
          onMuteChannel={channelHandlers.onMuteChannel}
          onUnmuteChannel={channelHandlers.onUnmuteChannel}
          onStarChannel={channelHandlers.onStarChannel}
          onUnstarChannel={channelHandlers.onUnstarChannel}
          onDeleteChannel={channelHandlers.onDeleteChannel}
          onLeaveChannel={channelHandlers.onLeaveChannel}
        />
      );
    }
    case "repo":
      return (
        <SidebarMenuItem>
          <SidebarMenuButton
            onClick={() => onOpenRepo(row.repo)}
            data-testid="project-code-row"
          >
            <FolderGit2 className="size-4 shrink-0" />
            <span className="truncate">{row.repo.name}</span>
          </SidebarMenuButton>
        </SidebarMenuItem>
      );
    case "workflow": {
      if (!onOpenWorkflow) return null;
      const { workflow } = row;
      return (
        <SidebarMenuItem>
          <SidebarMenuButton
            onClick={() => onOpenWorkflow(workflow)}
            data-testid="project-workflow-row"
          >
            <Zap className="size-4 shrink-0" />
            <span className="truncate">{workflow.name}</span>
          </SidebarMenuButton>
        </SidebarMenuItem>
      );
    }
    case "agent":
      return (
        <SidebarMenuItem>
          <SidebarMenuButton
            onClick={onOpenAgents}
            data-testid="project-agent-row"
          >
            <Bot className="size-4 shrink-0" />
            <span className="truncate">{row.agent.label}</span>
          </SidebarMenuButton>
        </SidebarMenuItem>
      );
    case "shell": {
      if (!onOpenShell || !onRequestRenameShell || !onRequestCloseShell) {
        return null;
      }
      return (
        <ShellSessionRow
          session={row.session}
          isActive={row.session.sessionId === activeShellSessionId}
          onOpen={onOpenShell}
          onRequestRename={onRequestRenameShell}
          onRequestClose={onRequestCloseShell}
        />
      );
    }
    case "remote-shell": {
      if (!onObserveShell) return null;
      const { terminal } = row;
      return (
        <SidebarMenuItem>
          <SidebarMenuButton
            onClick={() => onObserveShell(terminal)}
            data-testid="project-remote-shell-row"
            title="A member's shared terminal — open read-only"
          >
            <Terminal className="size-4 shrink-0" />
            <span className="truncate">{terminal.title}</span>
            <Eye className="ml-auto size-3.5 shrink-0 text-sidebar-foreground/50" />
          </SidebarMenuButton>
        </SidebarMenuItem>
      );
    }
  }
}
