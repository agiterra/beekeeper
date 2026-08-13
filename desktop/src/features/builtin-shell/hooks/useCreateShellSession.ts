import * as React from "react";
import { useNavigate } from "@tanstack/react-router";

import { createShellSession, shellWorkspaceId } from "@/shared/api/tauriShell";
import { useSessionConsent } from "./useSessionConsent";

import { upsertShellSession } from "./useShellSessions";

/**
 * Spawn a new built-in shell session and navigate to it. Creating one is the
 * owner's explicit act, so the human "Interact" consent for that session is
 * granted at creation (revocable in Settings); the separate agent consent
 * stays default-off.
 *
 * `createFor` takes the target project coordinate per call so one hook
 * instance can serve every project group without hooks-in-a-loop.
 */
export function useCreateShellSession(): {
  createFor: (projectRef?: string) => void;
  creating: boolean;
} {
  const { grant } = useSessionConsent();
  const navigate = useNavigate();
  const [creating, setCreating] = React.useState(false);

  const createFor = React.useCallback(
    (projectRef?: string) => {
      if (creating) return;
      setCreating(true);
      createShellSession(projectRef !== undefined ? { projectRef } : undefined)
        .then((info) => {
          grant(shellWorkspaceId(info.sessionId));
          // Make the session visible to every consumer (including the screen
          // we're about to navigate to) before the next poll tick.
          upsertShellSession(info);
          return navigate({
            to: "/shell/$sessionId",
            params: { sessionId: info.sessionId },
          });
        })
        .catch(() => {
          // Backend unavailable; leave the sidebar as-is.
        })
        .finally(() => setCreating(false));
    },
    [creating, grant, navigate],
  );

  return { createFor, creating };
}
