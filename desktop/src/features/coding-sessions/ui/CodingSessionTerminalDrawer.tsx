import * as React from "react";
import {
  Plus,
  SquareSplitHorizontal,
  SquareSplitVertical,
  SquareTerminal,
  Trash2,
} from "lucide-react";

import {
  upsertShellSession,
  useShellSessions,
} from "@/features/builtin-shell/hooks/useShellSessions";
import { ShellTerminal } from "@/features/builtin-shell/ui/ShellTerminal";
import type { ObserverStatus } from "@/features/builtin-shell/observe/useShellObserver";
import { SESSION_SHARED_TERMINALS_READ_LIMIT } from "@/features/builtin-shell/observe/useSessionSharedTerminals";
import {
  type CodingSessionSharedTerminalStatus,
  type CodingSessionTerminalLayout,
  type CodingSessionTerminalSplit,
  activateCodingSessionTerminal,
  addCodingSessionTerminal,
  closeCodingSessionTerminal,
  CODING_SESSION_FOREGROUND_UNKNOWN_LINE,
  codingSessionSharedTerminalLabel,
  codingSessionSharedTerminalsTruncatedLine,
  codingSessionTerminalHeaderLine,
  codingSessionTerminalLabels,
  codingSessionTerminalLayoutStorageKey,
  codingSessionTerminalSplitLimitReached,
  CODING_SESSION_TERMINAL_MAX_PER_GROUP,
  nextCodingSessionTerminalTitle,
  parseStoredCodingSessionTerminalLayout,
  reconcileCodingSessionTerminalLayout,
  splitCodingSessionTerminal,
  visibleCodingSessionTerminals,
} from "@/features/coding-sessions/lib/codingSessionTerminalModel";
import {
  type ShellCodingSessionRef,
  closeShellSession,
  createCodingSessionShell,
  resumeShellSession,
} from "@/shared/api/tauriShell";
import { cn } from "@/shared/lib/cn";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { Button } from "@/shared/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";

import { CodingSessionTerminalCloseDialog } from "./CodingSessionTerminalDrawerParts";
import { useCodingSessionTerminalHeight } from "./CodingSessionTerminalDrawerResize";
import { CodingSessionTerminalManager } from "./CodingSessionTerminalManager";
import { CodingSessionTerminalWatch } from "./CodingSessionTerminalDrawerWatch";
import type { CodingSessionSurfaceCtx } from "./surfaces/codingSessionSurfaceContext";
import type { CodingSessionTerminalExtension } from "./surfaces/CodingSessionSurfaceTerminalExtension";

function readLayout(key: string): CodingSessionTerminalLayout {
  try {
    return parseStoredCodingSessionTerminalLayout(
      window.localStorage.getItem(key),
    );
  } catch {
    return parseStoredCodingSessionTerminalLayout(null);
  }
}

function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "The terminal could not be opened.";
}

/** One drawer action, with T3's tooltip-labelled icon button. */
function TerminalActionButton({
  children,
  disabled = false,
  label,
  onClick,
  testId,
}: {
  children: React.ReactNode;
  disabled?: boolean;
  label: string;
  onClick: () => void;
  testId: string;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          aria-disabled={disabled ? "true" : undefined}
          aria-label={label}
          className={cn(
            "inline-flex items-center px-1 text-foreground/90 transition-colors",
            disabled
              ? "cursor-not-allowed opacity-60 hover:bg-transparent"
              : "hover:bg-accent/70",
          )}
          data-testid={testId}
          onClick={() => {
            if (!disabled) onClick();
          }}
          type="button"
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent side="top">{label}</TooltipContent>
    </Tooltip>
  );
}

/**
 * The session's terminal drawer (SV-25), under the composer: ⌘J.
 *
 * T3's drawer (`ThreadTerminalDrawer.tsx`) with Beekeeper's truths on top:
 *
 * - **Where it opens.** In the session's working tree as this machine
 *   recorded it. The renderer sends the session, never a directory; Rust
 *   resolves the tree and refuses when there is none here (DB9).
 * - **What it is.** The person's own login shell, not the agent's sandbox —
 *   the header says so: "Your shell · not sandboxed · {tree}".
 * - **Whose it is.** A teammate's shared terminal for this session is
 *   listed by owner and liveness, watched read-only; there is no "New
 *   terminal" where this machine has no tree (DB11).
 */
