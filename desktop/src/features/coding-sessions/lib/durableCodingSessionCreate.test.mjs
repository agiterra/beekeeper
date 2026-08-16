import assert from "node:assert/strict";
import test from "node:test";
import { finalizeEvent, generateSecretKey } from "nostr-tools/pure";

import { KIND_CODING_SESSION_LIFECYCLE_COMMAND } from "@/shared/constants/kinds.ts";
import {
  durableCodingSessionCreateStorageKey,
  loadDurableCodingSessionCreate,
  prepareDurableCodingSessionCreate,
  publishDurableCodingSessionCreate,
} from "./durableCodingSessionCreate.ts";

const SECRET = generateSecretKey();
const PROVIDER_AUTHORITY = "a".repeat(64);
const SCOPE = "channel-a";

function input(overrides = {}) {
  return {
    channelId: "channel-a",
    commandId: "csl-command-a",
    projectRef: null,
    repoRef: null,
    providerInstanceRef: "0123456789abcdef",
    providerAuthorityPubkey: PROVIDER_AUTHORITY,
    model: "sonnet",
    title: null,
    initialTurn: "Inspect the repository.",
    ...overrides,
  };
}

function signer(eventInput) {
  return Promise.resolve(
    finalizeEvent({ ...eventInput, created_at: 1_800_000_000 }, SECRET),
  );
}

function memoryStorage() {
  const values = new Map();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => values.delete(key),
    values,
  };
}

test("prepare persists the exact signed command and provider authority before publish", async () => {
  const storage = memoryStorage();
  const prepared = await prepareDurableCodingSessionCreate(SCOPE, input(), {
    signer,
    storage,
    now: () => 123,
  });

  assert.equal(prepared.ok, true);
  assert.equal(
    prepared.transaction.input.providerAuthorityPubkey,
    PROVIDER_AUTHORITY,
  );
  const stored = JSON.parse(
    storage.values.get(durableCodingSessionCreateStorageKey(SCOPE)),
  );
  assert.equal(stored.publishState, "prepared");
  assert.equal(stored.input.commandId, "csl-command-a");
  assert.match(stored.event.content, /providerAuthorityPubkey/);
  assert.equal(
    loadDurableCodingSessionCreate(SCOPE, storage).transaction.event.id,
    prepared.transaction.event.id,
  );
});

test("only the native lifecycle kind is ever signed", async () => {
  const storage = memoryStorage();
  const prepared = await prepareDurableCodingSessionCreate(SCOPE, input(), {
    signer,
    storage,
  });

  assert.equal(prepared.transaction.event.kind, 44221);
  assert.equal(
    prepared.transaction.event.kind,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  );
  // The donor pre-signed a kind-9 twin so it could fall back to chat when a
  // relay rejected the native kind. We own the relay; there is nothing to fall
  // back to, and a second signed event would be a second publishable command.
  const serialized = storage.values.get(
    durableCodingSessionCreateStorageKey(SCOPE),
  );
  assert.doesNotMatch(serialized, /compatibilityEvent|"transport"/);
  assert.equal(serialized.match(/"sig":/g).length, 1);
});

test("a standalone create publishes an explicit null projectRef", async () => {
  const storage = memoryStorage();
  const prepared = await prepareDurableCodingSessionCreate(SCOPE, input(), {
    signer,
    storage,
  });

  const payload = JSON.parse(prepared.transaction.event.content);
  assert.equal(payload.action.projectRef, null);
  assert.ok(
    "projectRef" in payload.action,
    "a missing key is malformed, not a standalone session",
  );
});

test("an ambiguous retry reuses the exact signed event and command id", async () => {
  const storage = memoryStorage();
  const prepared = await prepareDurableCodingSessionCreate(SCOPE, input(), {
    signer,
    storage,
  });
  const publishedIds = [];
  let attempts = 0;
  const publisher = {
    async publishEvent(event) {
      publishedIds.push(event.id);
      attempts += 1;
      if (attempts === 1) throw new Error("socket closed after send");
      return event;
    },
  };

  const ambiguous = await publishDurableCodingSessionCreate(
    prepared.transaction,
    { publisher, storage },
  );
  assert.equal(ambiguous.accepted, false);
  assert.equal(ambiguous.transaction.publishState, "ambiguous");

  const accepted = await publishDurableCodingSessionCreate(
    loadDurableCodingSessionCreate(SCOPE, storage).transaction,
    { publisher, storage },
  );
  assert.equal(accepted.accepted, true);
  assert.deepEqual(publishedIds, [
    prepared.transaction.event.id,
    prepared.transaction.event.id,
  ]);
  assert.equal(
    accepted.transaction.input.commandId,
    prepared.transaction.input.commandId,
  );
});

