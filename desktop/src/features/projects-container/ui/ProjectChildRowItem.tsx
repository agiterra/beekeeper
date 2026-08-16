import { Bot, Circle, Eye, FolderGit2, Terminal, Zap } from "lucide-react";

import type { RemoteTerminal } from "@/features/builtin-shell/observe/useProjectTerminals";
import { ShellSessionRow } from "@/features/builtin-shell/ui/ShellSessionRow";
import { ChannelSidebarRow } from "@/features/sidebar/ui/ChannelSidebarRow";
import type { ShellSessionInfo } from "@/shared/api/tauriShell";
import type { Workflow } from "@/shared/api/workflowTypes";
import type { Repository as CodeRepo } from "@/features/projects/hooks";
import { cn } from "@/shared/lib/cn";
import { SidebarMenuButton, SidebarMenuItem } from "@/shared/ui/sidebar";

import type { ExactProjectCodingSessionCoordinates } from "../lib/projectCodingSessionShelf";
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
      return (
        <SidebarMenuItem data-session-status={entry.status.kind}>
          <SidebarMenuButton
            aria-label={`Open ${entry.label}${details ? `, ${details}` : ""}`}
            className={cn(
              "h-auto min-h-8 py-1.5",
              entry.status.kind === "idle" &&
                "text-sidebar-foreground/65 hover:text-sidebar-accent-foreground",
            )}
            data-testid="project-coding-session-row"
            onClick={() =>
              onOpenCodingSession({
                channelId: entry.channelId,
                generationId: entry.generationId,
              })
            }
            type="button"
          >
            <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-sidebar-accent text-sidebar-foreground/65">
              <Bot className="size-3.5" />
            </span>
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="truncate">{entry.label}</span>
              {details ? (
                <span className="truncate text-2xs font-normal text-sidebar-foreground/50">
                  {details}
                </span>
              ) : null}
            </span>
            <span
              className={cn(
                "ml-1 flex shrink-0 items-center gap-1 text-2xs",
                entry.status.kind === "working"
                  ? "text-emerald-500"
                  : "text-sidebar-foreground/45",
              )}
            >
              {entry.status.kind === "working" ? (
                <Circle className="size-1.5 fill-current" aria-hidden />
              ) : null}
              {entry.status.label}
            </span>
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
