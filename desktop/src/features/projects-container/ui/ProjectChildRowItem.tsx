import {
  Bot,
  Circle,
  Eye,
  FolderGit2,
  LoaderCircle,
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
import type { ProjectChildRow } from "../lib/projectChildren";
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
  onRequestEndCodingSession,
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
  onOpenCodingSession?: (
    coordinates: ExactProjectCodingSessionCoordinates,
  ) => void;
  onRequestEndCodingSession?: (entry: ProjectCodingSessionShelfEntry) => void;
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
      // Settled is intent, not activity: only a user-ended session gets the
      // compact archival styling. Idle sessions keep the full row.
      const settled = entry.status.kind === "ended";
      const canEnd =
        onRequestEndCodingSession !== undefined &&
        entry.status.kind !== "ended" &&
        entry.stopTargets.length > 0;
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
            className={cn(
              "ml-1 flex shrink-0 items-center gap-1 text-2xs",
              !pending && entry.status.kind === "working"
                ? "text-emerald-500"
                : "text-sidebar-foreground/45",
            )}
          >
            {!pending && entry.status.kind === "working" ? (
              <Circle className="size-1.5 fill-current" aria-hidden />
            ) : null}
            {/* A pending row's Working/Idle is only a prediction — say what
                is actually happening instead. */}
            {pending ? "Starting…" : entry.status.label}
          </span>
        </SidebarMenuButton>
      );
      return (
        <SidebarMenuItem data-session-status={entry.status.kind}>
          {canEnd ? (
            <ContextMenu>
              <ContextMenuTrigger asChild>{button}</ContextMenuTrigger>
              <ContextMenuContent>
                <ContextMenuItem
                  className="text-destructive focus:text-destructive"
                  data-testid="project-coding-session-end"
                  onSelect={() => onRequestEndCodingSession(entry)}
                >
                  <Square />
                  End session
                </ContextMenuItem>
              </ContextMenuContent>
            </ContextMenu>
          ) : (
            button
          )}
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
