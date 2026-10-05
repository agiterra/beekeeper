import type * as React from "react";
import {
  Eye,
  Square,
  SquareSplitHorizontal,
  SquareSplitVertical,
  SquareTerminal,
  X,
} from "lucide-react";

import {
  type CodingSessionTerminalGroup,
  codingSessionTerminalGroupLabel,
} from "@/features/coding-sessions/lib/codingSessionTerminalModel";
import { cn } from "@/shared/lib/cn";

/** A teammate's shared terminal as the manager lists it. */
export type CodingSessionTerminalManagerSharedRow = {
  sessionId: string;
  /** "{owner}'s computer · live" — never a host name (DB11). */
  label: string;
  title: string;
  active: boolean;
};

/**
 * The terminal manager list (SV-25), T3's sidebar
 * (`ThreadTerminalDrawer.tsx:1588-1640`): the drawer's actions in its top
 * row, then each group — with a "Single", "Side by side" or "Stacked" header
 * once there is more than one group or any split — and its terminals, each
 * with a close button on its icon. It appears once the drawer holds two or
 * more terminals.
 *
 * Beyond T3: the session's terminals shared from other computers are listed
 * under their own heading, by owner and liveness, and open read-only.
 */
export function CodingSessionTerminalManager({
  actions,
  activeTerminalId,
  groups,
  idleIds,
  labels,
  onActivate,
  onClose,
  onWatch,
  runningIds,
  shared,
}: {
  /** The split / new / close buttons, rendered in the top row. */
  actions: React.ReactNode;
  activeTerminalId: string;
  groups: readonly CodingSessionTerminalGroup[];
  /** Shells read as at their prompt; in neither set means unknown. */
  idleIds: ReadonlySet<string>;
  labels: ReadonlyMap<string, string>;
  onActivate: (terminalId: string) => void;
  onClose: (terminalId: string) => void;
  onWatch: (sessionId: string) => void;
  runningIds: ReadonlySet<string>;
  shared: readonly CodingSessionTerminalManagerSharedRow[];
}) {
  const showGroupHeaders =
    groups.length > 1 || groups.some((group) => group.terminalIds.length > 1);
  return (
    <aside
      aria-label="Terminals"
      className="flex w-36 min-w-36 flex-col border-l border-border/70 bg-muted/10"
      data-testid="coding-session-terminal-manager"
    >
      <div className="flex h-[22px] items-stretch justify-end border-b border-border/70">
        <div className="inline-flex h-full items-stretch">{actions}</div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-1 py-1">
        {groups.map((group) => {
          const groupActive = group.terminalIds.includes(activeTerminalId);
          const GroupIcon =
            group.terminalIds.length < 2
              ? Square
              : group.splitDirection === "vertical"
                ? SquareSplitVertical
                : SquareSplitHorizontal;
          return (
            <div
              className="pb-0.5"
              data-testid={`coding-session-terminal-group-${group.id}`}
              key={group.id}
            >
              {showGroupHeaders ? (
                <button
                  className={cn(
                    "flex h-[22px] w-full items-center gap-1 rounded px-1.5 text-2xs",
                    groupActive
                      ? "bg-accent/50 text-foreground"
                      : "text-muted-foreground hover:bg-accent/40 hover:text-foreground",
                  )}
                  onClick={() => {
                    const target = groupActive
                      ? activeTerminalId
                      : group.terminalIds[0];
                    if (target) onActivate(target);
                  }}
                  type="button"
                >
                  <GroupIcon aria-hidden className="size-3 shrink-0" />
                  <span className="min-w-0 flex-1 truncate text-left">
                    {codingSessionTerminalGroupLabel(group)}
                  </span>
                  <span className="text-3xs tabular-nums text-muted-foreground/70">
                    {group.terminalIds.length}
                  </span>
                </button>
              ) : null}
              <div className="flex flex-col gap-0.5">
                {group.terminalIds.map((terminalId) => {
                  const active = terminalId === activeTerminalId;
                  const label = labels.get(terminalId) ?? "Terminal";
                  const running = runningIds.has(terminalId);
                  return (
                    <div
                      className={cn(
                        "group/tab flex h-6 w-full items-center gap-0.5 rounded-md pr-2 pl-1.5 text-xs",
                        active
                          ? "bg-accent text-foreground"
                          : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
                      )}
                      data-active={active ? "true" : "false"}
                      data-foreground={
                        running
                          ? "running"
                          : idleIds.has(terminalId)
                            ? "idle"
                            : "unknown"
                      }
                      data-testid={`coding-session-terminal-row-${terminalId}`}
                      key={terminalId}
                    >
                      <button
                        aria-label={`Close ${label}`}
                        className="relative inline-flex size-4 shrink-0 items-center justify-center rounded"
                        data-testid={`coding-session-terminal-row-close-${terminalId}`}
                        onClick={() => onClose(terminalId)}
                        title={`Close ${label}`}
                        type="button"
                      >
                        <SquareTerminal
                          aria-hidden
                          className="size-3 group-hover/tab:hidden"
                        />
                        <X
                          aria-hidden
                          className="hidden size-3 group-hover/tab:block"
                        />
                      </button>
                      <button
                        className="flex min-w-0 flex-1 items-center gap-1 text-left"
                        onClick={() => onActivate(terminalId)}
                        type="button"
                      >
                        <span className="truncate">{label}</span>
                        {running ? (
                          <>
                            <span
                              aria-hidden
                              className="ml-auto size-1.5 shrink-0 rounded-full bg-primary"
                              title="A command is running"
                            />
                            <span className="sr-only">
                              , a command is running
                            </span>
                          </>
                        ) : null}
                      </button>
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })}
        {shared.length > 0 ? (
          <div className="pt-1" data-testid="coding-session-terminal-shared">
            <p className="flex h-[22px] items-center gap-1 px-1.5 text-2xs text-muted-foreground">
              <Eye aria-hidden className="size-3 shrink-0" />
              Shared, read-only
            </p>
            {shared.map((row) => (
              <button
                className={cn(
                  "flex h-auto min-h-6 w-full flex-col items-start rounded-md px-1.5 py-0.5 text-left text-xs",
                  row.active
                    ? "bg-accent text-foreground"
                    : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
                )}
                data-active={row.active ? "true" : "false"}
                data-testid={`coding-session-terminal-shared-row-${row.sessionId}`}
                key={row.sessionId}
                onClick={() => onWatch(row.sessionId)}
                title={row.title}
                type="button"
              >
                <span className="w-full truncate">{row.label}</span>
              </button>
            ))}
          </div>
        ) : null}
      </div>
    </aside>
  );
}
