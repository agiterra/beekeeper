import { ChevronDown, ChevronRight, FileText, Folder, Pin } from "lucide-react";
import * as React from "react";

import { cn } from "@/shared/lib/cn";

import {
  type DocumentTreeNode,
  buildDocumentTree,
  documentAncestorFolders,
} from "../lib/agentsRepoDocumentTree";
import type { TreeRow } from "./AgentsRepoFileTree";

/**
 * The documents group of the Artifacts tab, nested.
 *
 * Every other group is a flat list, because every other tree in the layout is
 * flat. This one has folders, so it has disclosure, and a folder row is
 * selectable in its own right — it is a pinnable target (NIP-AR), not only a
 * container.
 */
export function AgentsRepoDocumentGroup({
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
  /** Whether this file path or folder prefix is pinned to the sidebar. */
  isPinned: (target: string) => boolean;
}) {
  const tree = React.useMemo(
    () => buildDocumentTree(rows, (row) => row.path),
    [rows],
  );
  // Expanded by default: a tree with two folders in it should not make
  // somebody click twice to see three files. Collapsing is per-device and
  // deliberately not remembered across reloads — the tree is small, and a
  // folder a person collapsed last week is not a reason to hide a document
  // somebody pinned today.
  const [collapsed, setCollapsed] = React.useState<ReadonlySet<string>>(
    () => new Set(),
  );
  // A selection always reveals itself, even inside a folder that was
  // collapsed: opening a document from a pin or a deep link must show it.
  const revealed = React.useMemo(
    () => new Set(selectedPath ? documentAncestorFolders(selectedPath) : []),
    [selectedPath],
  );
  const isOpen = (path: string) => revealed.has(path) || !collapsed.has(path);
  const toggle = (path: string) =>
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });

  const render = (nodes: readonly DocumentTreeNode<TreeRow>[], depth: number) =>
    nodes.map((node) => {
      const selected = node.path === selectedPath;
      const pinned = isPinned(node.path);
      const indent = { paddingLeft: `${0.5 + depth * 0.75}rem` };
      if (node.kind === "folder") {
        const open = isOpen(node.path);
        return (
          <li key={node.path}>
            <div
              className={cn(
                "flex w-full items-center gap-1 rounded-md pr-2 text-sm",
                selected
                  ? "bg-accent text-accent-foreground"
                  : "text-foreground hover:bg-muted/60",
              )}
              style={indent}
            >
              <button
                aria-expanded={open}
                aria-label={`${open ? "Collapse" : "Expand"} ${node.name}`}
                className="shrink-0 rounded p-0.5 text-muted-foreground hover:text-foreground"
                data-testid={`agents-repo-folder-toggle-${node.path}`}
                onClick={() => toggle(node.path)}
                type="button"
              >
                {open ? (
                  <ChevronDown aria-hidden className="size-3.5" />
                ) : (
                  <ChevronRight aria-hidden className="size-3.5" />
                )}
              </button>
              <button
                aria-current={selected ? "page" : undefined}
                className="flex min-w-0 flex-1 items-center gap-2 py-1 text-left"
                data-testid={`agents-repo-folder-${node.path}`}
                onClick={() => onSelect(node.path)}
                title={node.path}
                type="button"
              >
                <Folder aria-hidden className="size-3.5 shrink-0 opacity-70" />
                <span className="min-w-0 flex-1 truncate">{node.name}</span>
                {pinned ? (
                  <Pin
                    aria-label="Pinned to the sidebar"
                    className="size-3 shrink-0 text-muted-foreground"
                    data-testid={`agents-repo-pinned-${node.path}`}
                  />
                ) : null}
              </button>
            </div>
            {open && node.children.length > 0 ? (
              <ul className="space-y-0.5">
                {render(node.children, depth + 1)}
              </ul>
            ) : null}
          </li>
        );
      }
      const row = node.entry;
      return (
        <li key={node.path}>
          <button
            aria-current={selected ? "page" : undefined}
            className={cn(
              "flex w-full items-center gap-2 rounded-md py-1 pr-2 text-left text-sm",
              selected
                ? "bg-accent text-accent-foreground"
                : "text-foreground hover:bg-muted/60",
              row.notOnMain && "italic",
            )}
            data-testid={`agents-repo-file-${node.path}`}
            onClick={() => onSelect(node.path)}
            style={indent}
            title={node.path}
            type="button"
          >
            <FileText aria-hidden className="size-3.5 shrink-0 opacity-70" />
            <span className="min-w-0 flex-1 truncate">{node.name}</span>
            {pinned ? (
              <Pin
                aria-label="Pinned to the sidebar"
                className="size-3 shrink-0 text-muted-foreground"
                data-testid={`agents-repo-pinned-${node.path}`}
              />
            ) : null}
            {row.draft ? (
              <span
                className="shrink-0 rounded-full bg-amber-500/15 px-1.5 py-0.5 text-2xs font-medium text-amber-700 dark:text-amber-300"
                data-testid={`agents-repo-draft-badge-${node.path}`}
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
    });

  if (tree.length === 0) return null;
  return (
    <ul className="space-y-0.5" data-testid="agents-repo-document-tree">
      {render(tree, 0)}
    </ul>
  );
}
