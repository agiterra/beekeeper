import * as React from "react";
import { useQuery } from "@tanstack/react-query";

import { useSessionSharedTerminals } from "@/features/builtin-shell/observe/useSessionSharedTerminals";
import type { RemoteTerminal } from "@/features/builtin-shell/observe/useProjectTerminals";
import { useShellSessions } from "@/features/builtin-shell/hooks/useShellSessions";
import {
  codingSessionForegroundFold,
  codingSessionForegroundUnknown,
  codingSessionSharedTerminalsFor,
  codingSessionShellsFor,
} from "@/features/coding-sessions/lib/codingSessionTerminalModel";
import {
  type ShellSessionInfo,
  shellSessionsForeground,
} from "@/shared/api/tauriShell";

import type {
  CodingSessionSurfaceBaseCtx,
  CodingSessionSurfaceCtx,
} from "./codingSessionSurfaceContext";

/** How often the badge asks whether a command is running (SV-22). */
export const CODING_SESSION_TERMINAL_FOREGROUND_POLL_MS = 1_500;

/**
 * What the Terminal surface reads once per view and shares between its badge
 * and its drawer (`ctx.extensions.terminal`).
 */
export type CodingSessionTerminalExtension = {
  /** This session's shells on this computer, oldest first. */
  shells: ShellSessionInfo[];
  /** The host's shell list has not answered yet. */
  shellsLoading: boolean;
  /** Shells a command holds right now (the foreground read). */
  runningIds: ReadonlySet<string>;
  /** Shells read as at the prompt. A shell in neither set is unknown. */
  idleIds: ReadonlySet<string>;
  /** `runningIds` restricted to this session's live shells: the badge. */
  runningCount: number;
  /** This computer's foreground read failed or went stale: every live shell
   * is unknown, and the drawer says it could not tell. */
  foregroundUnknown: boolean;
  /** Teammates' (and the viewer's other computers') shared terminals for
   * this session; never one this computer runs. */
  shared: RemoteTerminal[];
  sharedState: "no-project" | "loading" | "ready" | "error";
  /** The shared read came back full (the project's newest announces only):
   * an empty `shared` is then not "none shared". */
  sharedTruncated: boolean;
  /** The viewer's pubkey, lowercase, to tell "your other computer". */
  myPubkey: string | null;
};

/**
 * The Terminal surface's `readExtension` hook: this session's shells on this
 * computer, which of them a command holds now, and the session's shared
 * terminals elsewhere. Every read is React Query or the built-in shell's
 * shared list (machine-scoped, not community data), so there is no module
 * cache here for `resetCommunityState()` to clear.
 */
export function useCodingSessionTerminalExtension(
  ctx: CodingSessionSurfaceBaseCtx,
): CodingSessionTerminalExtension {
  const { sessions, loading } = useShellSessions();
  const shells = React.useMemo(
    () =>
      codingSessionShellsFor(sessions, {
        sessionKey: ctx.sessionKey,
        channelId: ctx.channelId,
      }),
    [ctx.channelId, ctx.sessionKey, sessions],
  );
  const liveIds = React.useMemo(
    () =>
      shells
        .filter((shell) => shell.running && !shell.restorable)
        .map((shell) => shell.sessionId),
    [shells],
  );
  const foreground = useQuery({
    queryKey: ["coding-session-terminal-foreground", liveIds],
    queryFn: () => shellSessionsForeground(liveIds),
    enabled: liveIds.length > 0,
    refetchInterval: CODING_SESSION_TERMINAL_FOREGROUND_POLL_MS,
    retry: false,
  });
  // A failed read, or one older than two poll periods, says nothing about
  // now: every live shell is then unknown rather than the last answer
  // carried forward (a badge that kept "1 command running" after the read
  // stopped answering would be a guess).
  const foregroundUnknown =
    liveIds.length > 0 &&
    codingSessionForegroundUnknown({
      isError: foreground.isError,
      dataUpdatedAt: foreground.dataUpdatedAt,
      now: Date.now(),
      pollMs: CODING_SESSION_TERMINAL_FOREGROUND_POLL_MS,
    });
  const { runningIds, idleIds, runningCount } = React.useMemo(
    () =>
      codingSessionForegroundFold({
        reads: foreground.data,
        liveIds,
        unknown: foregroundUnknown,
      }),
    [foreground.data, foregroundUnknown, liveIds],
  );

  const sharedRead = useSessionSharedTerminals(
    ctx.communityScope,
    ctx.projectRef,
    ctx.sessionKey,
  );
  const shared = React.useMemo(
    () =>
      codingSessionSharedTerminalsFor(
        sharedRead.terminals,
        ctx.sessionKey,
        new Set(sessions.map((session) => session.sessionId)),
      ),
    [ctx.sessionKey, sessions, sharedRead.terminals],
  );

  return React.useMemo(
    () => ({
      shells,
      shellsLoading: loading,
      runningIds,
      idleIds,
      runningCount,
      foregroundUnknown,
      shared,
      sharedState: sharedRead.state,
      sharedTruncated: sharedRead.truncated,
      myPubkey: sharedRead.myPubkey,
    }),
    [
      foregroundUnknown,
      idleIds,
      loading,
      runningCount,
      runningIds,
      shared,
      sharedRead.myPubkey,
      sharedRead.state,
      sharedRead.truncated,
      shells,
    ],
  );
}

/** The extension from `ctx`, or `null` where the surface's hook did not run. */
export function codingSessionTerminalExtension(
  ctx: Pick<CodingSessionSurfaceCtx, "extensions">,
): CodingSessionTerminalExtension | null {
  const value = ctx.extensions.terminal;
  return value && typeof value === "object"
    ? (value as CodingSessionTerminalExtension)
    : null;
}
