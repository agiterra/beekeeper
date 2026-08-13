import * as React from "react";

import {
  listSessionAgentConsent,
  setSessionAgentConsent,
} from "@/shared/api/tauriShell";

const POLL_MS = 5000;

/**
 * The per-session **agent** consent, read from the backend (the session broker's
 * source of truth) rather than localStorage — distinct from the human "Interact"
 * consent. Polls the backend so the toggle reflects out-of-band changes, and
 * updates optimistically on toggle.
 */
export function useAgentConsent(): {
  isAgentConsented: (workspaceId: string) => boolean;
  setAgentConsented: (workspaceId: string, allowed: boolean) => void;
} {
  const [consented, setConsented] = React.useState<Set<string>>(new Set());

  const refresh = React.useCallback(() => {
    listSessionAgentConsent()
      .then((ids) => setConsented(new Set(ids)))
      .catch(() => {
        /* backend unreachable (e.g. non-unix); leave as-is */
      });
  }, []);

  React.useEffect(() => {
    refresh();
    const handle = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(handle);
  }, [refresh]);

  const isAgentConsented = React.useCallback(
    (workspaceId: string) => consented.has(workspaceId),
    [consented],
  );

  const setAgentConsented = React.useCallback(
    (workspaceId: string, allowed: boolean) => {
      // Optimistic; reconciled by the next poll.
      setConsented((prev) => {
        const next = new Set(prev);
        if (allowed) next.add(workspaceId);
        else next.delete(workspaceId);
        return next;
      });
      setSessionAgentConsent(workspaceId, allowed).catch(() => refresh());
    },
    [refresh],
  );

  return { isAgentConsented, setAgentConsented };
}
