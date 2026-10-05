import * as React from "react";

import {
  type CodingSessionTreeEntry,
  type CodingSessionTreeQuery,
  listCodingSessionTreeEntries,
} from "@/shared/api/tauriCodingSessionTree";

/**
 * The Files surface's tree, one folder at a time (T3 Code's
 * `files/useDirectoryEntries.ts`): a folder is listed when it is first
 * opened, collapsing keeps what was listed, and refresh re-lists only the
 * folders already listed. Nothing is cached past the panel: no module state,
 * so nothing for `resetCommunityState()` to reset.
 */

/** How many entries the host lists per folder before it stops (B0). */
export const CODING_SESSION_TREE_ENTRY_CAP = 2_000;

export type CodingSessionTreeListingState =
  | {
      kind: "ok";
      entries: readonly CodingSessionTreeEntry[];
      truncated: boolean;
      /**
       * Entries the host saw but could not list (non-UTF-8 or unreadable
       * names). Non-zero means the folder's listing is incomplete.
       */
      omitted?: number;
    }
  | { kind: "error"; message: string };

export type CodingSessionTreeRow =
  | { kind: "entry"; key: string; depth: number; entry: CodingSessionTreeEntry }
  | { kind: "notice"; key: string; depth: number; text: string };

/** Folders first, then files, each by name — the order T3's tree shows. */
function sortEntries(
  entries: readonly CodingSessionTreeEntry[],
): CodingSessionTreeEntry[] {
  return [...entries].sort((left, right) => {
    const leftDir = left.kind === "directory" ? 0 : 1;
    const rightDir = right.kind === "directory" ? 0 : 1;
    return leftDir - rightDir || left.name.localeCompare(right.name);
  });
}

function ancestorsOf(relPath: string): string[] {
  const parts = relPath.split("/");
  const out: string[] = [];
  for (let index = 1; index < parts.length; index += 1) {
    out.push(parts.slice(0, index).join("/"));
  }
  return out;
}

/**
 * The rows on screen: a depth-first walk of the listed folders through the
 * expanded ones. With a filter, every listed entry whose name contains it,
 * with the folders that lead to it opened — and nothing that was never
 * listed, which is why the field says "loaded".
 */
export function codingSessionVisibleTreeRows(input: {
  listings: ReadonlyMap<string, CodingSessionTreeListingState>;
  expanded: ReadonlySet<string>;
  filter: string;
}): CodingSessionTreeRow[] {
  const needle = input.filter.trim().toLowerCase();
  let open: ReadonlySet<string> = input.expanded;
  let keep: ReadonlySet<string> | null = null;
  if (needle !== "") {
    const matched = new Set<string>();
    for (const listing of input.listings.values()) {
      if (listing.kind !== "ok") continue;
      for (const entry of listing.entries) {
        if (!entry.name.toLowerCase().includes(needle)) continue;
        matched.add(entry.relPath);
        for (const ancestor of ancestorsOf(entry.relPath))
          matched.add(ancestor);
      }
    }
    keep = matched;
    open = new Set([...matched].filter((path) => input.listings.has(path)));
  }
  const rows: CodingSessionTreeRow[] = [];
  const walk = (dir: string, depth: number) => {
    const listing = input.listings.get(dir);
    if (listing === undefined) {
      rows.push({
        kind: "notice",
        key: `${dir}#loading`,
        depth,
        text: "Loading…",
      });
      return;
    }
    if (listing.kind === "error") {
      rows.push({
        kind: "notice",
        key: `${dir}#error`,
        depth,
        text: `Could not list this folder: ${listing.message}`,
      });
      return;
    }
    for (const entry of sortEntries(listing.entries)) {
      if (keep !== null && !keep.has(entry.relPath)) continue;
      rows.push({ kind: "entry", key: entry.relPath, depth, entry });
      if (entry.kind === "directory" && open.has(entry.relPath)) {
        walk(entry.relPath, depth + 1);
      }
    }
    if (listing.truncated && keep === null) {
      rows.push({
        kind: "notice",
        key: `${dir}#truncated`,
        depth,
        text: `This folder holds more than ${CODING_SESSION_TREE_ENTRY_CAP.toLocaleString("en-US")} entries; the first ${CODING_SESSION_TREE_ENTRY_CAP.toLocaleString("en-US")} are listed.`,
      });
    }
    const omitted = listing.omitted ?? 0;
    if (omitted > 0 && keep === null) {
      rows.push({
        kind: "notice",
        key: `${dir}#omitted`,
        depth,
        text:
          omitted === 1
            ? "1 entry in this folder could not be listed."
            : `${omitted.toLocaleString("en-US")} entries in this folder could not be listed.`,
      });
    }
  };
  if (input.listings.has("")) walk("", 0);
  return rows;
}

/** The listed folders of one session's tree, keyed by relative path. */
export function useCodingSessionTreeDirectories(query: CodingSessionTreeQuery) {
  const queryKey = JSON.stringify(query);
  const [listings, setListings] = React.useState<
    ReadonlyMap<string, CodingSessionTreeListingState>
  >(() => new Map());
  const [pendingCount, setPendingCount] = React.useState(0);
  const inFlight = React.useRef(new Set<string>());
  const generation = React.useRef(0);
  const queryRef = React.useRef(query);
  queryRef.current = query;

  const fetchDirectory = React.useCallback((relPath: string) => {
    if (inFlight.current.has(relPath)) return Promise.resolve();
    inFlight.current.add(relPath);
    const startedIn = generation.current;
    setPendingCount((count) => count + 1);
    return listCodingSessionTreeEntries(queryRef.current, relPath)
      .then(
        (listing): CodingSessionTreeListingState => ({
          kind: "ok",
          entries: listing.entries,
          truncated: listing.truncated,
          omitted: listing.omitted ?? 0,
        }),
        (error: unknown): CodingSessionTreeListingState => ({
          kind: "error",
          message: error instanceof Error ? error.message : String(error),
        }),
      )
      .then((state) => {
        if (startedIn !== generation.current) return;
        setListings((current) => new Map(current).set(relPath, state));
      })
      .finally(() => {
        // A request from an earlier session must not clear the marker of
        // the same folder's request for the current one.
        if (startedIn === generation.current) {
          inFlight.current.delete(relPath);
          setPendingCount((count) => Math.max(0, count - 1));
        }
      });
  }, []);

  const listingsRef = React.useRef(listings);
  listingsRef.current = listings;

  const load = React.useCallback(
    (relPath: string) =>
      listingsRef.current.has(relPath)
        ? Promise.resolve()
        : fetchDirectory(relPath),
    [fetchDirectory],
  );

  const refresh = React.useCallback(() => {
    const listed = [...listingsRef.current.keys()];
    for (const relPath of listed.length > 0 ? listed : [""]) {
      void fetchDirectory(relPath);
    }
  }, [fetchDirectory]);

  // A new session (or a new resolution of it) starts from its root again.
  // biome-ignore lint/correctness/useExhaustiveDependencies: keyed on the query's content, not its identity
  React.useEffect(() => {
    generation.current += 1;
    inFlight.current.clear();
    setListings(new Map());
    setPendingCount(0);
    void fetchDirectory("");
  }, [fetchDirectory, queryKey]);

  return { listings, load, refresh, pending: pendingCount > 0 };
}
