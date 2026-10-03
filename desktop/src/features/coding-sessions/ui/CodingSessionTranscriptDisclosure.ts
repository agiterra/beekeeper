import * as React from "react";

/**
 * Open/closed state for every disclosure in one transcript, keyed by a stable
 * id built from item ids (`tool:<item id>`, `fold:<turn id>`, …).
 *
 * Why a store rather than React state: the transcript used to hold one
 * `Set` of open ids and pass it to every row, so opening one disclosure gave
 * every memoized turn, entry and item a new prop and re-rendered the whole
 * conversation. Here each disclosure subscribes to its own id alone —
 * toggling one re-renders exactly the component that owns it.
 *
 * Why outside the rows: the virtualizer unmounts rows that scroll away. State
 * kept in a row (or in a `<details>` element's own `open`) was lost on the
 * way back. The store lives with the transcript (or with whoever passes one
 * in), so a row remounts already open.
 *
 * Never module-level: a store is created per transcript, so nothing here
 * outlives a community switch (`resetCommunityState`).
 */
export type CodingSessionDisclosureStore = {
  isOpen: (id: string) => boolean;
  /** `(id, open)` so it can be handed straight to an `onOpenChange` prop. */
  setOpen: (id: string, open: boolean) => void;
  subscribe: (id: string, listener: () => void) => () => void;
};

export function createCodingSessionDisclosureStore(
  initiallyOpen: Iterable<string> = [],
): CodingSessionDisclosureStore {
  const open = new Set(initiallyOpen);
  const listeners = new Map<string, Set<() => void>>();
  return {
    isOpen: (id) => open.has(id),
    setOpen: (id, next) => {
      if (open.has(id) === next) return;
      if (next) open.add(id);
      else open.delete(id);
      for (const listener of listeners.get(id) ?? []) listener();
    },
    subscribe: (id, listener) => {
      const forId = listeners.get(id) ?? new Set<() => void>();
      forId.add(listener);
      listeners.set(id, forId);
      return () => {
        forId.delete(listener);
        if (forId.size === 0) listeners.delete(id);
      };
    },
  };
}

export const CodingSessionDisclosureContext =
  React.createContext<CodingSessionDisclosureStore | null>(null);

const NO_SUBSCRIPTION = () => () => {};

/**
 * One disclosure's open state and its setter.
 *
 * Outside a transcript (no store in context) it falls back to local state,
 * so a part rendered on its own still opens and closes.
 */
export function useCodingSessionDisclosure(
  id: string,
): [open: boolean, setOpen: (open: boolean) => void] {
  const store = React.useContext(CodingSessionDisclosureContext);
  const [localOpen, setLocalOpen] = React.useState(false);
  const subscribe = React.useCallback(
    (listener: () => void) =>
      store ? store.subscribe(id, listener) : NO_SUBSCRIPTION(),
    [id, store],
  );
  const getSnapshot = React.useCallback(
    () => (store ? store.isOpen(id) : false),
    [id, store],
  );
  const storedOpen = React.useSyncExternalStore(
    subscribe,
    getSnapshot,
    getSnapshot,
  );
  const setOpen = React.useCallback(
    (next: boolean) => {
      if (store) store.setOpen(id, next);
      else setLocalOpen(next);
    },
    [id, store],
  );
  return [store ? storedOpen : localOpen, setOpen];
}

/**
 * The `{ disclosureId, open, onOpenChange }` props the transcript's parts
 * take, for one id. `onOpenChange` is stable while the id is.
 */
export function useCodingSessionDisclosureProps(disclosureId: string): {
  disclosureId: string;
  open: boolean;
  onOpenChange: (id: string, open: boolean) => void;
} {
  const [open, setOpen] = useCodingSessionDisclosure(disclosureId);
  const onOpenChange = React.useCallback(
    (_id: string, next: boolean) => setOpen(next),
    [setOpen],
  );
  return { disclosureId, open, onOpenChange };
}
