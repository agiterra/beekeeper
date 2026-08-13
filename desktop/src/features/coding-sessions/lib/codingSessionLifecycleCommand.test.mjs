import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionCreateEvent,
  createCodingSessionSessionRef,
  MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES,
  MAX_CODING_SESSION_LIFECYCLE_REFERENCE_BYTES,
  publishCodingSessionCreate,
} from "./codingSessionLifecycleCommand.ts";

const input = {
  channelId: "channel-1",
  commandId: "create-1",
  projectRef: "30621:owner:amas-redux",
  repoRef: "30617:owner:amas-redux",
  providerInstanceRef: "claude-primary",
  providerAuthorityPubkey: "ab".repeat(32),
  model: "claude-sonnet-4-6",
  title: "Advance Buzz live sessions",
  initialTurn: "Inspect the project and start with the highest priority task.",
};

test("session.create content and tags are deterministic and carry no host authority", () => {
  const event = buildCodingSessionCreateEvent(input);
  assert.equal(event.kind, 44221);
  assert.equal(
    event.content,
    `{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{"type":"session.create","projectRef":"30621:owner:amas-redux","repoRef":"30617:owner:amas-redux","providerInstanceRef":"claude-primary","providerAuthorityPubkey":"${"ab".repeat(32)}","model":"claude-sonnet-4-6","title":"Advance Buzz live sessions","initialTurn":"Inspect the project and start with the highest priority task."}}`,
  );
  assert.deepEqual(event.tags, [
    ["h", "channel-1"],
    ["csl-v", "csl1-1"],
    ["csl-command", "create-1"],
  ]);
  const content = JSON.parse(event.content);
  assert.equal("cwd" in content.action, false);
  assert.equal("env" in content.action, false);
  assert.equal("secrets" in content.action, false);
  assert.equal("generation" in content.action, false);
});

test("standalone sessions serialize an explicit null projectRef, never a missing key", () => {
  const event = buildCodingSessionCreateEvent({ ...input, projectRef: null });
  const action = JSON.parse(event.content).action;
  assert.equal("projectRef" in action, true);
  assert.equal(action.projectRef, null);
  assert.match(event.content, /"projectRef":null/);
});

test("provider instance is required while optional fields stay explicit", () => {
  const event = buildCodingSessionCreateEvent({
    ...input,
    repoRef: null,
    model: null,
    title: null,
    initialTurn: null,
  });
  assert.deepEqual(JSON.parse(event.content).action, {
    type: "session.create",
    projectRef: input.projectRef,
    repoRef: null,
    providerInstanceRef: input.providerInstanceRef,
    providerAuthorityPubkey: input.providerAuthorityPubkey,
    model: null,
    title: null,
    initialTurn: null,
  });
  for (const invalid of [
    { projectRef: " " },
    { providerInstanceRef: "\n" },
    { providerAuthorityPubkey: "AB".repeat(32) },
    { providerAuthorityPubkey: "ab".repeat(31) },
    { repoRef: "\t" },
    { model: "" },
    { title: "   " },
    { initialTurn: "\n" },
  ]) {
    assert.throws(() =>
      buildCodingSessionCreateEvent({ ...input, ...invalid }),
    );
  }
});

test("a sessionRef serializes in canonical position between repoRef and providerInstanceRef", () => {
  const sessionRef = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
  const event = buildCodingSessionCreateEvent({ ...input, sessionRef });
  assert.deepEqual(Object.keys(JSON.parse(event.content).action), [
    "type",
    "projectRef",
    "repoRef",
    "sessionRef",
    "providerInstanceRef",
    "providerAuthorityPubkey",
    "model",
    "title",
    "initialTurn",
  ]);
  assert.equal(JSON.parse(event.content).action.sessionRef, sessionRef);
  // Tag shape is untouched: umbrella and non-umbrella creates produce
  // identically shaped envelopes.
  assert.deepEqual(event.tags, [
    ["h", "channel-1"],
    ["csl-v", "csl1-1"],
    ["csl-command", "create-1"],
  ]);
});

