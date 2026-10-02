import * as React from "react";
import {
  Archive,
  Bot,
  Eye,
  FileText,
  Folder,
  FolderPlus,
  ListChecks,
  LoaderCircle,
  Lock,
  RotateCcw,
  Square,
  Terminal,
  Trash2,
} from "lucide-react";

import type { PinRow } from "@/features/agents-repo/lib/artifactPinFold";
import type { RemoteTerminal } from "@/features/builtin-shell/observe/useProjectTerminals";
import { useNewSessionInWorkspaceAction } from "@/features/coding-sessions/hooks/useNewSessionInWorkspaceAction";
import { NEW_SESSION_IN_WORKSPACE_LABEL } from "@/features/coding-sessions/lib/codingSessionWorkspaceReuseCopy";
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
import { type ProjectChildRow, artifactPinLabel } from "../lib/projectChildren";
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
  activeTodoListId,
  onOpenTodoList,
  activeArtifactTarget,
  onOpenArtifact,
}: {
  row: ProjectChildRow;
  /** Id of the to-do list on screen, or null. */
  activeTodoListId?: string | null;
  onOpenTodoList?: (listId: string) => void;
  /** Path or folder prefix of the artifact on screen, or null. */
  activeArtifactTarget?: string | null;
  onOpenArtifact?: (pin: PinRow) => void;
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
      // A founded row is a genesis nothing has started: no runtime to name,
      // so the second line says the one true thing about it instead.
      const founded = entry.founded === true;
      const details = [
        entry.sourceChannelLabel,
        founded ? "Not started" : entry.runtimeLabel,
      ]
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
      // A founded session that never ran has nothing worth a closed record;
      // its one way out is Delete (2026-09-11).
      const canClose = Boolean(
        !settled &&
          !founded &&
          hasClosureCoordinates &&
          isFounder &&
          onRequestCloseCodingSession,
      );
      // Archive is a close that also files the session away, so it is
      // offered wherever the session is not yet archived — open or closed.
      const canArchive = Boolean(
        !entry.isArchived &&
          !founded &&
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
              : founded
                ? `Open ${entry.label}, not started`
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
          // A founded row opens too: its generationId is the founded row id,
          // and the handler above tells the two routes apart
          // (`resolveProjectCodingSessionOpenTarget`) — the same path the
          // group's hotkey activation takes.
          onClick={
            pending
              ? undefined
              : () =>
                  onOpenCodingSession({
                    channelId: entry.channelId,
                    generationId: entry.generationId,
                  })
          }
          onKeyDown={openContextMenuFromKeyboard}
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
                  ? founded
                    ? `Founded by ${founderName}`
                    : `Started by ${founderName}`
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
          {/* The menu is mounted for every session row now, not only for rows
              with a close/archive/reopen/delete to offer. "New session in this
              workspace" is always available — availability decides what it
              opens, never whether it exists (contract §3) — so gating the menu
              on the other four would have hidden it on exactly the rows that
              have no other action. */}
          <ContextMenu>
            <ContextMenuTrigger asChild>{button}</ContextMenuTrigger>
            <ContextMenuContent>
              <NewSessionInWorkspaceMenuItem entry={entry} />
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
    case "todo-list": {
      if (!onOpenTodoList) return null;
      const { list } = row;
      const personal = list.visibility === "personal";
      return (
        <SidebarMenuItem>
          <SidebarMenuButton
            data-testid={`project-todo-list-row-${list.id.slice(0, 8)}`}
            isActive={list.id === activeTodoListId}
            onClick={() => onOpenTodoList(list.id)}
            title={
              personal
                ? "Your personal to-do list — only you can see it"
                : "Shared to-do list"
            }
          >
            <ListChecks className="size-4 shrink-0" />
            <span className="truncate">{list.title}</span>
            {personal ? (
              <Lock
                aria-label="Personal"
                className="ml-auto size-3 shrink-0 text-sidebar-foreground/50"
                data-hotkey-dim
                data-testid="project-todo-list-personal"
              />
            ) : null}
            <ItemPositionBadge index={hotkeyIndex ?? null} />
          </SidebarMenuButton>
        </SidebarMenuItem>
      );
    }
    case "artifact": {
      if (!onOpenArtifact) return null;
      const { pin } = row;
      const folder = pin.targetKind === "folder";
      return (
        <SidebarMenuItem>
          <SidebarMenuButton
            data-testid={`project-artifact-row-${pin.target}`}
            isActive={pin.target === activeArtifactTarget}
            onClick={() => onOpenArtifact(pin)}
            title={
              folder
                ? `${pin.target} — a pinned folder of the project's documents`
                : `${pin.target} — pinned by this project`
            }
          >
            {folder ? (
              <Folder className="size-4 shrink-0" />
            ) : (
              <FileText className="size-4 shrink-0" />
            )}
            <span className="truncate">{artifactPinLabel(pin)}</span>
            <ItemPositionBadge index={hotkeyIndex ?? null} />
          </SidebarMenuButton>
        </SidebarMenuItem>
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

/**
 * "New session in this workspace" on a sidebar session row.
 *
 * It mounts exactly when the menu opens — Radix renders context-menu content
 * only while open — which is where the workspace read belongs: a sidebar
 * draws these rows hundreds of times, and a disk stat per draw would be a
 * filesystem walk per scroll tick. So the read starts here, on mount, and the
 * second line fills in when it answers.
 *
 * Selecting it opens a draft and nothing else: no signature, no start, resume
 * or stop, no branch or grant change, no directory created
 * (`useNewSessionInWorkspaceAction`).
 */
function NewSessionInWorkspaceMenuItem({
  entry,
}: {
  entry: ProjectCodingSessionShelfEntry;
}) {
  const action = useNewSessionInWorkspaceAction({
    sourceRepoRef: entry.session.repoRef ?? null,
    channelId: entry.channelId,
    executionProviderPubkey: entry.session.providerAuthorityPubkey,
    projectId: entry.projectId,
    sessionRef: entry.sessionRef,
  });
  const { resolveNow } = action;
  React.useEffect(() => {
    resolveNow();
  }, [resolveNow]);
  return (
    <ContextMenuItem
      data-testid="project-coding-session-new-session-in-workspace"
      onSelect={action.start}
    >
      <FolderPlus />
      <span className="flex min-w-0 flex-col">
        <span>{NEW_SESSION_IN_WORKSPACE_LABEL}</span>
        <span
          className="text-2xs text-muted-foreground"
          data-testid="project-coding-session-new-session-in-workspace-detail"
        >
          {action.detail}
        </span>
      </span>
    </ContextMenuItem>
  );
}

/**
 * The keyboard route to the row's own context menu.
 *
 * Radix's `ContextMenuTrigger` listens for a pointer's `contextmenu` and
 * nothing else, so until now every action on this row was unreachable without
 * a right-click — including this feature's only sidebar entry point. Rather
 * than mount a second, differently-populated `⋯` menu beside it (two menus
 * drift; that is how a control starts lying about what it offers), the
 * platform keys dispatch the very event the pointer would: same trigger, same
 * menu, same items, anchored to the row.
 *
 * Exported for its own test: the dispatch is the whole behaviour, and it is
 * the half a Playwright run cannot isolate from Radix's own handling.
 */
export function openContextMenuFromKeyboard(
  event: React.KeyboardEvent<HTMLElement>,
) {
  const wantsMenu =
    event.key === "ContextMenu" || (event.shiftKey && event.key === "F10");
  if (!wantsMenu || event.defaultPrevented) return;
  event.preventDefault();
  const row = event.currentTarget;
  const rect = row.getBoundingClientRect();
  row.dispatchEvent(
    new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: Math.round(rect.left + 12),
      clientY: Math.round(rect.bottom - 8),
      view: row.ownerDocument.defaultView,
    }),
  );
}
