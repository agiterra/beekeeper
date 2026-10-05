import assert from "node:assert/strict";
import test from "node:test";

import { codingSessionVisibleTreeRows } from "./CodingSessionFilesPanelTree.ts";

const root = {
  kind: "ok",
  truncated: false,
  entries: [
    { name: "README.md", relPath: "README.md", kind: "file" },
    { name: "src", relPath: "src", kind: "directory" },
    { name: "link", relPath: "link", kind: "symlink" },
  ],
};
const src = {
  kind: "ok",
  truncated: true,
  entries: [{ name: "relay.ts", relPath: "src/relay.ts", kind: "file" }],
};

function labels(rows) {
  return rows.map((row) =>
    row.kind === "entry"
      ? `${row.depth}:${row.entry.relPath}`
      : `${row.depth}:${row.text}`,
  );
}

test("folders first, collapsed until opened, relative paths only", () => {
  const rows = codingSessionVisibleTreeRows({
    listings: new Map([["", root]]),
    expanded: new Set(),
    filter: "",
  });
  assert.deepEqual(labels(rows), ["0:src", "0:link", "0:README.md"]);
});

test("an opened folder lists beneath it, and says when the host stopped at its cap", () => {
  const rows = codingSessionVisibleTreeRows({
    listings: new Map([
      ["", root],
      ["src", src],
    ]),
    expanded: new Set(["src"]),
    filter: "",
  });
  assert.deepEqual(labels(rows), [
    "0:src",
    "1:src/relay.ts",
    "1:This folder holds more than 2,000 entries; the first 2,000 are listed.",
    "0:link",
    "0:README.md",
  ]);
});

test("an opened folder not yet listed says so, and a failed one says why", () => {
  const loading = codingSessionVisibleTreeRows({
    listings: new Map([["", root]]),
    expanded: new Set(["src"]),
    filter: "",
  });
  assert.deepEqual(labels(loading).slice(0, 2), ["0:src", "1:Loading…"]);
  const failed = codingSessionVisibleTreeRows({
    listings: new Map([
      ["", root],
      ["src", { kind: "error", message: "refused" }],
    ]),
    expanded: new Set(["src"]),
    filter: "",
  });
  assert.equal(labels(failed)[1], "1:Could not list this folder: refused");
});

test("the filter searches only what was loaded, with the path that leads to it", () => {
  const rows = codingSessionVisibleTreeRows({
    listings: new Map([
      ["", root],
      ["src", src],
    ]),
    expanded: new Set(),
    filter: "RELAY",
  });
  assert.deepEqual(labels(rows), ["0:src", "1:src/relay.ts"]);
  const none = codingSessionVisibleTreeRows({
    listings: new Map([["", root]]),
    expanded: new Set(),
    filter: "relay",
  });
  assert.deepEqual(none, []);
});

test("a folder the host listed incompletely says how many entries it left out", () => {
  const rows = codingSessionVisibleTreeRows({
    listings: new Map([
      ["", { ...root, omitted: 3 }],
      ["src", { ...src, truncated: false, omitted: 1 }],
    ]),
    expanded: new Set(["src"]),
    filter: "",
  });
  assert.deepEqual(labels(rows), [
    "0:src",
    "1:src/relay.ts",
    "1:1 entry in this folder could not be listed.",
    "0:link",
    "0:README.md",
    "0:3 entries in this folder could not be listed.",
  ]);
  const complete = codingSessionVisibleTreeRows({
    listings: new Map([["", { ...root, omitted: 0 }]]),
    expanded: new Set(),
    filter: "",
  });
  assert.equal(
    labels(complete).some((label) => label.includes("could not be listed")),
    false,
  );
});
