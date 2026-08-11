import * as React from "react";

/**
 * Per-device collapse state for project sidebar groups. Stored in
 * localStorage keyed by (pubkey, relay) — collapse is a lightweight UI
 * preference, so it stays device-local rather than living in the encrypted
 * synced channel-sections blob.
 *
 * Older blobs may carry a `subsections` field from the pre-flat-list tree;
 * it parses harmlessly and is shed the next time the project is toggled.
 */
const STORAGE_PREFIX = "buzz.projects.collapsed.v1";

export type ProjectCollapseState = Record<string, { collapsed?: boolean }>;

export function projectCollapseStorageKey(
  pubkey: string | undefined,
  relayUrl: string | undefined,
): string {
  return `${STORAGE_PREFIX}:${pubkey ?? "anon"}:${relayUrl ?? "unknown"}`;
}

function readState(key: string): ProjectCollapseState {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(key) ?? "{}");
    return parsed && typeof parsed === "object"
      ? (parsed as ProjectCollapseState)
      : {};
  } catch {
    return {};
  }
}

function writeState(key: string, state: ProjectCollapseState): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(state));
  } catch {
    // Storage unavailable; collapse still works for this session.
  }
}

export function useProjectCollapse(
  pubkey: string | undefined,
  relayUrl: string | undefined,
): {
  isProjectCollapsed: (projectId: string) => boolean;
  toggleProject: (projectId: string) => void;
} {
  const storageKey = projectCollapseStorageKey(pubkey, relayUrl);
  const [state, setState] = React.useState<ProjectCollapseState>(() =>
    readState(storageKey),
  );

  // Re-read when the identity/relay scope changes (community switch).
  React.useEffect(() => {
    setState(readState(storageKey));
  }, [storageKey]);

  const update = React.useCallback(
    (mutate: (current: ProjectCollapseState) => ProjectCollapseState) => {
      setState((current) => {
        const next = mutate(current);
        writeState(storageKey, next);
        return next;
      });
    },
    [storageKey],
  );

  const isProjectCollapsed = React.useCallback(
    (projectId: string) => state[projectId]?.collapsed === true,
    [state],
  );

  const toggleProject = React.useCallback(
    (projectId: string) =>
      update((current) => ({
        ...current,
        [projectId]: {
          collapsed: !(current[projectId]?.collapsed === true),
        },
      })),
    [update],
  );

  return {
    isProjectCollapsed,
    toggleProject,
  };
}
