import {
  Archive,
  Bot,
  Eye,
  LoaderCircle,
  RotateCcw,
  Square,
  Terminal,
  Trash2,
} from "lucide-react";

import type { RemoteTerminal } from "@/features/builtin-shell/observe/useProjectTerminals";
import { ShellSessionRow } from "@/features/builtin-shell/ui/ShellSessionRow";
import type { UserProfileLookup } from "@/features/profile/lib/identity";
import { resolveUserLabel } from "@/features/profile/lib/identity";
import { ProfileAvatar } from "@/features/profile/ui/ProfileAvatar";
import { ChannelSidebarRow } from "@/features/sidebar/ui/ChannelSidebarRow";
import type { ShellSessionInfo } from "@/shared/api/tauriShell";
import { cn } from "@/shared/lib/cn";
import { ItemPositionBadge } from "@/features/hotkeys/ui/HotkeyBadge";
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
import { codingSessionRowKey } from "../lib/activeCodingSession";
import { projectSessionIndicator } from "../lib/projectSessionIndicator";
import type { ProjectChannelHandlers } from "./ProjectSidebarGroup";

/**
 * Renders one row of a project's flat child list; the row's type picks the
 * icon and behavior. Channel/forum and shell rows delegate to the shared row
 * components so they behave identically to their non-project counterparts.
 */
