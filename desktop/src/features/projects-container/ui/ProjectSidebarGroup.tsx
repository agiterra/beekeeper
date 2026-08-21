import * as React from "react";
import {
  CheckCheck,
  ChevronDown,
  FileText,
  FolderKanban,
  Hash,
  Plus,
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
import {
  buildProjectChildren,
  projectChildKey,
  type ProjectAgentRow,
} from "../lib/projectChildren";
import { ProjectChildRowItem } from "./ProjectChildRowItem";

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
 * child the project owns — channels, forums, repos, workflows, and agents —
 * type-ranked and identified by icon. Collapsing the header hides all of it.
 */
export function ProjectSidebarGroup({
  project,
  isFallback,
  agents,
  streamChannels,
  forumChannels,
  repos,
  channelHandlers,
  collapsed,
  onToggleCollapsed,
  onOpenAgents,
  onOpenProject,
  onOpenRepo,
  onRequestCreate,
  workflows,
  onOpenWorkflow,
}: {
  project: ProjectContainer;
  /** True for the locally-synthesized General bucket that exists before the
   * workspace owner has published a real `general` project event. */
  isFallback?: boolean;
  agents: ProjectAgentRow[];
  streamChannels: Channel[];
  forumChannels: Channel[];
  repos: CodeRepo[];
  channelHandlers: ProjectChannelHandlers;
  collapsed: boolean;
  onToggleCollapsed: () => void;
  onOpenAgents: () => void;
  onOpenProject: () => void;
  onOpenRepo: (repo: CodeRepo) => void;
  /** Opens a project-scoped create dialog (hosted by the sections parent so
   * a single dialog instance serves every group). */
  onRequestCreate?: (kind: "channel" | "forum") => void;
  /** Workflows whose trigger channel belongs to this project (derived — a
   * kind:30620 def is always channel-scoped via its `h` tag). */
  workflows?: Workflow[];
  onOpenWorkflow?: (workflow: Workflow) => void;
}) {
  const { unreadChannelIds, onMarkChannelRead } = channelHandlers;
  const forumEnabled = useFeatureEnabled("forum");

  const hasUnread = React.useMemo(
    () =>
      [...streamChannels, ...forumChannels].some((channel) =>
        unreadChannelIds.has(channel.id),
      ),
    [streamChannels, forumChannels, unreadChannelIds],
  );

  const markAllRead = React.useCallback(() => {
    for (const channel of [...streamChannels, ...forumChannels]) {
      onMarkChannelRead(channel.id, channel.lastMessageAt);
    }
  }, [streamChannels, forumChannels, onMarkChannelRead]);

  const children = React.useMemo(
    () =>
      buildProjectChildren({
        streamChannels,
        forumChannels,
        repos,
        workflows: workflows ?? [],
        agents,
      }),
    [streamChannels, forumChannels, repos, workflows, agents],
  );

  const childRows = children.map((row) => (
    <ProjectChildRowItem
      key={projectChildKey(row)}
      row={row}
      channelHandlers={channelHandlers}
      onOpenAgents={onOpenAgents}
      onOpenRepo={onOpenRepo}
      onOpenWorkflow={onOpenWorkflow}
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
              {isFallback ? (
                <span className="text-2xs text-sidebar-foreground/45">
                  (local)
                </span>
              ) : null}
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
        <div className="absolute right-1 top-1/2 flex -translate-y-1/2 items-center">
          {onRequestCreate || hasUnread ? (
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
                {hasUnread ? (
                  <>
                    {onRequestCreate ? <DropdownMenuSeparator /> : null}
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
