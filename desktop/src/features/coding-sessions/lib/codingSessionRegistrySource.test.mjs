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
  assert.equal(source.path, "/checkout/team/model-registry.yaml");
});

test("a missing registry file is unreadable, with the host's own sentence", async () => {
  const source = await readModelRegistry("30621:abc:beekeeper", {
    read: stubReader({}),
  });
  assert.equal(source.kind, "unreadable");
  assert.match(source.reason, /team\/model-registry\.yaml/);
  assert.match(source.reason, /No such file or directory/);
});

test("no project at all is unreadable, and says which project is missing", async () => {
  const source = await readModelRegistry(null, {
    read: stubReader({
      "30621:abc:beekeeper|team/model-registry.yaml": REGISTRY_YAML,
    }),
  });
  assert.equal(source.kind, "unreadable");
  assert.match(source.reason, /no project/i);
});

test("a reader that throws a bare error still produces a sentence", async () => {
  const source = await readModelRegistry("30621:abc:beekeeper", {
    read: async () => {
      throw new Error("the webview is not running in Tauri");
    },
  });
  assert.equal(source.kind, "unreadable");
  assert.match(source.reason, /not running in Tauri/);
});

test("rows for the badge come from the real file, with its version", async () => {
  const rows = await readModelRegistryRows("30621:abc:beekeeper", {
    read: stubReader({
      "30621:abc:beekeeper|team/model-registry.yaml": REGISTRY_YAML,
    }),
  });
  assert.equal(rows.kind, "read");
  assert.equal(rows.version, 1);
  assert.deepEqual(rows.rows, [
    { provider: "codex-primary", model: "gpt-5.6-luna" },
    { provider: "claude-primary", model: "sonnet" },
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
