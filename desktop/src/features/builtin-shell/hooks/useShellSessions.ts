import * as React from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import {
  SHELL_SESSION_EXIT_EVENT,
  listShellSessions,
  type ShellSessionInfo,
} from "@/shared/api/tauriShell";

const POLL_MS = 5000;

// Module-level external store (useSyncExternalStore over module state),
// shared by every consumer
// (sidebar, session screen, settings card). A plain per-component
// useState/useEffect hook would give each caller its own list and its own
// poll timer — so a session created in the sidebar wouldn't appear on its own
// screen until that screen's independent timer happened to tick, showing a
// "this session has ended" flash for up to `POLL_MS`. A single shared list
// means `upsertShellSession` (called right after creating/resuming a session)
// is visible to every consumer immediately, before any poll runs.
let sessions: ShellSessionInfo[] = [];
let loading = true;
let snapshot: { sessions: ShellSessionInfo[]; loading: boolean } = {
  sessions,
  loading,
};
const listeners = new Set<() => void>();

function publish(): void {
  snapshot = { sessions, loading };
  for (const listener of listeners) listener();
}

function setSessions(next: ShellSessionInfo[]): void {
  sessions = next;
  loading = false;
  publish();
}

function refresh(): void {
  listShellSessions()
    .then(setSessions)
    .catch(() => {
      // Backend unavailable (e.g. browser preview); keep the last list.
      loading = false;
      publish();
    });
}

/** Merge a just-created/resumed session into the shared list immediately, so
 * navigating straight to its screen doesn't race the next poll tick. The
 * backend already registered the session synchronously before returning this
 * info, so this is authoritative, not a guess. */
export function upsertShellSession(info: ShellSessionInfo): void {
  setSessions([
    ...sessions.filter((s) => s.sessionId !== info.sessionId),
    info,
  ]);
}

let pollHandle: number | null = null;
let unlistenExit: UnlistenFn | null = null;
let subscriberCount = 0;
let initialized = false;

function ensureStarted(): void {
  subscriberCount += 1;
  if (subscriberCount > 1) return;
  if (!initialized) {
    initialized = true;
    refresh();
  }
  pollHandle = window.setInterval(refresh, POLL_MS);
  listen(SHELL_SESSION_EXIT_EVENT, refresh)
    .then((fn) => {
      unlistenExit = fn;
    })
    .catch(() => {
      // Event bridge unavailable outside Tauri; polling still covers it.
    });
}

function ensureStopped(): void {
  subscriberCount -= 1;
  if (subscriberCount > 0) return;
  if (pollHandle !== null) {
    window.clearInterval(pollHandle);
    pollHandle = null;
  }
  unlistenExit?.();
  unlistenExit = null;
}

/**
 * The live list of built-in shell sessions, shared across every consumer.
 * Polls the backend (sessions can be created from anywhere) and refreshes
 * immediately when a shell exits so the sidebar/settings reflect it without
 * waiting a poll tick.
 */
export function useShellSessions(): {
  sessions: ShellSessionInfo[];
  loading: boolean;
  refresh: () => void;
} {
  const state = React.useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => snapshot,
    () => snapshot,
  );

  React.useEffect(() => {
    ensureStarted();
    return ensureStopped;
  }, []);

  return { sessions: state.sessions, loading: state.loading, refresh };
}

/**
 * The owning project of one shell session, or null.
 *
 * A terminal route carries no channel, so this back-reference is the only
 * thing that can say which project `/shell/<id>` belongs to.
 *
 * Subscribes to the shared list *only* while a session id is in play. The
 * store polls the backend every {@link POLL_MS}, and the caller is the app
 * shell — mounted for the whole session, on every route. Subscribing
 * unconditionally there would run that poll forever for people who never open
 * a terminal, to answer a question that only a `/shell/` route ever asks.
 */
export function useShellSessionProjectRef(
  sessionId: string | undefined,
): string | null {
  React.useEffect(() => {
    if (!sessionId) return;
    ensureStarted();
    return ensureStopped;
  }, [sessionId]);

  const state = React.useSyncExternalStore(
    (listener) => {
      if (!sessionId) return () => {};
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => snapshot,
    () => snapshot,
  );

  if (!sessionId) return null;
  return (
    state.sessions.find((session) => session.sessionId === sessionId)
      ?.projectRef ?? null
  );
}
