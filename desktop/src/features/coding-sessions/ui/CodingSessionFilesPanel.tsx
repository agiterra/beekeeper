import * as React from "react";
import { useNavigate } from "@tanstack/react-router";
import { ChevronRight, ChevronsDownUp, RefreshCw } from "lucide-react";

import {
  AgentsRepoFileTree,
  treeRows,
} from "@/features/agents-repo/ui/AgentsRepoFileTree";
import {
  codingSessionTreeElsewhereClause,
  codingSessionTreeProviderName,
} from "@/features/coding-sessions/lib/codingSessionTerminalModel";
import type { CodingSessionTreeEntry } from "@/shared/api/tauriCodingSessionTree";
import { cn } from "@/shared/lib/cn";
import { truncatePubkey } from "@/shared/lib/pubkey";

import { CodingSessionSurfaceSubheader } from "./CodingSessionChangesRailSubheader";
import { useCodingSessionAgentsRepoRead } from "./CodingSessionFilesAgentsRepoRead";
import { codingSessionFileIcon } from "./CodingSessionFilesPanelIcons";
import {
  codingSessionVisibleTreeRows,
  useCodingSessionTreeDirectories,
} from "./CodingSessionFilesPanelTree";
import type { CodingSessionSurfaceCtx } from "./surfaces/codingSessionSurfaceContext";
import { CodingSessionTreeDefaultNotice } from "./surfaces/CodingSessionSurfaceTerminal";

/**
 * The Files surface (SV-24): this session's working tree, read-only, and the
 * project's agents repository.
 *
 * T3 Code's file browser (`files/FileBrowserPanel.tsx`) lists a thread's
 * working directory lazily, one folder at a time, with a refresh control, a
 * search field and expand/collapse-all in a 40px subheader. Beekeeper's tree
 * comes from the host's own resolution (B0's `list_coding_session_tree_entries`):
 * relative paths only, `.git` hidden, at most 2,000 entries per folder, and
 * only where this computer has the tree. Elsewhere the section is one line
 * saying whose computer it is on. Two deliberate differences: the field
 * filters the folders already opened (there is no host search command, so it
 * says "loaded"), and a file row opens nothing (there is no read command for
 * the session's tree; the listing is the whole of it).
 */
export function CodingSessionFilesPanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-testid="coding-session-files"
    >
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
        <WorkingTreeSection ctx={ctx} />
        <AgentsRepoSection ctx={ctx} />
      </div>
    </div>
  );
}

/**
 * The one line a session whose tree is on another computer gets: the shared
 * clause the Terminal surface's reason is built on
 * (`codingSessionTreeElsewhereClause`), so the two state one fact one way.
 */
export function codingSessionFilesElsewhereLine(
  ctx: Pick<
    CodingSessionSurfaceCtx,
    "focusedExecution" | "focusedRecord" | "resolveActorName"
  >,
): string {
  return `${codingSessionTreeElsewhereClause(codingSessionTreeProviderName(ctx))}.`;
}

function WorkingTreeSection({ ctx }: { ctx: CodingSessionSurfaceCtx }) {
  const { tree } = ctx;
  if (!tree.available) {
    const line =
      tree.state === "loading"
        ? "Checking where this session's working tree is."
        : tree.refusal === "notLocal"
          ? codingSessionFilesElsewhereLine(ctx)
          : (tree.reason ??
            "No working tree for this session is recorded on this computer.");
    return (
      <section data-testid="coding-session-files-tree">
        <CodingSessionSurfaceSubheader title="Working tree" />
        <p
          className="px-3 py-3 text-xs text-muted-foreground"
          data-testid="coding-session-files-tree-line"
        >
          {line}
        </p>
      </section>
    );
  }
  return <WorkingTreeListing ctx={ctx} />;
}

function WorkingTreeListing({ ctx }: { ctx: CodingSessionSurfaceCtx }) {
  const { tree } = ctx;
  const directories = useCodingSessionTreeDirectories(tree.query);
  const [expanded, setExpanded] = React.useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const [filter, setFilter] = React.useState("");
  const { load } = directories;
  const toggle = React.useCallback(
    (relPath: string) => {
      setExpanded((current) => {
        const next = new Set(current);
        if (next.has(relPath)) next.delete(relPath);
        else {
          next.add(relPath);
          void load(relPath);
        }
        return next;
      });
    },
    [load],
  );
  const rows = codingSessionVisibleTreeRows({
    listings: directories.listings,
    expanded,
    filter,
  });
  const root = directories.listings.get("");
  return (
    <section data-testid="coding-session-files-tree">
      <CodingSessionSurfaceSubheader
        actions={
          <>
            <IconButton
              label="Refresh files"
              onClick={directories.refresh}
              testId="coding-session-files-refresh"
            >
              <RefreshCw
                aria-hidden
                className={cn(
                  "size-3.5",
                  directories.pending && "animate-spin",
                )}
              />
            </IconButton>
            {expanded.size > 0 ? (
              <IconButton
                label="Collapse all folders"
                onClick={() => setExpanded(new Set())}
                testId="coding-session-files-collapse"
              >
                <ChevronsDownUp aria-hidden className="size-3.5" />
              </IconButton>
            ) : null}
          </>
        }
        meta={
          <span title="Where this computer recorded the session's tree. The path itself stays on this computer.">
            {tree.label} · read-only
          </span>
        }
        title="Working tree"
      />
      <div className="px-3 pt-2">
        <CodingSessionTreeDefaultNotice ctx={ctx} />
        <input
          aria-label="Filter loaded files"
          className="h-7 w-full rounded-md border border-border/60 bg-transparent px-2 text-xs placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          data-testid="coding-session-files-filter"
          onChange={(event) => setFilter(event.target.value)}
          placeholder="Filter loaded files"
          spellCheck={false}
          type="search"
          value={filter}
        />
      </div>
      {root?.kind === "error" ? (
        <button
          className="px-3 py-2 text-left text-xs text-muted-foreground"
          onClick={directories.refresh}
          type="button"
        >
          The tree could not be listed: {root.message} Click to retry.
        </button>
      ) : root === undefined ? (
        <p className="px-3 py-2 text-xs text-muted-foreground" role="status">
          Loading files…
        </p>
      ) : (
        <ul
          aria-label="Working tree files"
          className="py-1"
          data-testid="coding-session-files-tree-rows"
        >
          {rows.length === 0 ? (
            <li className="px-3 py-1 text-xs text-muted-foreground">
              {filter.trim()
                ? "No loaded file matches."
                : "This folder is empty."}
            </li>
          ) : null}
          {rows.map((row) =>
            row.kind === "notice" ? (
              <li
                className="py-1 pr-3 text-2xs text-muted-foreground"
                key={row.key}
                style={{ paddingLeft: `${row.depth * 12 + 28}px` }}
              >
                {row.text}
              </li>
            ) : (
              <TreeRow
                depth={row.depth}
                entry={row.entry}
                expanded={expanded.has(row.entry.relPath)}
                key={row.key}
                onToggle={toggle}
              />
            ),
          )}
        </ul>
      )}
    </section>
  );
}

