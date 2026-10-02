import assert from "node:assert/strict";
import { test } from "node:test";

import {
  buildDocumentTree,
  documentAncestorFolders,
  documentTreeFolderPaths,
} from "@/features/agents-repo/lib/agentsRepoDocumentTree";

const row = (path) => ({ path });
const pathOf = (r) => r.path;

test("folders nest and sort before the files beside them", () => {
  const tree = buildDocumentTree(
    [
      row("docs/zebra.md"),
      row("docs/mockups/login.html"),
      row("docs/mockups/img/shot.png"),
      row("docs/alpha.md"),
      row("docs/notes/api.md"),
    ],
    pathOf,
  );
  assert.deepEqual(
    tree.map((n) => `${n.kind}:${n.name}`),
    ["folder:mockups", "folder:notes", "file:alpha.md", "file:zebra.md"],
  );
  const mockups = tree[0];
  assert.deepEqual(
    mockups.children.map((n) => `${n.kind}:${n.name}`),
    ["folder:img", "file:login.html"],
  );
  assert.equal(mockups.path, "docs/mockups");
  assert.equal(mockups.children[0].path, "docs/mockups/img");
});

test("a keep is the folder, never a row of its own", () => {
  const tree = buildDocumentTree([row("docs/empty/.gitkeep")], pathOf);
  assert.equal(tree.length, 1);
  assert.equal(tree[0].kind, "folder");
  assert.equal(tree[0].name, "empty");
  // The whole point of the keep: an empty folder someone created is a row.
  assert.deepEqual(tree[0].children, []);
  assert.equal(tree[0].implied, false, "a kept folder is not merely implied");
});

test("a folder that exists only because of its contents is implied", () => {
  const tree = buildDocumentTree([row("docs/mockups/login.html")], pathOf);
  assert.equal(tree[0].implied, true);

  const kept = buildDocumentTree(
    [row("docs/mockups/login.html"), row("docs/mockups/.gitkeep")],
    pathOf,
  );
  assert.equal(kept[0].implied, false);
  assert.deepEqual(
    kept[0].children.map((n) => n.name),
    ["login.html"],
    "the keep did not become a row beside the document",
  );
});

test("rows outside the documents tree are skipped, so the whole listing may be passed", () => {
  const tree = buildDocumentTree(
    [
      row("plans/CURRENT_STATE.md"),
      row("roles/lead.md"),
      row("team.yml"),
      row("docs/a.md"),
      row("docs"),
    ],
    pathOf,
  );
  assert.deepEqual(
    tree.map((n) => n.path),
    ["docs/a.md"],
  );
});

test("every folder path is collectable, for expanding a small tree by default", () => {
  const tree = buildDocumentTree(
    [row("docs/a/b/c.md"), row("docs/d/e.md")],
    pathOf,
  );
  assert.deepEqual(documentTreeFolderPaths(tree), [
    "docs/a",
    "docs/a/b",
    "docs/d",
  ]);
});

test("a path's ancestors are named outermost first, for revealing a selection", () => {
  assert.deepEqual(documentAncestorFolders("docs/a/b/c.md"), [
    "docs/a",
    "docs/a/b",
  ]);
  assert.deepEqual(documentAncestorFolders("docs/a.md"), []);
  assert.deepEqual(documentAncestorFolders("plans/a.md"), []);
});
