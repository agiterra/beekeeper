import assert from "node:assert/strict";
import test from "node:test";

import {
  clearNewCodingSessionDraft,
  MAX_NEW_CODING_SESSION_DRAFT_STORAGE_BYTES,
  newCodingSessionDraftStorageKey,
  readNewCodingSessionDraft,
  readNewCodingSessionDraftState,
  writeNewCodingSessionDraft,
} from "./newCodingSessionDraft.ts";

function memoryStorage() {
  const values = new Map();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => values.delete(key),
    values,
  };
}

test("new-session drafts persist locally per exact project and clear explicitly", () => {
  const storage = memoryStorage();
  writeNewCodingSessionDraft("project-a", "Inspect the repo.", storage);
  writeNewCodingSessionDraft("project-b", "Run the tests.", storage);
  assert.equal(
    readNewCodingSessionDraft("project-a", storage),
    "Inspect the repo.",
  );
  assert.equal(
    readNewCodingSessionDraft("project-b", storage),
    "Run the tests.",
  );
  clearNewCodingSessionDraft("project-a", storage);
  assert.equal(readNewCodingSessionDraft("project-a", storage), "");
  assert.equal(
    readNewCodingSessionDraft("project-b", storage),
    "Run the tests.",
  );
});

test("draft reader fails closed on stale project binding or malformed data", () => {
  const storage = memoryStorage();
  storage.setItem(
    newCodingSessionDraftStorageKey("project-a"),
    JSON.stringify({
      schema: "buzz-new-coding-session-draft/v1",
      scopeId: "project-b",
      text: "wrong project",
    }),
  );
  assert.equal(readNewCodingSessionDraft("project-a", storage), "");
  storage.setItem(newCodingSessionDraftStorageKey("project-a"), "{bad");
  assert.equal(readNewCodingSessionDraft("project-a", storage), "");
});

test("draft persistence preserves text above the send limit while retaining a larger storage bound", () => {
  const storage = memoryStorage();
  const overSendLimit = "a".repeat(12 * 1024 + 1);
  writeNewCodingSessionDraft("project-a", overSendLimit, storage);
  assert.equal(readNewCodingSessionDraft("project-a", storage), overSendLimit);

  const overStorageLimit = "a".repeat(
    MAX_NEW_CODING_SESSION_DRAFT_STORAGE_BYTES + 1,
  );
  assert.deepEqual(
    writeNewCodingSessionDraft("project-b", overStorageLimit, storage),
    {
      state: "over-cap",
      message:
        "This draft is too large for local persistence and will not survive an app restart.",
    },
  );
  assert.equal(readNewCodingSessionDraft("project-b", storage), "");
});

test("blocked or full localStorage reports that the in-memory draft can be lost", () => {
  const blocked = {
    getItem() {
      throw new Error("blocked");
    },
    setItem() {
      throw new Error("quota");
    },
    removeItem() {
      throw new Error("blocked");
    },
  };
  assert.deepEqual(
    writeNewCodingSessionDraft("project-a", "Keep editing.", blocked),
    {
      state: "unavailable",
      message:
        "Draft storage is unavailable. This text is only in memory and will not survive an app restart.",
    },
  );
  assert.equal(
    readNewCodingSessionDraftState("project-a", blocked).persistence.state,
    "unavailable",
  );
  assert.equal(
    clearNewCodingSessionDraft("project-a", blocked).state,
    "unavailable",
  );
  assert.equal(readNewCodingSessionDraft("project-a", blocked), "");
});