test("an explicit null sessionRef serializes; an absent key reproduces the 8-key historical form", () => {
  const withNull = buildCodingSessionCreateEvent({
    ...input,
    sessionRef: null,
  });
  assert.match(withNull.content, /"sessionRef":null/);

  const historical = buildCodingSessionCreateEvent(input);
  const roundTripped = buildCodingSessionCreateEvent({
    ...input,
    sessionRef: undefined,
  });
  assert.equal("sessionRef" in JSON.parse(historical.content).action, false);
  // `undefined` counts as absent so a JSON round-trip (which drops the key)
  // rebuilds the same bytes.
  assert.equal(roundTripped.content, historical.content);
});

test("a sessionRef must be a canonical lowercase hyphenated UUID", () => {
  for (const invalid of [
    "5B7E1C2A-90D4-4B0E-A1F3-7C2D8E6F4A10",
    "5b7e1c2a90d44b0ea1f37c2d8e6f4a10",
    "not-a-uuid",
    "",
    " ",
    `5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10 `,
  ]) {
    assert.throws(
      () => buildCodingSessionCreateEvent({ ...input, sessionRef: invalid }),
      /action\.sessionRef/,
    );
  }
});

test("every minted sessionRef is accepted by the builder", () => {
  for (let round = 0; round < 16; round += 1) {
    const sessionRef = createCodingSessionSessionRef();
    assert.match(
      sessionRef,
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/,
    );
    assert.doesNotThrow(() =>
      buildCodingSessionCreateEvent({ ...input, sessionRef }),
    );
  }
  // Every create mints its own umbrella identity.
  assert.notEqual(
    createCodingSessionSessionRef(),
    createCodingSessionSessionRef(),
  );
});

test("references and initial turn use UTF-8 byte limits", () => {
  const referenceAtLimit = "é".repeat(
    MAX_CODING_SESSION_LIFECYCLE_REFERENCE_BYTES / 2,
  );
  const turnAtLimit = "é".repeat(
    MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES / 2,
  );
  assert.doesNotThrow(() =>
    buildCodingSessionCreateEvent({
      ...input,
      projectRef: referenceAtLimit,
      initialTurn: turnAtLimit,
    }),
  );
  assert.throws(
    () =>
      buildCodingSessionCreateEvent({
        ...input,
        projectRef: `${referenceAtLimit}a`,
      }),
    /action\.projectRef exceeds 2048 bytes/,
  );
  assert.throws(
    () =>
      buildCodingSessionCreateEvent({
        ...input,
        initialTurn: `${turnAtLimit}a`,
      }),
    /action\.initialTurn exceeds 12288 bytes/,
  );
});

test("publish signs the native lifecycle kind exactly once", async () => {
  const signed = [];
  const accepted = [];
  const result = await publishCodingSessionCreate(input, {
    signer: async (event) => {
      signed.push(event);
      return {
        id: `event-${event.kind}`,
        kind: event.kind,
        pubkey: "p",
        created_at: 1,
        tags: event.tags,
        content: event.content,
        sig: "s",
      };
    },
    publisher: {
      publishEvent: async (event) => {
        accepted.push(event);
        return event;
      },
    },
  });
  assert.deepEqual(
    signed.map((event) => event.kind),
    [44221],
  );
  assert.equal(accepted.length, 1);
  assert.deepEqual(result, { eventId: "event-44221", kind: 44221 });
});

test("every relay rejection fails the create without a second signature", async () => {
  for (const message of [
    "restricted: unsupported event kind",
    "restricted: not a channel member",
  ]) {
    let signerCalls = 0;
    await assert.rejects(
      publishCodingSessionCreate(input, {
        signer: async (event) => {
          signerCalls += 1;
          return {
            id: "event-44221",
            kind: event.kind,
            pubkey: "p",
            created_at: 1,
            tags: event.tags,
            content: event.content,
            sig: "s",
          };
        },
        publisher: {
          publishEvent: async () => {
            throw new Error(message);
          },
        },
      }),
    );
    assert.equal(signerCalls, 1);
  }
});
