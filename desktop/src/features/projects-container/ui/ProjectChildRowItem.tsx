import { Bot, FolderGit2, Zap } from "lucide-react";

import { ChannelSidebarRow } from "@/features/sidebar/ui/ChannelSidebarRow";
import type { Workflow } from "@/shared/api/workflowTypes";
import type { Repository as CodeRepo } from "@/features/projects/hooks";
import { SidebarMenuButton, SidebarMenuItem } from "@/shared/ui/sidebar";

import type { ProjectChildRow } from "../lib/projectChildren";
import type { ProjectChannelHandlers } from "./ProjectSidebarGroup";

/**
 * Renders one row of a project's flat child list; the row's type picks the
 * icon and behavior. Channel/forum rows delegate to the shared row component
 * so they behave identically to their non-project counterparts.
 */
export function ProjectChildRowItem({
  row,
  channelHandlers,
  onOpenAgents,
  onOpenRepo,
  onOpenWorkflow,
}: {
  row: ProjectChildRow;
  channelHandlers: ProjectChannelHandlers;
  onOpenAgents: () => void;
  onOpenRepo: (repo: CodeRepo) => void;
  onOpenWorkflow?: (workflow: Workflow) => void;
}) {
  switch (row.type) {
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
  }
}
