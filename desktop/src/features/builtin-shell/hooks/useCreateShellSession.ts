import * as React from "react";
import { useNavigate } from "@tanstack/react-router";

import { createShellSession } from "@/shared/api/tauriShell";

import { upsertShellSession } from "./useShellSessions";

/**
 * Spawn a new built-in shell session and navigate to it. The owner always
 * has interact rights on their own sessions; access for others (people or
 * agents) is granted by inviting them to the session's roster.
 *
 * `createFor` takes the target project coordinate per call so one hook
 * instance can serve every project group without hooks-in-a-loop.
 */
export function useCreateShellSession(): {
  createFor: (projectRef?: string, cwd?: string) => void;
  creating: boolean;
} {
  const navigate = useNavigate();
  const [creating, setCreating] = React.useState(false);

  const createFor = React.useCallback(
    (projectRef?: string, cwd?: string) => {
      if (creating) return;
      setCreating(true);
      createShellSession(
        projectRef !== undefined || cwd !== undefined
          ? { projectRef, cwd }
          : undefined,
      )
        .then((info) => {
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
    [creating, navigate],
  );

  return { createFor, creating };
}
