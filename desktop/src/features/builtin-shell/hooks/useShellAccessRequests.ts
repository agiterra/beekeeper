import * as React from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import {
  SHELL_ACCESS_REQUEST_EVENT,
  SHELL_ACCESS_REQUEST_RESOLVED_EVENT,
  listShellAccessRequests,
  resolveShellAccessRequest,
  type ShellAccessDecision,
  type ShellAccessRequest,
} from "@/shared/api/tauriShell";

/**
 * The queue of pending agent access requests for built-in shell sessions.
 * Agents call `buzz session request-access` when a session's "Agents" consent
 * is off; the backend blocks that call and emits `shell-access-request`, which
 * this hook turns into an in-app approval prompt. Resolving wakes the agent.
 */
export function useShellAccessRequests(): {
  requests: ShellAccessRequest[];
  resolve: (id: string, decision: ShellAccessDecision) => void;
} {
  const [requests, setRequests] = React.useState<ShellAccessRequest[]>([]);

  React.useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    // Catch requests that arrived before this mounted.
    listShellAccessRequests()
      .then((initial) => {
        if (!cancelled) setRequests(initial);
      })
      .catch(() => {
        // Backend unavailable (browser preview); nothing to show.
      });

    listen<ShellAccessRequest>(SHELL_ACCESS_REQUEST_EVENT, (event) => {
      setRequests((prev) =>
        prev.some((r) => r.id === event.payload.id)
          ? prev
          : [...prev, event.payload],
      );
    })
      .then((fn) => (cancelled ? fn() : unlisteners.push(fn)))
      .catch(() => {});

    // Resolved elsewhere or timed out server-side — drop it from the queue.
    listen<{ id: string }>(SHELL_ACCESS_REQUEST_RESOLVED_EVENT, (event) => {
      setRequests((prev) => prev.filter((r) => r.id !== event.payload.id));
    })
      .then((fn) => (cancelled ? fn() : unlisteners.push(fn)))
      .catch(() => {});

    return () => {
      cancelled = true;
      for (const fn of unlisteners) fn();
    };
  }, []);

  const resolve = React.useCallback(
    (id: string, decision: ShellAccessDecision) => {
      // Optimistically remove; the resolved event would also clear it.
      setRequests((prev) => prev.filter((r) => r.id !== id));
      void resolveShellAccessRequest(id, decision).catch(() => {
        // If the resolve failed (already gone), the queue is already correct.
      });
    },
    [],
  );

  return { requests, resolve };
}