export function CodingSessionTerminalDrawer({
  ctx,
  extension,
}: {
  ctx: CodingSessionSurfaceCtx;
  extension: CodingSessionTerminalExtension;
}) {
  const { height, handleProps } = useCodingSessionTerminalHeight(
    ctx.communityScope,
  );
  const canCreate = ctx.tree.available;
  const layoutKey = codingSessionTerminalLayoutStorageKey({
    relayUrl: ctx.communityScope,
    channelId: ctx.channelId,
    sessionKey: ctx.sessionKey,
  });
  const [storedLayout, setStoredLayout] = React.useState(() =>
    readLayout(layoutKey),
  );
  // Closed here, possibly still in the host list until its next read: never
  // drawn again, so a closed pane cannot come back for a poll's length.
  const [closedIds, setClosedIds] = React.useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const { refresh: refreshShells } = useShellSessions();
  const shellIds = React.useMemo(
    () =>
      extension.shells
        .map((shell) => shell.sessionId)
        .filter((id) => !closedIds.has(id)),
    [closedIds, extension.shells],
  );
  const layout = React.useMemo(
    () => reconcileCodingSessionTerminalLayout(storedLayout, shellIds),
    [shellIds, storedLayout],
  );
  // Not before the host's list answers: an empty first read would
  // overwrite a stored split with nothing.
  const shellsLoading = extension.shellsLoading;
  React.useEffect(() => {
    if (shellsLoading) return;
    try {
      window.localStorage.setItem(layoutKey, JSON.stringify(layout));
    } catch {
      // Storage unavailable: the layout still holds for this view.
    }
  }, [layout, layoutKey, shellsLoading]);

  const [watching, setWatching] = React.useState<string | null>(null);
  const [creating, setCreating] = React.useState(false);
  const [failure, setFailure] = React.useState<string | null>(null);
  const [pendingClose, setPendingClose] = React.useState<string | null>(null);
  const labels = React.useMemo(
    () => codingSessionTerminalLabels(layout.terminalIds),
    [layout.terminalIds],
  );

  const reference = React.useMemo<ShellCodingSessionRef>(
    () => ({
      sessionRef: ctx.sessionKey,
      sessionId: ctx.tree.query.sessionId,
      channelId: ctx.channelId,
      projectRef: ctx.projectRef,
      isLocalProvider: ctx.tree.query.isLocalProvider,
      isHiredSeat: ctx.tree.query.isHiredSeat ?? false,
    }),
    [ctx.channelId, ctx.projectRef, ctx.sessionKey, ctx.tree.query],
  );

  const open = React.useCallback(
    (mode: "new" | CodingSessionTerminalSplit) => {
      if (!canCreate || creating) return;
      if (mode !== "new" && codingSessionTerminalSplitLimitReached(layout)) {
        return;
      }
      setCreating(true);
      setFailure(null);
      createCodingSessionShell({
        codingSession: reference,
        title: nextCodingSessionTerminalTitle(extension.shells),
      })
        .then((info) => {
          upsertShellSession(info);
          setWatching(null);
          setStoredLayout((current) => {
            const base = reconcileCodingSessionTerminalLayout(current, [
              ...shellIds,
            ]);
            return mode === "new"
              ? addCodingSessionTerminal(base, info.sessionId)
              : splitCodingSessionTerminal(base, info.sessionId, mode);
          });
        })
        .catch((error: unknown) => setFailure(errorText(error)))
        .finally(() => setCreating(false));
    },
    [canCreate, creating, extension.shells, layout, reference, shellIds],
  );

  // T3 opens a terminal the first time the drawer opens on a thread with
  // none; here, only where this machine has the session's tree.
  const autoOpened = React.useRef(false);
  React.useEffect(() => {
    if (autoOpened.current || !canCreate || extension.shellsLoading) return;
    autoOpened.current = true;
    if (shellIds.length === 0) open("new");
  }, [canCreate, extension.shellsLoading, open, shellIds.length]);

  const closeNow = React.useCallback(
    (terminalId: string) => {
      setClosedIds((current) => new Set([...current, terminalId]));
      setStoredLayout((current) =>
        closeCodingSessionTerminal(current, terminalId),
      );
      closeShellSession(terminalId)
        .catch((error: unknown) => setFailure(errorText(error)))
        .finally(refreshShells);
    },
    [refreshShells],
  );
  // Close asks first unless the shell is known to be at its prompt (T3
  // confirms every close; at the prompt a close loses nothing but history).
  // A shell whose foreground could not be read is asked about too.
  const requestClose = React.useCallback(
    (terminalId: string) => {
      if (!terminalId) return;
      if (extension.idleIds.has(terminalId)) closeNow(terminalId);
      else setPendingClose(terminalId);
    },
    [closeNow, extension.idleIds],
  );

  // A shell that exited closes its pane (T3's `onSessionExited`); one
  // restored from disk resumes in the session's tree, or says why not.
  const resumed = React.useRef(new Set<string>());
  const exited = React.useRef(new Set<string>());
  React.useEffect(() => {
    for (const shell of extension.shells) {
      if (closedIds.has(shell.sessionId)) continue;
      if (
        !shell.running &&
        !shell.restorable &&
        !exited.current.has(shell.sessionId)
      ) {
        exited.current.add(shell.sessionId);
        closeNow(shell.sessionId);
      }
      if (shell.restorable && !resumed.current.has(shell.sessionId)) {
        resumed.current.add(shell.sessionId);
        resumeShellSession(shell.sessionId)
          .then(upsertShellSession)
          .catch((error: unknown) => setFailure(errorText(error)));
      }
    }
  }, [closeNow, closedIds, extension.shells]);

  const shared = extension.shared;
  const nameOf = (pubkey: string) =>
    ctx.resolveActorName(pubkey)?.trim() || truncatePubkey(pubkey);
  const watchedId =
    watching ?? (canCreate ? null : (shared[0]?.sessionId ?? null));
  const watched = shared.find((t) => t.sessionId === watchedId) ?? null;
  const [watchStatus, setWatchStatus] = React.useState<{
    sessionId: string;
    status: ObserverStatus;
  } | null>(null);
  const statusOf = (sessionId: string): CodingSessionSharedTerminalStatus =>
    watched?.sessionId === sessionId && watchStatus?.sessionId === sessionId
      ? watchStatus.status
      : watched?.sessionId === sessionId
        ? "connecting"
        : "shared";
  const sharedLabel = (terminal: { ownerPubkey: string; sessionId: string }) =>
    codingSessionSharedTerminalLabel({
      ownerName: nameOf(terminal.ownerPubkey),
      isSelf: extension.myPubkey === terminal.ownerPubkey.toLowerCase(),
      status: statusOf(terminal.sessionId),
    });

  const limit = codingSessionTerminalSplitLimitReached(layout);
  const actions = canCreate ? (
    <>
      <TerminalActionButton
        disabled={limit}
        label={
          limit
            ? `Split side by side (max ${CODING_SESSION_TERMINAL_MAX_PER_GROUP} per group)`
            : "Split side by side (⌘D)"
        }
        onClick={() => open("horizontal")}
        testId="coding-session-terminal-split"
      >
        <SquareSplitHorizontal aria-hidden className="size-3.5" />
      </TerminalActionButton>
      <TerminalActionButton
        disabled={limit}
        label={
          limit
            ? `Split stacked (max ${CODING_SESSION_TERMINAL_MAX_PER_GROUP} per group)`
            : "Split stacked (⇧⌘D)"
        }
        onClick={() => open("vertical")}
        testId="coding-session-terminal-split-vertical"
      >
        <SquareSplitVertical aria-hidden className="size-3.5" />
      </TerminalActionButton>
      <TerminalActionButton
        label="New terminal"
        onClick={() => open("new")}
        testId="coding-session-terminal-new"
      >
        <Plus aria-hidden className="size-3.5" />
      </TerminalActionButton>
      <TerminalActionButton
        disabled={layout.activeTerminalId === ""}
        label="Close terminal"
        onClick={() => requestClose(layout.activeTerminalId)}
        testId="coding-session-terminal-close"
      >
        <Trash2 aria-hidden className="size-3.5" />
      </TerminalActionButton>
    </>
  ) : null;

  // What this drawer could not read, said rather than guessed: a failed or
  // stale foreground read, and a shared read cut off at its page size.
  const readNotices: string[] = [];
  if (extension.foregroundUnknown && layout.terminalIds.length > 0) {
    readNotices.push(CODING_SESSION_FOREGROUND_UNKNOWN_LINE);
  }
  if (extension.sharedTruncated) {
    readNotices.push(
      codingSessionSharedTerminalsTruncatedLine(
        SESSION_SHARED_TERMINALS_READ_LIMIT,
      ),
    );
  }

  const showManager = layout.terminalIds.length + shared.length > 1;
  const visible = visibleCodingSessionTerminals(layout);

  const onKeyDownCapture = (event: React.KeyboardEvent<HTMLElement>) => {
    if (!canCreate || event.code !== "KeyD") return;
    if (!(event.metaKey || event.ctrlKey) || event.altKey) return;
    event.preventDefault();
    event.stopPropagation();
    open(event.shiftKey ? "vertical" : "horizontal");
  };

  return (
    <aside
      aria-label="Session terminal"
      className="relative flex min-h-0 min-w-0 flex-col overflow-hidden bg-background"
      data-session-key={ctx.sessionKey}
      data-testid="coding-session-terminal-drawer"
      onKeyDownCapture={onKeyDownCapture}
      style={{ height: `${height}px` }}
    >
      <div
        {...handleProps}
        className="absolute inset-x-0 top-0 z-20 h-1.5 cursor-row-resize outline-none focus-visible:bg-ring/40"
        data-testid="coding-session-terminal-resize"
      />
      <header className="flex h-7 shrink-0 items-center gap-1.5 border-b border-border/60 px-3 text-2xs text-muted-foreground">
        <SquareTerminal aria-hidden className="size-3.5 shrink-0" />
        <span className="truncate" data-testid="coding-session-terminal-header">
          {canCreate
            ? codingSessionTerminalHeaderLine(ctx.tree.label)
            : "Shared terminals · read-only"}
        </span>
        {failure ? (
          <span
            className="ml-auto truncate text-destructive"
            data-testid="coding-session-terminal-error"
            role="alert"
          >
            {failure}
          </span>
        ) : null}
      </header>
      {readNotices.length > 0 ? (
        <div
          className="shrink-0 border-b border-border/60 px-3 py-0.5 text-2xs text-muted-foreground"
          data-testid="coding-session-terminal-read-notice"
          role="status"
        >
          {readNotices.map((line) => (
            <p key={line}>{line}</p>
          ))}
        </div>
      ) : null}
      <div className="flex min-h-0 flex-1">
        <div className="relative flex min-h-0 min-w-0 flex-1 flex-col">
          {watched ? (
            <CodingSessionTerminalWatch
              label={sharedLabel(watched)}
              onStatus={(status) =>
                setWatchStatus({ sessionId: watched.sessionId, status })
              }
              terminal={watched}
            />
          ) : layout.terminalIds.length === 0 ? (
            <div
              className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 px-4 py-6 text-center text-sm text-muted-foreground"
              data-testid="coding-session-terminal-empty"
            >
              <p>
                {creating
                  ? "Opening a terminal in this session's tree…"
                  : "No terminal in this session yet."}
              </p>
              {canCreate ? (
                <Button
                  disabled={creating}
                  onClick={() => open("new")}
                  size="sm"
                  variant="outline"
                >
                  New terminal
                </Button>
              ) : null}
            </div>
          ) : (
            <div
              className="grid min-h-0 flex-1 overflow-hidden"
              data-split-direction={visible.direction}
              style={
                visible.direction === "vertical"
                  ? {
                      gridTemplateRows: `repeat(${visible.ids.length}, minmax(0, 1fr))`,
                    }
                  : {
                      gridTemplateColumns: `repeat(${visible.ids.length}, minmax(0, 1fr))`,
                    }
              }
            >
              {visible.ids.map((terminalId) => (
                <div
                  className={cn(
                    "flex min-h-0 min-w-0 flex-col",
                    visible.direction === "vertical"
                      ? "border-t first:border-t-0"
                      : "border-l first:border-l-0",
                    terminalId === layout.activeTerminalId
                      ? "border-border"
                      : "border-border/70",
                  )}
                  data-active={
                    terminalId === layout.activeTerminalId ? "true" : "false"
                  }
                  data-terminal-label={labels.get(terminalId) ?? "Terminal"}
                  data-testid={`coding-session-terminal-pane-${terminalId}`}
                  key={terminalId}
                  onFocusCapture={() =>
                    setStoredLayout((current) =>
                      activateCodingSessionTerminal(
                        reconcileCodingSessionTerminalLayout(current, shellIds),
                        terminalId,
                      ),
                    )
                  }
                >
                  <ShellTerminal
                    autoFocus={terminalId === layout.activeTerminalId}
                    sessionId={terminalId}
                  />
                </div>
              ))}
            </div>
          )}
          {!showManager && actions ? (
            <div className="pointer-events-none absolute top-2 right-2 z-20">
              <div
                className="pointer-events-auto inline-flex h-6 items-stretch overflow-hidden rounded-md border border-border/80 bg-background shadow-xs [&>button+button]:border-l [&>button+button]:border-border/80"
                data-testid="coding-session-terminal-actions"
              >
                {actions}
              </div>
            </div>
          ) : null}
        </div>
        {showManager ? (
          <CodingSessionTerminalManager
            actions={actions}
            activeTerminalId={watched ? "" : layout.activeTerminalId}
            groups={layout.groups}
            idleIds={extension.idleIds}
            labels={labels}
            onActivate={(terminalId) => {
              setWatching(null);
              setStoredLayout((current) =>
                activateCodingSessionTerminal(
                  reconcileCodingSessionTerminalLayout(current, shellIds),
                  terminalId,
                ),
              );
            }}
            onClose={requestClose}
            onWatch={setWatching}
            runningIds={extension.runningIds}
            shared={shared.map((terminal) => ({
              sessionId: terminal.sessionId,
              label: sharedLabel(terminal),
              title: terminal.title,
              active: terminal.sessionId === watched?.sessionId,
            }))}
          />
        ) : null}
      </div>
      <CodingSessionTerminalCloseDialog
        label={pendingClose ? (labels.get(pendingClose) ?? "Terminal") : null}
        running={pendingClose ? extension.runningIds.has(pendingClose) : false}
        onCancel={() => setPendingClose(null)}
        onConfirm={() => {
          if (pendingClose) closeNow(pendingClose);
          setPendingClose(null);
        }}
      />
    </aside>
  );
}
