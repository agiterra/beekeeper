import assert from "node:assert/strict";
import { test } from "node:test";

import {
  readModelRegistry,
  readModelRegistryRows,
} from "./codingSessionRegistrySource.ts";

/** A stub `read_project_file` that answers from a plain object. */
function stubReader(files) {
  return async (projectRef, relativePath) => {
    const key = `${projectRef}|${relativePath}`;
    const hit = files[key];
    if (hit === undefined) {
      // What `readProjectFile` really rejects with: the host's own refusal.
      throw {
        code: "file-missing",
        message: `/checkout/${relativePath} cannot be opened: No such file or directory`,
      };
    }
    return { path: `/checkout/${relativePath}`, text: hit };
  };
}

const REGISTRY_YAML = `version: 1
updatedAt: "2026-08-30"
traits: [reasoning, coding, taste, judgment, agency, discipline, context, verification, velocity, cost_efficiency]
tiers:
  fast: { risk: [1, 8], effort: low }
  standard: { risk: [9, 39], effort: medium }
  deep: { risk: [40, 125], effort: high }
classes:
  builder:
    minimums: { coding: 4 }
targets:
  - provider: codex-primary
    model: gpt-5.6-luna
    scores: { coding: 4.1 }
    facts: {}
    status: { builder: incumbent-fast }
    rating: { status: operational_opinion, confidence: low, author: brian, date: "2026-08-30" }
  - provider: claude-primary
    model: sonnet
    scores: { coding: 4.5 }
    facts: {}
    status: { builder: incumbent-standard }
    rating: { status: operational_opinion, confidence: low, author: brian, date: "2026-08-30" }
`;

test("reads the registry out of the project the caller named", async () => {
  const source = await readModelRegistry("30621:abc:beekeeper", {
    read: stubReader({
      "30621:abc:beekeeper|team/model-registry.yaml": REGISTRY_YAML,
    }),
  });
  assert.equal(source.kind, "readable");
  assert.equal(source.text, REGISTRY_YAML);
  assert.equal(source.label, "/checkout/team/model-registry.yaml");
});

test("a missing registry file is unreadable, with the host's own sentence", async () => {
  const source = await readModelRegistry("30621:abc:beekeeper", {
    read: stubReader({}),
  });
  assert.equal(source.kind, "unreadable");
  assert.match(source.why, /team\/model-registry\.yaml/);
  assert.match(source.why, /No such file or directory/);
});

test("no project at all is unreadable, and says which project is missing", async () => {
  const source = await readModelRegistry(null, {
    read: stubReader({
      "30621:abc:beekeeper|team/model-registry.yaml": REGISTRY_YAML,
    }),
  });
  assert.equal(source.kind, "unreadable");
  assert.match(source.why, /no project/i);
});

test("a reader that throws a bare error still produces a sentence", async () => {
  const source = await readModelRegistry("30621:abc:beekeeper", {
    read: async () => {
      throw new Error("the webview is not running in Tauri");
    },
  });
  assert.equal(source.kind, "unreadable");
  assert.match(source.why, /not running in Tauri/);
});

test("rows for the badge come from the real file, with its version", async () => {
  const rows = await readModelRegistryRows("30621:abc:beekeeper", {
    read: stubReader({
      "30621:abc:beekeeper|team/model-registry.yaml": REGISTRY_YAML,
    }),
  });
  assert.equal(rows.kind, "read");
  assert.equal(rows.version, 1);
  // `measured` rides on every row so the coverage badge can ever say a row
  // was measured; both of these are opinions, and say so.
  assert.deepEqual(rows.rows, [
    { provider: "codex-primary", model: "gpt-5.6-luna", measured: false },
    { provider: "claude-primary", model: "sonnet", measured: false },
  ]);
});

test("a file that is not a registry is unreadable, naming the parse failure", async () => {
  const rows = await readModelRegistryRows("30621:abc:beekeeper", {
    read: stubReader({
      "30621:abc:beekeeper|team/model-registry.yaml": "version: 99\n",
    }),
  });
  assert.equal(rows.kind, "unreadable");
  assert.match(rows.reason, /version 99/);
});

test("a blank project coordinate is the no-project answer, not a missing file", async () => {
  const source = await readModelRegistry("   ", {
    read: stubReader({
      "30621:abc:beekeeper|team/model-registry.yaml": REGISTRY_YAML,
    }),
  });
  assert.equal(source.kind, "unreadable");
  assert.match(source.why, /no project/i);
});

test("the reader answers in the shape the router consumes", async () => {
  // The 2026-08-30 drop was two near-identical shapes on either side of a
  // seam. `readModelRegistry` returns `CodingSessionRegistrySource` itself, so
  // the keys the router reads are the keys this produces.
  const source = await readModelRegistry("30621:abc:beekeeper", {
    read: stubReader({
      "30621:abc:beekeeper|team/model-registry.yaml": REGISTRY_YAML,
    }),
  });
  assert.deepEqual(Object.keys(source).sort(), ["kind", "label", "text"]);
});

test("F4: every mapped row carries a measured flag, and it follows the block", async () => {
  // The producer used to map { provider, model } and nothing else, so the
  // coverage badge's `unmeasured` count was a constant: every row read
  // unmeasured forever, including after one was genuinely measured. Right
  // today by accident is the same class of defect as a badge pointing at a
  // message you cannot find.
  const { readFileSync } = await import("node:fs");
  const { fileURLToPath } = await import("node:url");
  const text = readFileSync(
    fileURLToPath(
      new URL("../../../../../team/model-registry.yaml", import.meta.url),
    ),
    "utf8",
  );

  const rowsOf = async (source) =>
    readModelRegistryRows("proj", {
      read: async () => ({ text: source, path: "team/model-registry.yaml" }),
    });

  const shipped = await rowsOf(text);
  assert.equal(shipped.kind, "read");
  assert.equal(shipped.rows.length, 11);
  assert.ok(
    shipped.rows.every((row) => row.measured === false),
    "every shipped row is still an opinion",
  );

  const anchor = "  - provider: claude-primary\n    model: opus[1m]\n";
  const block = [
    "    measured:",
    "      role: verifier",
    "      benchVersion: 2",
    '      benchHash: "abc"',
    '      measuredAt: "2026-09-02"',
    '      measuredBy: "aa"',
    '      runs: ["e1"]',
    "      traits:",
    "        reasoning: { score: 4.6, n: 3, min: 4.4, max: 4.8 }",
    "",
  ].join("\n");
  const after = await rowsOf(text.replace(anchor, `${anchor}${block}`));
  const row = after.rows.find((entry) => entry.model === "opus[1m]");
  assert.equal(row.measured, true, "the flag must follow the block");
  assert.equal(after.rows.filter((entry) => entry.measured).length, 1);
});
