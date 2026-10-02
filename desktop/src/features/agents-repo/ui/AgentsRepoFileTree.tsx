import * as React from "react";

import type { AgentsRepoEntry } from "@/shared/api/agentsRepoTypes";
import { cn } from "@/shared/lib/cn";

import type { DraftPath } from "../lib/agentsRepoDraftFold";
import { displayName, groupPaths, kindOf } from "../lib/agentsRepoPaths";
import { AgentsRepoDocumentGroup } from "./AgentsRepoDocumentGroup";

export type TreeRow = AgentsRepoEntry & {
  /** The open draft head on this path, if any. */
  draft: DraftPath | null;
  /** The path exists only as a draft; `main` has no such file. */
  notOnMain: boolean;
};

/** Merge the listing of `main` with the open drafts into one set of rows. */
export function treeRows(
  entries: readonly AgentsRepoEntry[],
  drafts: readonly DraftPath[],
): TreeRow[] {
  const byPath = new Map<string, TreeRow>();
  for (const entry of entries) {
    byPath.set(entry.path, { ...entry, draft: null, notOnMain: false });
  }
  for (const draft of drafts) {
    const existing = byPath.get(draft.path);
    if (existing) {
      existing.draft = draft;
      continue;
    }
    byPath.set(draft.path, {
      path: draft.path,
      blob: "",
      size: 0,
      kind: kindOf(draft.path),
      draft,
      notOnMain: true,
    });
  }
  // A `.gitkeep` outside the documents tree is the seed holding a directory
  // open, not a file anyone edits. Under `docs/` it is classified
  // `document-folder` and kept, because there it *is* the folder.
  return [...byPath.values()].filter((row) => row.kind !== "gitkeep");
}

export function AgentsRepoFileTree({
  rows,
  selectedPath,
  onSelect,
  personName,
  isPinned,
}: {
  rows: readonly TreeRow[];
  selectedPath: string | null;
  onSelect: (path: string) => void;
  personName: (pubkey: string) => string;
  /** Whether a file path or folder prefix is pinned to the sidebar. */
  isPinned: (target: string) => boolean;
}) {
  const groups = React.useMemo(() => groupPaths(rows), [rows]);
  return (
    <nav
      aria-label="Agents repository files"
      className="space-y-4"
      data-testid="agents-repo-tree"
    >
      {groups.map((group) => (
        <section key={group.group}>
          <h3 className="mb-1 px-2 text-2xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">
            {group.label}
          </h3>
          {group.group === "documents" ? (
            <AgentsRepoDocumentGroup
              isPinned={isPinned}
              onSelect={onSelect}
              personName={personName}
              rows={group.entries}
              selectedPath={selectedPath}
            />
          ) : (
            <ul className="space-y-0.5">
              {group.entries.map((row) => {
                const selected = row.path === selectedPath;
                return (
                  <li key={row.path}>
                    <button
                      aria-current={selected ? "page" : undefined}
                      className={cn(
                        "flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-sm",
                        selected
                          ? "bg-accent text-accent-foreground"
                          : "text-foreground hover:bg-muted/60",
                        row.notOnMain && "italic",
                      )}
                      data-testid={`agents-repo-file-${row.path}`}
                      onClick={() => onSelect(row.path)}
                      title={row.path}
                      type="button"
                    >
                      <span className="min-w-0 flex-1 truncate">
                        {displayName(row.path)}
                      </span>
                      {row.draft ? (
                        <span
                          className="shrink-0 rounded-full bg-amber-500/15 px-1.5 py-0.5 text-2xs font-medium text-amber-700 dark:text-amber-300"
                          data-testid={`agents-repo-draft-badge-${row.path}`}
                          title={`Draft by ${personName(row.draft.head.author)}`}
                        >
                          draft
                        </span>
                      ) : null}
                      {row.notOnMain ? (
                        <span className="shrink-0 text-2xs text-muted-foreground">
                          not on main
                        </span>
                      ) : null}
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      ))}
      {groups.length === 0 ? (
        <p className="px-2 text-sm text-muted-foreground">No files.</p>
      ) : null}
    </nav>
  );
}