export function ProjectChildRowItem({
  row,
  hotkeyIndex,
  channelHandlers,
  onOpenCodingSession,
  onRequestCloseCodingSession,
  onRequestArchiveCodingSession,
  onRequestReopenCodingSession,
  onRequestDeleteCodingSession,
  canDeleteCodingSession,
  activeCodingSessionKey,
  currentPubkey,
  founderProfiles,
  activeShellSessionId,
  onOpenShell,
  onRequestRenameShell,
  onRequestCloseShell,
  onObserveShell,
}: {
  row: ProjectChildRow;
  /** Zero-based position of this row for the item hotkey, or null when unreachable. */
  hotkeyIndex?: number | null;
  channelHandlers: ProjectChannelHandlers;
  onOpenCodingSession?: (
    coordinates: ExactProjectCodingSessionCoordinates,
  ) => void;
  onRequestCloseCodingSession?: (entry: ProjectCodingSessionShelfEntry) => void;
  /** Archive = close + file away; one 44230 revision with `archived`. */
  onRequestArchiveCodingSession?: (
    entry: ProjectCodingSessionShelfEntry,
  ) => void;
  onRequestReopenCodingSession?: (
    entry: ProjectCodingSessionShelfEntry,
  ) => void;
  onRequestDeleteCodingSession?: (
    entry: ProjectCodingSessionShelfEntry,
  ) => void;
  /**
   * Whether the viewer may delete a session founded by this pubkey. Unlike
   * close/archive/reopen — which are founder-only — delete is the project's
   * rule: an Owner reaches any session in the project. Passed in because the
   * decision belongs to the screen that holds the project's capabilities,
   * not to a row.
   */
  canDeleteCodingSession?: (founderPubkey: string | null) => boolean;
  /**
   * `channelId:generationId` of the coding session currently on screen, or
   * null. The sidebar's other two row types have carried an active state
   * since they existed (`isActiveChannel`, `activeShellSessionId`); coding
   * sessions never did, so opening one left the sidebar showing nothing
   * selected.
   */
  activeCodingSessionKey?: string | null;
  currentPubkey?: string;
  /** Batched profiles for the founders of the rows being rendered — resolved
   * once by the group so a row never fires its own profile query. */
  founderProfiles?: UserProfileLookup;
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
      const isFounder = Boolean(
        currentPubkey &&
          entry.founderPubkey?.toLowerCase() === currentPubkey.toLowerCase(),
      );
      const canClose = Boolean(
        !settled &&
          hasClosureCoordinates &&
          isFounder &&
          onRequestCloseCodingSession,
      );
      // Archive is a close that also files the session away, so it is
      // offered wherever the session is not yet archived — open or closed.
      const canArchive = Boolean(
        !entry.isArchived &&
          hasClosureCoordinates &&
          isFounder &&
          onRequestArchiveCodingSession,
      );
      // Delete is the one action here that is not founder-only: a project
      // Owner reaches any session in their project, which is the rule the
      // relay applies too. `canDeleteCodingSession` carries that decision in
      // from the screen holding the project's capabilities.
      const canDelete = Boolean(
        entry.sessionRef &&
          onRequestDeleteCodingSession &&
          canDeleteCodingSession?.(entry.founderPubkey),
      );
      const canReopen = Boolean(
        settled &&
          hasClosureCoordinates &&
          currentPubkey &&
          onRequestReopenCodingSession,
      );
      const indicator = projectSessionIndicator(entry);
      // A pending row stands for a create the provider has not acknowledged
      // yet — there is no generation to open, so the row is presence-only.
      const pending = entry.pending === true;
      // The row's icon is the person who started the session. A session with
      // no resolved genesis has no founder; it keeps the generic glyph and
      // says so rather than wearing someone else's face.
      const founderPubkey = entry.founderPubkey?.toLowerCase() ?? null;
      const founderName = founderPubkey
        ? resolveUserLabel({
            currentPubkey,
            profiles: founderProfiles,
            pubkey: founderPubkey,
          })
        : null;
      // A pending row stands for a create the provider has not acknowledged,
      // so it has no generation to be open at and can never be the active
      // one — matching the click handler, which is also withheld.
      const isActiveSession =
        !pending &&
        activeCodingSessionKey ===
          codingSessionRowKey(entry.channelId, entry.generationId);
      const button = (
        <SidebarMenuButton
          aria-label={
            pending
              ? `${entry.label}, starting`
              : `Open ${entry.label}${details ? `, ${details}` : ""}`
          }
          isActive={isActiveSession}
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
              settled ? "size-4 opacity-55" : "size-6",
              !(founderPubkey && !pending) &&
                !settled &&
                "rounded-full bg-sidebar-accent",
            )}
            data-testid={
              pending
                ? undefined
                : founderPubkey
                  ? "project-coding-session-founder"
                  : "project-coding-session-founder-unknown"
            }
            title={
              pending
                ? undefined
                : founderName
                  ? `Started by ${founderName}`
                  : "Initiator unknown"
            }
          >
            {pending ? (
              <LoaderCircle className="size-3.5 animate-spin" />
            ) : founderPubkey && founderName ? (
              <ProfileAvatar
                avatarUrl={founderProfiles?.[founderPubkey]?.avatarUrl ?? null}
                className={cn(
                  "shadow-none",
                  settled ? "h-4 w-4 text-3xs" : "h-6 w-6 text-2xs",
                )}
                iconClassName={settled ? "h-2.5 w-2.5" : "h-3 w-3"}
                label={founderName}
              />
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
          {/* The state dot: closure facts outrank provider metadata, and the
              hover names the state plus what the colour does not prove. */}
          <span
            aria-label={indicator.label}
            className={cn(
              "ml-1 size-2.5 shrink-0 rounded-full",
              indicator.colorClass,
              pending && "animate-pulse",
            )}
            data-hotkey-dim
            data-session-indicator={indicator.state}
            data-testid="project-coding-session-indicator"
            role="img"
            title={indicator.title}
          />
          <ItemPositionBadge index={hotkeyIndex ?? null} />
        </SidebarMenuButton>
      );
      return (
        <SidebarMenuItem
          data-session-closure={settled ? "closed" : "open"}
          data-session-status={entry.status.kind}
        >
          {canClose || canArchive || canReopen || canDelete ? (
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
                {canArchive ? (
                  <ContextMenuItem
                    data-testid="project-coding-session-archive"
                    onSelect={() => onRequestArchiveCodingSession?.(entry)}
                  >
                    <Archive />
                    Archive session
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
                {canDelete ? (
                  <ContextMenuItem
                    className="text-destructive focus:text-destructive"
                    data-testid="project-coding-session-delete"
                    onSelect={() => onRequestDeleteCodingSession?.(entry)}
                  >
                    <Trash2 />
                    Delete session
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
    case "channel":
    case "forum": {
      const { channel } = row;
      return (
        <ChannelSidebarRow
          channel={channel}
          hotkeyIndex={hotkeyIndex}
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
    case "shell": {
      if (!onOpenShell || !onRequestRenameShell || !onRequestCloseShell) {
        return null;
      }
      return (
        <ShellSessionRow
          session={row.session}
          hotkeyIndex={hotkeyIndex}
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
            <Eye
              className="ml-auto size-3.5 shrink-0 text-sidebar-foreground/50"
              data-hotkey-dim
            />
            <ItemPositionBadge index={hotkeyIndex ?? null} />
          </SidebarMenuButton>
        </SidebarMenuItem>
      );
    }
  }
}
