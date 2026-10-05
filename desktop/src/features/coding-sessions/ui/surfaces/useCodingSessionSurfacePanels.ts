import * as React from "react";

/**
 * Which surfaces are open, per session, and the shortcuts that move them
 * (SV-21).
 *
 * The state is `{rightOpen, tabs, active, expanded, bottomOpen}`, matching T3
 * Code's per-thread right-panel store (`rightPanelStore.ts`): opening a
 * surface adds a tab and activates it; closing the active tab falls back to
 * its neighbour; closing the last one closes the panel; the panel itself can
 * be hidden with its tabs remembered.
 *
 * Persisted under `beekeeper:session-panels:v1:<relayUrl>:<channelId>:<sessionKey>`.
 * The key carries the community, so this holds no module cache and needs no
 * entry in `resetCommunityState()`.
 */
export type CodingSessionSurfacePanelState = {
  /** Whether the right panel is showing. */
  rightOpen: boolean;
  /** Right-panel surfaces opened as tabs, in the order they were opened. */
  tabs: readonly string[];
  /** The active tab, or null: the launcher shows while the panel is open. */
  active: string | null;
  /**
   * The panel is maximized and the transcript hidden. Never true together
   * with `bottomOpen`: the drawer lives under the transcript, so an expanded
   * panel would hide a drawer the header calls open. Opening the drawer
   * restores the panel; expanding the panel closes the drawer.
   */
  expanded: boolean;
  /** The bottom drawer (drawer-placement surfaces) is showing. */
  bottomOpen: boolean;
  /**
   * The person has chosen something about these panels (opened, closed,
   * toggled, switched a tab). Persisted, so an automatic open — Agents on a
   * wide team view, Mission's tabs on mount — never overrides a stored choice
   * (T3's `openProactive` / `userActionRevision` rule).
   */
  userActed: boolean;
};

export type CodingSessionSurfacePanelActions = {
  /** Open a surface: a tab (activated) or, for a drawer surface, the drawer. */
  open: (id: string) => void;
  /**
   * Open tabs on the view's own initiative (`activate` becomes active),
   * refused once the person has acted on these panels. Never marks
   * `userActed`.
   */
  openProactive: (ids: readonly string[], activate: string) => void;
  /** The old header toggle: close the panel if `id` is showing, else open it. */
  toggle: (id: string) => void;
  /** Close one tab. */
  close: (id: string) => void;
  /** Close several tabs at once (a lens change). */
  closeMany: (ids: readonly string[]) => void;
  activate: (id: string) => void;
  /** Hide the right panel; its tabs are remembered. */
  closeRight: () => void;
  /** ⌘⌥B. */
  toggleRight: () => void;
  /** ⌘J. */
  toggleBottom: () => void;
  toggleExpanded: () => void;
};

export const CODING_SESSION_SURFACE_PANELS_CLOSED: CodingSessionSurfacePanelState =
  Object.freeze({
    rightOpen: false,
    tabs: Object.freeze([]) as readonly string[],
    active: null,
    expanded: false,
    bottomOpen: false,
    userActed: false,
  });

/** The localStorage key for one session's panels. */
export function codingSessionSurfacePanelsStorageKey(input: {
  relayUrl: string;
  channelId: string;
  sessionKey: string;
}): string {
  return `beekeeper:session-panels:v1:${input.relayUrl}:${input.channelId}:${input.sessionKey}`;
}

/**
 * Parse a persisted value strictly. Anything malformed reads as closed;
 * unknown surface ids (a surface since removed) are dropped.
 */
export function parseCodingSessionSurfacePanels(
  raw: string | null,
  knownIds: ReadonlySet<string>,
): CodingSessionSurfacePanelState {
  if (raw === null) return CODING_SESSION_SURFACE_PANELS_CLOSED;
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return CODING_SESSION_SURFACE_PANELS_CLOSED;
  }
  if (typeof value !== "object" || value === null) {
    return CODING_SESSION_SURFACE_PANELS_CLOSED;
  }
  const record = value as Record<string, unknown>;
  const tabs = Array.isArray(record.tabs)
    ? [
        ...new Set(
          record.tabs.filter(
            (id): id is string => typeof id === "string" && knownIds.has(id),
          ),
        ),
      ]
    : [];
  const active =
    typeof record.active === "string" && tabs.includes(record.active)
      ? record.active
      : null;
  const rightOpen = record.rightOpen === true;
  return {
    rightOpen,
    tabs,
    active,
    expanded:
      rightOpen && record.expanded === true && record.bottomOpen !== true,
    bottomOpen: record.bottomOpen === true,
    userActed: record.userActed === true,
  };
}

