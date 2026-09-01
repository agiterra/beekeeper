import { Pencil, Terminal, X } from "lucide-react";

import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/shared/ui/context-menu";
import {
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/shared/ui/sidebar";
import type { ShellSessionInfo } from "@/shared/api/tauriShell";
import { ItemPositionBadge } from "@/features/hotkeys/ui/HotkeyBadge";

/**
 * One built-in shell session row: opens the session's terminal on click, with
 * rename/close in the context menu and a hover ✕. Shared by the classic
 * Terminals section and the flat per-project child lists.
 */
export function ShellSessionRow({
  session,
  hotkeyIndex,
  isActive,
  onOpen,
  onRequestRename,
  onRequestClose,
}: {
  session: ShellSessionInfo;
  /** Zero-based position of this row for the item hotkey, or null when unreachable. */
  hotkeyIndex?: number | null;
  isActive: boolean;
  onOpen: (sessionId: string) => void;
  onRequestRename: (session: ShellSessionInfo) => void;
  onRequestClose: (session: ShellSessionInfo) => void;
}) {
  return (
    <SidebarMenuItem>
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <SidebarMenuButton
            isActive={isActive}
            onClick={() => onOpen(session.sessionId)}
            data-testid="builtin-shell-sidebar-row"
          >
            <Terminal className="size-4 shrink-0" />
            <span className="truncate">{session.title}</span>
            {!session.running ? (
              <span
                title="Shell exited"
                className="ml-auto inline-flex size-1.5 shrink-0 rounded-full bg-muted-foreground/50"
                data-hotkey-dim
              />
            ) : null}
            <ItemPositionBadge index={hotkeyIndex ?? null} />
          </SidebarMenuButton>
        </ContextMenuTrigger>
        <ContextMenuContent>
          <ContextMenuItem onSelect={() => onRequestRename(session)}>
            <Pencil />
            Rename…
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem
            className="text-destructive focus:text-destructive"
            onSelect={() => onRequestClose(session)}
          >
            <X />
            Close session
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
      <SidebarMenuAction
        showOnHover
        aria-label="Close shell"
        data-testid="builtin-shell-sidebar-close"
        onClick={(event) => {
          event.stopPropagation();
          onRequestClose(session);
        }}
      >
        <X />
      </SidebarMenuAction>
    </SidebarMenuItem>
  );
}