test("restart loading rejects a payload that no longer matches its signed event", async () => {
  const storage = memoryStorage();
  await prepareDurableCodingSessionCreate(SCOPE, input(), { signer, storage });
  const key = durableCodingSessionCreateStorageKey(SCOPE);
  const stored = JSON.parse(storage.values.get(key));
  stored.input.providerAuthorityPubkey = "b".repeat(64);
  storage.values.set(key, JSON.stringify(stored));

  assert.deepEqual(loadDurableCodingSessionCreate(SCOPE, storage), {
    transaction: null,
    errorMessage:
      "The saved session request is invalid. Creation is blocked to prevent duplicate sessions.",
  });
});

test("a transaction from another scope is never adopted", async () => {
  const storage = memoryStorage();
  await prepareDurableCodingSessionCreate(SCOPE, input(), { signer, storage });
  const stored = storage.values.get(
    durableCodingSessionCreateStorageKey(SCOPE),
  );
  storage.values.set(durableCodingSessionCreateStorageKey("channel-b"), stored);

  assert.equal(
    loadDurableCodingSessionCreate("channel-b", storage).transaction,
    null,
  );
});

test("persistence failure blocks the first publish", async () => {
  const storage = {
    getItem: () => null,
    setItem() {
      throw new Error("quota");
    },
    removeItem() {},
  };

  const prepared = await prepareDurableCodingSessionCreate(SCOPE, input(), {
    signer,
    storage,
  });

  assert.deepEqual(prepared, {
    ok: false,
    errorMessage:
      "Durable session-request storage is unavailable. Nothing was published.",
  });
});

test("a signer that returns a different command fails local verification", async () => {
  const storage = memoryStorage();
  const tamperingSigner = (eventInput) =>
    signer({ ...eventInput, content: eventInput.content.replace("a", "z") });

  const prepared = await prepareDurableCodingSessionCreate(SCOPE, input(), {
    signer: tamperingSigner,
    storage,
  });

  assert.deepEqual(prepared, {
    ok: false,
    errorMessage: "The signed session request failed local verification.",
  });
  assert.equal(storage.values.size, 0, "nothing tampered-with is persisted");
});

test("a sessionRef-bearing create round-trips through storage byte-exactly", async () => {
  const storage = memoryStorage();
  const sessionRef = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
  const prepared = await prepareDurableCodingSessionCreate(
    SCOPE,
    input({ sessionRef }),
    { signer, storage },
  );

  assert.equal(prepared.ok, true);
  assert.equal(prepared.transaction.input.sessionRef, sessionRef);
  assert.match(
    prepared.transaction.event.content,
    /"sessionRef":"5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10"/,
  );

  const loaded = loadDurableCodingSessionCreate(SCOPE, storage);
  assert.equal(loaded.errorMessage, null);
  assert.deepEqual(loaded.transaction, prepared.transaction);
});

test("a genesisRef-bearing create round-trips through storage byte-exactly", async () => {
  const storage = memoryStorage();
  const sessionRef = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
  const genesisRef = "b".repeat(64);
  const prepared = await prepareDurableCodingSessionCreate(
    SCOPE,
    input({ sessionRef, genesisRef }),
    { signer, storage },
  );

  assert.equal(prepared.ok, true);
  assert.equal(prepared.transaction.input.genesisRef, genesisRef);
  assert.match(prepared.transaction.event.content, new RegExp(genesisRef));

  const loaded = loadDurableCodingSessionCreate(SCOPE, storage);
  assert.equal(loaded.errorMessage, null);
  assert.deepEqual(loaded.transaction, prepared.transaction);
});

test("a stored genesisRef without sessionRef is rejected", async () => {
  const storage = memoryStorage();
  const prepared = await prepareDurableCodingSessionCreate(SCOPE, input(), {
    signer,
    storage,
  });
  assert.equal(prepared.ok, true);

  const key = durableCodingSessionCreateStorageKey(SCOPE);
  const stored = JSON.parse(storage.values.get(key));
  stored.input.genesisRef = "b".repeat(64);
  storage.values.set(key, JSON.stringify(stored));

  assert.equal(
    loadDurableCodingSessionCreate(SCOPE, storage).transaction,
    null,
  );
});

test("a stored pre-sessionRef transaction stays valid and resumable forever", async () => {
  const storage = memoryStorage();
  // The historical 9-field input: prepared by a build that predates the
  // umbrella field. Its event bytes carry the 8-key action.
  const prepared = await prepareDurableCodingSessionCreate(SCOPE, input(), {
    signer,
    storage,
  });
  assert.equal(prepared.ok, true);
  assert.equal(
    prepared.transaction.event.content.includes("sessionRef"),
    false,
  );

  const loaded = loadDurableCodingSessionCreate(SCOPE, storage);
  assert.equal(loaded.errorMessage, null);
  assert.deepEqual(loaded.transaction, prepared.transaction);
});