export function serializeCodingSessionSurfacePanels(
  state: CodingSessionSurfacePanelState,
): string {
  return JSON.stringify({
    rightOpen: state.rightOpen,
    tabs: state.tabs,
    active: state.active,
    expanded: state.expanded,
    bottomOpen: state.bottomOpen,
    userActed: state.userActed,
  });
}

/** Open (or re-activate) a right-panel tab. */
export function openCodingSessionSurfaceTab(
  state: CodingSessionSurfacePanelState,
  id: string,
): CodingSessionSurfacePanelState {
  return {
    ...state,
    rightOpen: true,
    tabs: state.tabs.includes(id) ? state.tabs : [...state.tabs, id],
    active: id,
  };
}

/**
 * Close tabs. When the active one goes, its neighbour takes over (T3's
 * `closeSurface`); when no tab the lens can see is left, the panel closes.
 */
export function closeCodingSessionSurfaceTabs(
  state: CodingSessionSurfacePanelState,
  ids: readonly string[],
  visibleIds: ReadonlySet<string>,
): CodingSessionSurfacePanelState {
  if (!ids.some((id) => state.tabs.includes(id))) return state;
  const closing = new Set(ids);
  const tabs = state.tabs.filter((id) => !closing.has(id));
  const visibleBefore = state.tabs.filter((id) => visibleIds.has(id));
  const visibleAfter = tabs.filter((id) => visibleIds.has(id));
  let active = state.active;
  if (active !== null && closing.has(active)) {
    const index = visibleBefore.indexOf(active);
    active =
      visibleAfter[Math.min(Math.max(index, 0), visibleAfter.length - 1)] ??
      null;
  }
  const rightOpen = state.rightOpen && visibleAfter.length > 0;
  return {
    ...state,
    tabs,
    active,
    rightOpen,
    expanded: rightOpen && state.expanded,
  };
}

/**
 * The state a lens can see: tabs it lists, and an active tab that is one of
 * them (else the newest visible tab, else the launcher).
 */
export function visibleCodingSessionSurfacePanels(
  state: CodingSessionSurfacePanelState,
  visibleIds: ReadonlySet<string>,
): CodingSessionSurfacePanelState {
  const tabs = state.tabs.filter((id) => visibleIds.has(id));
  const active =
    state.active !== null && tabs.includes(state.active)
      ? state.active
      : (tabs.at(-1) ?? null);
  if (tabs.length === state.tabs.length && active === state.active) {
    return state;
  }
  return { ...state, tabs, active };
}

/**
 * Whether an expanded right panel hides the transcript (SV-21's maximize).
 * Only inline: a narrow window shows the panel as a sheet over the
 * transcript, so the transcript stays mounted underneath.
 */
export function isTranscriptHiddenByPanel(
  state: CodingSessionSurfacePanelState,
  isNarrow: boolean,
): boolean {
  return state.rightOpen && state.expanded && !isNarrow;
}

type CodingSessionSurfacePanelAction =
  | { type: "open"; id: string; drawer: boolean }
  | { type: "openProactive"; ids: readonly string[]; activate: string }
  | { type: "toggle"; id: string; drawer: boolean }
  | { type: "close"; ids: readonly string[]; visibleIds: ReadonlySet<string> }
  | { type: "activate"; id: string }
  | { type: "closeRight" }
  | { type: "toggleRight" }
  | { type: "toggleBottom" }
  | { type: "toggleExpanded" }
  | { type: "replace"; state: CodingSessionSurfacePanelState };

/**
 * Pure transitions for every action, for the hook and its tests.
 *
 * Every action but `openProactive` and `replace` is the person's own, so a
 * change it makes also records `userActed`.
 */
