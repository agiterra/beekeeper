/**
 * The registry seam: readable when a reader is installed, honestly unreadable
 * when one is not, and never a copy compiled into the app.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  readModelRegistry,
  setModelRegistryReader,
} from "./codingSessionRegistrySource.ts";

test("with no reader installed the answer is unreadable, naming the file", async () => {
  setModelRegistryReader(null);
  const source = await readModelRegistry("30621:abc:beekeeper");
  assert.equal(source.kind, "unreadable");
  assert.match(source.why, /team\/model-registry\.yaml/);
});

test("with no project resolved it says that, not that the file is missing", async () => {
  setModelRegistryReader(null);
  for (const empty of [null, "   "]) {
    const source = await readModelRegistry(empty);
    assert.equal(source.kind, "unreadable");
    assert.match(source.why, /no project is resolved/);
  }
});

test("an installed reader is used, and is given the project coordinate", async () => {
  const asked = [];
  setModelRegistryReader(async (projectRef) => {
    asked.push(projectRef);
    return { kind: "readable", text: "version: 1\n", label: "fixture" };
  });
  try {
    const source = await readModelRegistry("30621:abc:beekeeper");
    assert.equal(source.kind, "readable");
    assert.deepEqual(asked, ["30621:abc:beekeeper"]);
  } finally {
    setModelRegistryReader(null);
  }
});

test("a reader that throws is unreadable with what it said, never an empty registry", async () => {
  setModelRegistryReader(async () => {
    throw new Error("no such file");
  });
  try {
    const source = await readModelRegistry("30621:abc:beekeeper");
    // An empty registry would route nothing while looking like a registry that
    // offers nothing, and those are different facts.
    assert.equal(source.kind, "unreadable");
    assert.match(source.why, /no such file/);
  } finally {
    setModelRegistryReader(null);
  }
});
