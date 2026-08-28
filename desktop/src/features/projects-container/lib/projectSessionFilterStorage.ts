import * as React from "react";

import {
  DEFAULT_PROJECT_SESSION_FILTER,
  parseProjectSessionFilter,
  type ProjectSessionFilter,
} from "./projectSessionFilter";

/**
 * Per-device session-filter choice for each project sidebar group. Stored in
 * localStorage keyed by (pubkey, relay), exactly like project collapse — a
 * filter is a lightweight viewing preference, not shared state.
 */
const STORAGE_PREFIX = "buzz.projects.sessionFilter.v1";

export type ProjectSessionFilterState = Record<string, ProjectSessionFilter>;

export function projectSessionFilterStorageKey(
  pubkey: string | undefined,
  relayUrl: string | undefined,
): string {
  return `${STORAGE_PREFIX}:${pubkey ?? "anon"}:${relayUrl ?? "unknown"}`;
}

/** Parse a stored blob; every entry is normalised, junk becomes the default. */
export function parseProjectSessionFilterState(
  raw: string | null,
): ProjectSessionFilterState {
  try {
    const parsed: unknown = JSON.parse(raw ?? "{}");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return {};
    }
    const state: ProjectSessionFilterState = {};
    for (const [projectId, value] of Object.entries(parsed)) {
      state[projectId] = parseProjectSessionFilter(value);
    }
    return state;
  } catch {
    return {};
  }
}

function readState(key: string): ProjectSessionFilterState {
  try {
    return parseProjectSessionFilterState(window.localStorage.getItem(key));
  } catch {
    return {};
  }
}

function writeState(key: string, state: ProjectSessionFilterState): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(state));
  } catch {
    // Storage unavailable; the filter still works for this session.
  }
}

export function useProjectSessionFilters(
  pubkey: string | undefined,
  relayUrl: string | undefined,
): {
  getFilter: (projectId: string) => ProjectSessionFilter;
  setFilter: (projectId: string, filter: ProjectSessionFilter) => void;
} {
  const storageKey = projectSessionFilterStorageKey(pubkey, relayUrl);
  const [state, setState] = React.useState<ProjectSessionFilterState>(() =>
    readState(storageKey),
  );

  // Re-read when the identity/relay scope changes (community switch).
  React.useEffect(() => {
    setState(readState(storageKey));
  }, [storageKey]);

  const getFilter = React.useCallback(
    (projectId: string) => state[projectId] ?? DEFAULT_PROJECT_SESSION_FILTER,
    [state],
  );

  const setFilter = React.useCallback(
    (projectId: string, filter: ProjectSessionFilter) => {
      setState((current) => {
        const next = { ...current, [projectId]: filter };
        writeState(storageKey, next);
        return next;
      });
    },
    [storageKey],
  );

  return { getFilter, setFilter };
}