export function reduceCodingSessionSurfacePanels(
  state: CodingSessionSurfacePanelState,
  action: CodingSessionSurfacePanelAction,
): CodingSessionSurfacePanelState {
  if (action.type === "openProactive") {
    if (state.userActed) return state;
    let next = state;
    for (const id of action.ids) next = openCodingSessionSurfaceTab(next, id);
    return next.tabs.includes(action.activate)
      ? { ...next, active: action.activate }
      : next;
  }
  if (action.type === "replace") return action.state;
  const next = reducePersonAction(state, action);
  return next === state || next.userActed ? next : { ...next, userActed: true };
}

function reducePersonAction(
  state: CodingSessionSurfacePanelState,
  action: Exclude<
    CodingSessionSurfacePanelAction,
    { type: "openProactive" } | { type: "replace" }
  >,
): CodingSessionSurfacePanelState {
  switch (action.type) {
    case "open":
      return action.drawer
        ? openCodingSessionDrawer(state)
        : openCodingSessionSurfaceTab(state, action.id);
    case "toggle":
      if (action.drawer) return toggleCodingSessionDrawer(state);
      return state.rightOpen && state.active === action.id
        ? { ...state, rightOpen: false, expanded: false }
        : openCodingSessionSurfaceTab(state, action.id);
    case "close":
      return closeCodingSessionSurfaceTabs(
        state,
        action.ids,
        action.visibleIds,
      );
    case "activate":
      return state.tabs.includes(action.id)
        ? { ...state, rightOpen: true, active: action.id }
        : state;
    case "closeRight":
      return state.rightOpen
        ? { ...state, rightOpen: false, expanded: false }
        : state;
    case "toggleRight":
      return state.rightOpen
        ? { ...state, rightOpen: false, expanded: false }
        : { ...state, rightOpen: true };
    case "toggleBottom":
      return toggleCodingSessionDrawer(state);
    case "toggleExpanded":
      if (!state.rightOpen) return state;
      return state.expanded
        ? { ...state, expanded: false }
        : { ...state, expanded: true, bottomOpen: false };
  }
}

/** Opens the drawer, restoring an expanded panel so the drawer is visible. */
function openCodingSessionDrawer(
  state: CodingSessionSurfacePanelState,
): CodingSessionSurfacePanelState {
  return { ...state, bottomOpen: true, expanded: false };
}

function toggleCodingSessionDrawer(
  state: CodingSessionSurfacePanelState,
): CodingSessionSurfacePanelState {
  return state.bottomOpen
    ? { ...state, bottomOpen: false }
    : openCodingSessionDrawer(state);
}

function readStorage(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // A full or blocked store loses persistence, never the panel.
  }
}

/**
 * One session's panels: state (as the lens sees it), and its actions.
 *
 * `knownIds` is every registered surface (so a persisted tab for a surface
 * another lens lists survives a lens change); `visibleIds` are the ones the
 * current lens lists; `drawerIds` open the drawer instead of a tab.
 */
