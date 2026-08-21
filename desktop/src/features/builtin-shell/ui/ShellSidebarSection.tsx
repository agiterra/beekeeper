import * as React from "react";
import { useNavigate, useParams } from "@tanstack/react-router";
import { ChevronDown, Plus, Settings2 } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import {
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/shared/ui/sidebar";

import { useCreateShellSession } from "../hooks/useCreateShellSession";
import { useShellSessionDialogs } from "../hooks/useShellSessionDialogs";
import { useShellSessions } from "../hooks/useShellSessions";
import { ShellSessionRow } from "./ShellSessionRow";

const COLLAPSED_KEY = "buzz.builtin-shell.sidebar-collapsed.v1";

function readCollapsed(): boolean {
  try {
    return window.localStorage.getItem(COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

/**
 * Built-in shell sessions in the sidebar (the "Built-in Shell" experiment).
 * A sidebar section of shell sessions: rows open the session's terminal in the
 * content area; the header gear opens Settings → Shell. The plus button spawns
 * a new shell.
 *
 * This is the classic top-level Terminals section; with the Projects
 * experiment on, shells render inside each project's flat child list instead.
 */
export function ShellSidebarSection({ onManage }: { onManage: () => void }) {
  const { sessions } = useShellSessions();
  const navigate = useNavigate();
  const activeSessionId = useParams({
    strict: false,
    select: (p) => (p as { sessionId?: string }).sessionId,
  });
  const [collapsed, setCollapsed] = React.useState(readCollapsed);
  const { createFor, creating } = useCreateShellSession();
  const { requestRename, requestClose, dialogs } = useShellSessionDialogs();

  const toggleCollapsed = React.useCallback(() => {
    setCollapsed((prev) => {
      const next = !prev;
      try {
        window.localStorage.setItem(COLLAPSED_KEY, next ? "1" : "0");
      } catch {
        // Ignore unavailable storage; collapse still works this session.
      }
      return next;
    });
  }, []);

  const newShell = React.useCallback(() => createFor(), [createFor]);
  const openSession = React.useCallback(
    (sessionId: string) => {
      void navigate({ to: "/shell/$sessionId", params: { sessionId } });
    },
    [navigate],
  );

  return (
    <SidebarGroup
      className="group/sidebar-section"
      data-testid="builtin-shell-sidebar"
    >
      <div className="relative flex items-center">
        <SidebarGroupLabel asChild>
          <button
            type="button"
            aria-expanded={!collapsed}
            onClick={toggleCollapsed}
            className="group/section-label flex cursor-pointer items-center gap-1 text-left"
            data-testid="builtin-shell-sidebar-toggle"
          >
            <ChevronDown
              className={cn(
                "size-2.5 shrink-0 transition-transform",
                collapsed && "-rotate-90",
              )}
            />
            <span data-sidebar-section-title>Terminals</span>
          </button>
        </SidebarGroupLabel>
        <div className="absolute right-1 top-1/2 flex -translate-y-1/2 items-center">
          <button
            type="button"
            onClick={newShell}
            disabled={creating}
            aria-label="New shell"
            data-testid="builtin-shell-new"
            className="flex size-6 items-center justify-center rounded-md text-sidebar-foreground/45 opacity-0 transition-colors hover:text-sidebar-foreground focus-visible:opacity-100 group-hover/sidebar-section:opacity-100"
          >
            <Plus className="size-4" />
          </button>
          <button
            type="button"
            onClick={onManage}
            aria-label="Manage shell sessions"
            data-testid="builtin-shell-manage"
            className="flex size-6 items-center justify-center rounded-md text-sidebar-foreground/45 opacity-0 transition-colors hover:text-sidebar-foreground focus-visible:opacity-100 group-hover/sidebar-section:opacity-100"
          >
            <Settings2 className="size-4" />
          </button>
        </div>
      </div>
      {collapsed ? null : (
        <SidebarGroupContent>
          <SidebarMenu>
            {sessions.map((session) => (
              <ShellSessionRow
                key={session.sessionId}
                session={session}
                isActive={session.sessionId === activeSessionId}
                onOpen={openSession}
                onRequestRename={requestRename}
                onRequestClose={requestClose}
              />
            ))}
            {sessions.length === 0 ? (
              <SidebarMenuItem>
                <SidebarMenuButton
                  onClick={newShell}
                  data-testid="builtin-shell-empty-new"
                >
                  <Plus className="size-4 shrink-0" />
                  <span className="truncate text-muted-foreground">
                    New shell
                  </span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            ) : null}
          </SidebarMenu>
        </SidebarGroupContent>
      )}
      {dialogs}
    </SidebarGroup>
  );
}