function TreeRow({
  depth,
  entry,
  expanded,
  onToggle,
}: {
  depth: number;
  entry: CodingSessionTreeEntry;
  expanded: boolean;
  onToggle: (relPath: string) => void;
}) {
  const isDirectory = entry.kind === "directory";
  // T3's rows: a folder is its chevron and name; a file is its type's
  // coloured glyph in the same column, so names line up at every depth.
  const fileIcon = isDirectory ? null : codingSessionFileIcon(entry);
  const content = (
    <>
      {fileIcon === null ? (
        <ChevronRight
          aria-hidden
          className={cn(
            "size-3.5 shrink-0 text-muted-foreground transition-transform",
            expanded && "rotate-90",
          )}
        />
      ) : (
        <fileIcon.Icon
          aria-hidden
          className={cn("size-3.5 shrink-0", fileIcon.className)}
        />
      )}
      <span className="min-w-0 truncate">{entry.name}</span>
    </>
  );
  const style = { paddingLeft: `${depth * 12 + 8}px` };
  return (
    <li
      data-kind={entry.kind}
      data-testid="coding-session-files-tree-row"
      title={entry.relPath}
    >
      {isDirectory ? (
        <button
          aria-expanded={expanded}
          className="flex h-6 w-full items-center gap-1.5 pr-3 text-left text-xs hover:bg-muted/50 focus-visible:bg-muted/50 focus-visible:outline-none"
          onClick={() => onToggle(entry.relPath)}
          style={style}
          type="button"
        >
          {content}
        </button>
      ) : (
        <div
          className="flex h-6 items-center gap-1.5 pr-3 text-xs"
          style={style}
        >
          {content}
        </div>
      )}
    </li>
  );
}

function IconButton({
  children,
  label,
  onClick,
  testId,
}: {
  children: React.ReactNode;
  label: string;
  onClick: () => void;
  testId: string;
}) {
  return (
    <button
      aria-label={label}
      className="inline-flex size-6 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      data-testid={testId}
      onClick={onClick}
      title={label}
      type="button"
    >
      {children}
    </button>
  );
}

/**
 * The project's agents repository, `main` only, through the same reads the
 * project's Files screen makes. Relay-backed, so every member sees the same
 * list; a row opens that screen at the file.
 */
function AgentsRepoSection({ ctx }: { ctx: CodingSessionSurfaceCtx }) {
  const { project, coordinate, sourceQuery, isAgentsRepo, listing } =
    useCodingSessionAgentsRepoRead(ctx, true);
  const rows = React.useMemo(
    () => treeRows(listing.data?.entries ?? [], []),
    [listing.data],
  );
  const navigate = useNavigate();
  let line: string | null = null;
  if (ctx.projectRef === null) {
    line =
      "This session belongs to no project, so it has no agents repository.";
  } else if (coordinate === null) {
    line = "This session's project is not readable here.";
  } else if (sourceQuery.isPending || (isAgentsRepo && listing.isPending)) {
    line = "Reading the agents repository.";
  } else if (sourceQuery.isError) {
    line = "The project's agents repository record was not read.";
  } else if (!isAgentsRepo) {
    line = "This project has no agents repository.";
  } else if (listing.isError) {
    line = `The agents repository was not listed: ${
      listing.error instanceof Error ? listing.error.message : "unknown error"
    }`;
  }
  return (
    <section
      className="border-t border-border/60"
      data-testid="coding-session-files-agents-repo"
    >
      <CodingSessionSurfaceSubheader
        meta={
          listing.data
            ? `main at ${listing.data.commit.slice(0, 7)} · the same for every member`
            : "the same for every member"
        }
        title="Agents repo"
      />
      {line !== null ? (
        <p className="px-3 py-3 text-xs text-muted-foreground">{line}</p>
      ) : (
        <div className="p-2">
          <AgentsRepoFileTree
            isPinned={() => false}
            onSelect={(path) => {
              if (!project) return;
              void navigate({
                to: "/projects/$projectId/files",
                params: { projectId: project.id },
                search: { path },
              });
            }}
            personName={(pubkey) =>
              ctx.resolveActorName(pubkey)?.trim() || truncatePubkey(pubkey)
            }
            rows={rows}
            selectedPath={null}
          />
        </div>
      )}
    </section>
  );
}
