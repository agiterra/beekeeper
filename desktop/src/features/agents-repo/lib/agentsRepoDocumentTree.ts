/**
 * Shaping the documents tree for the Artifacts tab.
 *
 * `plans/`, `roles/` and `skills/` list flat, grouped (`agentsRepoPaths.ts`).
 * `docs/` is the one tree with folders, so it is the one that needs a shape:
 * folders nest, a folder sorts before a file at the same level, and a folder
 * someone created and has not filled yet is a row of its own — it exists in
 * git only as its `.gitkeep`, and hiding it would make "new folder" something
 * the app claims and the tree never shows.
 *
 * The keep itself is never a row. It is the folder.
 */
import { DOCS_ROOT } from "./agentsRepoDraftOp";

export type DocumentTreeFile<T> = {
  kind: "file";
  /** The full repository path. */
  path: string;
  /** The last segment, which is what the row shows. */
  name: string;
  entry: T;
};

export type DocumentTreeFolder<T> = {
  kind: "folder";
  /** The folder prefix, e.g. `docs/mockups` — what a pin names. */
  path: string;
  name: string;
  children: DocumentTreeNode<T>[];
  /**
   * The folder exists only because something under it does; no `.gitkeep`
   * names it. Not shown differently today — recorded because a client that
   * wants to offer "keep this folder" needs to know which have no keep.
   */
  implied: boolean;
};

export type DocumentTreeNode<T> = DocumentTreeFile<T> | DocumentTreeFolder<T>;

/** The segments of a path under `docs/`, or null when it is not one. */
function componentsUnderDocs(path: string): string[] | null {
  const prefix = `${DOCS_ROOT}/`;
  if (!path.startsWith(prefix)) return null;
  const rest = path.slice(prefix.length);
  if (rest.length === 0) return null;
  return rest.split("/");
}

/**
 * Build the nested shape of the documents tree from flat rows.
 *
 * `pathOf` reads a row's repository path. Rows outside `docs/` are skipped, so
 * a caller may pass the whole listing.
 */
export function buildDocumentTree<T>(
  rows: readonly T[],
  pathOf: (row: T) => string,
): DocumentTreeNode<T>[] {
  type Build = {
    folders: Map<string, Build>;
    files: DocumentTreeFile<T>[];
    /** A `.gitkeep` names this folder, so it is not merely implied. */
    kept: boolean;
  };
  const empty = (): Build => ({ folders: new Map(), files: [], kept: false });
  const root = empty();

  for (const row of rows) {
    const path = pathOf(row);
    const components = componentsUnderDocs(path);
    if (components === null) continue;
    const file = components[components.length - 1] ?? "";
    const folders = components.slice(0, -1);
    let at = root;
    for (const folder of folders) {
      let next = at.folders.get(folder);
      if (!next) {
        next = empty();
        at.folders.set(folder, next);
      }
      at = next;
    }
    if (file === ".gitkeep") {
      // The keep *is* the folder: it marks this level as deliberately kept
      // and is never a row of its own.
      at.kept = true;
      continue;
    }
    at.files.push({ kind: "file", path, name: file, entry: row });
  }

  const nodes = (build: Build, prefix: string): DocumentTreeNode<T>[] => {
    const folders: DocumentTreeFolder<T>[] = [...build.folders.entries()]
      .map(([name, child]) => ({
        kind: "folder" as const,
        path: `${prefix}/${name}`,
        name,
        children: nodes(child, `${prefix}/${name}`),
        implied: !child.kept,
      }))
      .sort((a, b) => a.name.localeCompare(b.name));
    const files = [...build.files].sort((a, b) => a.name.localeCompare(b.name));
    // Folders first: a tree where a file can hide the folder below it reads
    // as a flat list that happens to be indented.
    return [...folders, ...files];
  };
  return nodes(root, DOCS_ROOT);
}

/**
 * Every folder path in a tree, so a caller can expand all of them — which is
 * what a tree with two folders in it should do rather than making someone
 * click twice to see three files.
 */
export function documentTreeFolderPaths<T>(
  nodes: readonly DocumentTreeNode<T>[],
): string[] {
  const out: string[] = [];
  const walk = (list: readonly DocumentTreeNode<T>[]) => {
    for (const node of list) {
      if (node.kind === "folder") {
        out.push(node.path);
        walk(node.children);
      }
    }
  };
  walk(nodes);
  return out;
}

/** The folder paths that contain `path`, outermost first. */
export function documentAncestorFolders(path: string): string[] {
  const components = componentsUnderDocs(path);
  if (components === null) return [];
  const out: string[] = [];
  let prefix = DOCS_ROOT;
  for (const folder of components.slice(0, -1)) {
    prefix = `${prefix}/${folder}`;
    out.push(prefix);
  }
  return out;
}