export function useCodingSessionSurfacePanels(input: {
  storageKey: string;
  knownIds: ReadonlySet<string>;
  visibleIds: ReadonlySet<string>;
  drawerIds: ReadonlySet<string>;
}): {
  state: CodingSessionSurfacePanelState;
  actions: CodingSessionSurfacePanelActions;
} {
  const { drawerIds, knownIds, storageKey } = input;
  const [stored, setStored] = React.useState(() => ({
    key: storageKey,
    state: parseCodingSessionSurfacePanels(readStorage(storageKey), knownIds),
  }));
  // A different session in the same mounted workspace reads its own record.
  const current =
    stored.key === storageKey
      ? stored.state
      : parseCodingSessionSurfacePanels(readStorage(storageKey), knownIds);
  if (stored.key !== storageKey) setStored({ key: storageKey, state: current });

  const visibleRef = React.useRef(input.visibleIds);
  visibleRef.current = input.visibleIds;
  const dispatch = React.useCallback(
    (action: CodingSessionSurfacePanelAction) =>
      setStored((previous) => {
        const next = reduceCodingSessionSurfacePanels(previous.state, action);
        if (next === previous.state) return previous;
        writeStorage(previous.key, serializeCodingSessionSurfacePanels(next));
        return { key: previous.key, state: next };
      }),
    [],
  );
  const drawerRef = React.useRef(drawerIds);
  drawerRef.current = drawerIds;
  const actions = React.useMemo<CodingSessionSurfacePanelActions>(
    () => ({
      open: (id) =>
        dispatch({ type: "open", id, drawer: drawerRef.current.has(id) }),
      openProactive: (ids, activate) =>
        dispatch({ type: "openProactive", ids, activate }),
      toggle: (id) =>
        dispatch({ type: "toggle", id, drawer: drawerRef.current.has(id) }),
      close: (id) =>
        dispatch({ type: "close", ids: [id], visibleIds: visibleRef.current }),
      closeMany: (ids) =>
        dispatch({ type: "close", ids, visibleIds: visibleRef.current }),
      activate: (id) => dispatch({ type: "activate", id }),
      closeRight: () => dispatch({ type: "closeRight" }),
      toggleRight: () => dispatch({ type: "toggleRight" }),
      toggleBottom: () => dispatch({ type: "toggleBottom" }),
      toggleExpanded: () => dispatch({ type: "toggleExpanded" }),
    }),
    [dispatch],
  );
  const state = visibleCodingSessionSurfacePanels(current, input.visibleIds);
  return { state, actions };
}

/** Is this keydown ⌘J (Ctrl+J off macOS), with no other modifier? */
export function isCodingSessionDrawerShortcut(
  event: Pick<
    KeyboardEvent,
    "altKey" | "code" | "ctrlKey" | "isComposing" | "metaKey" | "shiftKey"
  >,
): boolean {
  return (
    event.code === "KeyJ" &&
    (event.metaKey || event.ctrlKey) &&
    !event.altKey &&
    !event.shiftKey &&
    !event.isComposing
  );
}

/** Is this keydown ⌘⌥B (Ctrl+Alt+B off macOS)? `code`, since ⌥B types "∫". */
export function isCodingSessionRightPanelShortcut(
  event: Pick<
    KeyboardEvent,
    "altKey" | "code" | "ctrlKey" | "isComposing" | "metaKey" | "shiftKey"
  >,
): boolean {
  return (
    event.code === "KeyB" &&
    (event.metaKey || event.ctrlKey) &&
    event.altKey &&
    !event.shiftKey &&
    !event.isComposing
  );
}

/**
 * Bind ⌘J (drawer) and ⌘⌥B (right panel) while the workspace is mounted.
 *
 * ⌘J's keydown and keyup both stop here (`stopImmediatePropagation`), so no
 * later window listener also acts on the chord. The channel terminal's own
 * ⌘J listener mounts first; it stands down on a session route by its own
 * route flag (`shortcutStandsDown`, DB10, B4's half).
 */
export function useCodingSessionSurfaceShortcuts(
  actions: Pick<
    CodingSessionSurfacePanelActions,
    "toggleBottom" | "toggleRight"
  >,
): void {
  React.useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (isCodingSessionDrawerShortcut(event)) {
        event.preventDefault();
        event.stopImmediatePropagation();
        if (event.type === "keydown") actions.toggleBottom();
      } else if (
        event.type === "keydown" &&
        isCodingSessionRightPanelShortcut(event)
      ) {
        event.preventDefault();
        event.stopPropagation();
        actions.toggleRight();
      }
    };
    window.addEventListener("keydown", handler, true);
    window.addEventListener("keyup", handler, true);
    return () => {
      window.removeEventListener("keydown", handler, true);
      window.removeEventListener("keyup", handler, true);
    };
  }, [actions]);
}

/**
 * The header's two panel toggles, for B1 (SV-20). Optional on the header
 * until B1 renders it; built from the same state the panels use.
 */
export type CodingSessionHeaderPanels = {
  rightOpen: boolean;
  onToggleRight: () => void;
  bottomOpen: boolean;
  onToggleBottom: () => void;
  /** The drawer surface's reason when it cannot open here, else null. */
  bottomUnavailableReason: string | null;
};
