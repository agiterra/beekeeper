import type { ActiveChannelTurnSummary } from "@/features/agents/activeAgentTurnsStore";
import type { ChannelSection } from "@/features/sidebar/lib/useChannelSections";
import type { Channel } from "@/shared/api/types";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuTrigger,
} from "@/shared/ui/context-menu";
import { SidebarMenuItem } from "@/shared/ui/sidebar";
import { ChannelMenuButton } from "@/features/sidebar/ui/SidebarSection";
import { ChannelContextMenuItems } from "@/features/sidebar/ui/ChannelContextMenu";
import { DraggableChannelRow } from "@/features/sidebar/ui/SidebarDnd";

/**
 * One channel row in the sidebar: the menu button plus its right-click
 * context menu. Shared by the grouped channel sections and the flat
 * per-project child lists so the row behaves identically everywhere.
 */
export function ChannelSidebarRow({
  channel,
  isActive,
  activeWorking,
  hasUnread,
  unreadCount,
  draggable,
  itemClassName,
  onSelectChannel,
  onMarkChannelRead,
  onMarkChannelUnread,
  sections,
  assignments,
  onAssignChannel,
  onUnassignChannel,
  onCreateSectionForChannel,
  isMuted,
  onMuteChannel,
  onUnmuteChannel,
  isStarred,
  onStarChannel,
  onUnstarChannel,
  onDeleteChannel,
  onLeaveChannel,
}: {
  channel: Channel;
  isActive: boolean;
  activeWorking?: ActiveChannelTurnSummary;
  hasUnread: boolean;
  unreadCount: number;
  draggable?: boolean;
  itemClassName?: string;
  onSelectChannel: (channelId: string) => void;
  onMarkChannelRead: (
    channelId: string,
    lastMessageAt: string | null | undefined,
  ) => void;
  onMarkChannelUnread: (channelId: string) => void;
  sections?: ChannelSection[];
  assignments?: Record<string, string>;
  onAssignChannel?: (channelId: string, sectionId: string) => void;
  onUnassignChannel?: (channelId: string) => void;
  onCreateSectionForChannel?: (channelId: string) => void;
  isMuted?: boolean;
  onMuteChannel?: (channelId: string) => void;
  onUnmuteChannel?: (channelId: string) => void;
  isStarred?: boolean;
  onStarChannel?: (channelId: string) => void;
  onUnstarChannel?: (channelId: string) => void;
  onDeleteChannel?: (channel: Channel) => void;
  onLeaveChannel?: (channel: Channel) => void;
}) {
  const button = (
    <ChannelMenuButton
      channel={channel}
      activeWorking={activeWorking}
      hasUnread={hasUnread}
      unreadCount={unreadCount}
      isMuted={isMuted}
      isActive={isActive}
      onSelectChannel={onSelectChannel}
    />
  );
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <SidebarMenuItem className={itemClassName}>
          {draggable ? (
            <DraggableChannelRow channelId={channel.id}>
              {button}
            </DraggableChannelRow>
          ) : (
            button
          )}
        </SidebarMenuItem>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ChannelContextMenuItems
          channel={channel}
          hasUnread={hasUnread}
          isMuted={isMuted}
          isStarred={isStarred}
          sections={sections}
          assignments={assignments}
          onMarkChannelRead={onMarkChannelRead}
          onMarkChannelUnread={onMarkChannelUnread}
          onMuteChannel={onMuteChannel}
          onUnmuteChannel={onUnmuteChannel}
          onStarChannel={onStarChannel}
          onUnstarChannel={onUnstarChannel}
          onAssignChannel={onAssignChannel}
          onUnassignChannel={onUnassignChannel}
          onCreateSectionForChannel={onCreateSectionForChannel}
          onDeleteChannel={onDeleteChannel}
          onLeaveChannel={onLeaveChannel}
        />
      </ContextMenuContent>
    </ContextMenu>
  );
}
