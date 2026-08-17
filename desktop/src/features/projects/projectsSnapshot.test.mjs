import assert from "node:assert/strict";
import test from "node:test";

import {
  projectsSnapshotKey,
  readProjectsSnapshot,
  writeProjectsSnapshot,
} from "./projectsSnapshot.ts";

function withMemoryLocalStorage(run) {
  const store = new Map();
  const original = globalThis.localStorage;
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key) => store.get(key) ?? null,
      setItem: (key, value) => store.set(key, String(value)),
      removeItem: (key) => store.delete(key),
    },
  });
  try {
    return run(store);
  } finally {
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: original,
    });
  }
}

test("the key requires both relay and viewer identity", () => {
  assert.equal(projectsSnapshotKey(undefined, "abc"), undefined);
  assert.equal(
    projectsSnapshotKey("wss://relay.example", undefined),
    undefined,
  );
  assert.equal(
    projectsSnapshotKey("wss://relay.example", "abc"),
    "buzz.projects.repos.v1:wss://relay.example:abc",
  );
});

test("snapshots round-trip and carry a sweepable updatedAt", () => {
  withMemoryLocalStorage((store) => {
    const key = projectsSnapshotKey("wss://relay.example", "abc");
    const projects = [{ id: "p1", name: "Buzz" }];
    writeProjectsSnapshot(key, projects);
    const raw = JSON.parse(store.get(key));
    assert.equal(typeof raw.updatedAt, "number");
    assert.deepEqual(readProjectsSnapshot(key), projects);
  });
});

test("an undefined key reads and writes as a no-op", () => {
  withMemoryLocalStorage(() => {
    writeProjectsSnapshot(undefined, [{ id: "p1" }]);
    assert.equal(readProjectsSnapshot(undefined), undefined);
  });
});

test("malformed payloads read as no snapshot", () => {
  withMemoryLocalStorage((store) => {
    const key = projectsSnapshotKey("wss://relay.example", "abc");
    for (const bad of ["not json", '"string"', "[]", '{"projects": 42}']) {
      store.set(key, bad);
      assert.equal(readProjectsSnapshot(key), undefined, bad);
    }
  });
});
