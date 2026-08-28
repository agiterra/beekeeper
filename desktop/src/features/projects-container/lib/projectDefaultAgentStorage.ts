import * as React from "react";

/**
 * Per-device default agent seat for a project's new coding sessions.
 *
 * Stored in localStorage keyed by (pubkey, relay) like the sidebar collapse
 * state — managed agents are host-local identities (this computer's agent
 * pubkeys), so a default naming one is meaningless on another device and
 * must never enter a relay event.
 *
 * A seat is `{pubkey, role}`: the session create path refuses an actor
 * without a role, so the default carries both.
 */
const STORAGE_PREFIX = "buzz.projects.defaultAgent.v1";

export type ProjectDefaultAgentSeat = {
  pubkey: string;
  role: string;
};

export type ProjectDefaultAgentState = Record<string, ProjectDefaultAgentSeat>;

export function projectDefaultAgentStorageKey(
  pubkey: string | undefined,
  relayUrl: string | undefined,
): string {
  return `${STORAGE_PREFIX}:${pubkey ?? "anon"}:${relayUrl ?? "unknown"}`;
}

export function readProjectDefaultAgentState(
  key: string,
): ProjectDefaultAgentState {
  try {
    const parsed: unknown = JSON.parse(
      window.localStorage.getItem(key) ?? "{}",
    );
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return {};
    }
    const state: ProjectDefaultAgentState = {};
    for (const [projectId, seat] of Object.entries(parsed)) {
      if (!seat || typeof seat !== "object") continue;
      const { pubkey, role } = seat as { pubkey?: unknown; role?: unknown };
      if (typeof pubkey !== "string" || pubkey.length === 0) continue;
      // A seat needs a role (the create path refuses actor-without-role);
      // an emptied-out role reads as the default rather than dropping the seat.
      const trimmedRole = typeof role === "string" ? role.trim() : "";
      state[projectId] = {
        pubkey: pubkey.toLowerCase(),
        role: trimmedRole || "builder",
      };
    }
    return state;
  } catch {
    return {};
  }
}

function writeState(key: string, state: ProjectDefaultAgentState): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(state));
  } catch {
    // Storage unavailable; the default still applies for this session.
  }
}

export function useProjectDefaultAgent(
  pubkey: string | undefined,
  relayUrl: string | undefined,
  projectId: string,
): {
  defaultSeat: ProjectDefaultAgentSeat | null;
  setDefaultSeat: (seat: ProjectDefaultAgentSeat | null) => void;
} {
  const storageKey = projectDefaultAgentStorageKey(pubkey, relayUrl);
  const [state, setState] = React.useState<ProjectDefaultAgentState>(() =>
    readProjectDefaultAgentState(storageKey),
  );

  // Re-read when the identity/relay scope changes (community switch).
  React.useEffect(() => {
    setState(readProjectDefaultAgentState(storageKey));
  }, [storageKey]);

  const setDefaultSeat = React.useCallback(
    (seat: ProjectDefaultAgentSeat | null) => {
      setState((current) => {
        const next = { ...current };
        if (seat) {
          // Role is stored raw so typing (including spaces) round-trips; the
          // read path falls back to "builder" for an emptied-out role.
          next[projectId] = {
            pubkey: seat.pubkey.toLowerCase(),
            role: seat.role,
          };
        } else {
          delete next[projectId];
        }
        writeState(storageKey, next);
        return next;
      });
    },
    [storageKey, projectId],
  );

  const defaultSeat = state[projectId] ?? null;

  return React.useMemo(
    () => ({ defaultSeat, setDefaultSeat }),
    [defaultSeat, setDefaultSeat],
  );
}
