import assert from "node:assert/strict";
import { test } from "node:test";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { KIND_CODING_SESSION_NAME } from "../../../shared/constants/kinds.ts";
import {
  autoNameCodingSession,
  codingSessionAutoNameApplies,
  codingSessionAutoNameSentence,
} from "./codingSessionAutoName.ts";
import { buildCodingSessionNameEvent } from "./codingSessionName.ts";

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const SESSION_REF = "22222222-2222-4222-8222-222222222222";
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER = getPublicKey(FOUNDER_SECRET);
const MESSAGE = "Close ledger item 104 and record the finding.";
const ON = {
  provider: "anthropic",
  baseUrl: "",
  model: "claude-haiku-4-5",
  hasApiKey: true,
};

function signedName(content, createdAt = 1_700_000_000) {
  const built = buildCodingSessionNameEvent({
    channelId: CHANNEL_ID,
    content,
    sessionRef: SESSION_REF,
  });
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_NAME,
      created_at: createdAt,
      tags: built.tags,
      content: built.content,
    },
    FOUNDER_SECRET,
  );
}

function deps({
  settings = ON,
  generated = "Ledger item 104",
  names = [],
  publishFails = null,
} = {}) {
  const calls = [];
  return {
    calls,
    deps: {
      getSettings: async () => settings,
      generate: async (firstMessage) => {
        calls.push(["generate", firstMessage]);
        if (generated instanceof Error) throw generated;
        return generated;
      },
      readNames: async (channelId) => {
        calls.push(["readNames", channelId]);
        return names;
      },
      publishName: async (input) => {
        calls.push(["publish", input]);
        if (publishFails) throw publishFails;
        return {};
      },
    },
  };
}

const INPUT = {
  channelId: CHANNEL_ID,
  sessionRef: SESSION_REF,
  founderPubkey: FOUNDER,
  firstMessage: MESSAGE,
};

test("a blank-named session is named from its first message after Start", async () => {
  const d = deps();
  assert.deepEqual(await autoNameCodingSession(INPUT, d.deps), {
    kind: "published",
    name: "Ledger item 104",
  });
  assert.deepEqual(
    d.calls.map(([name]) => name),
    ["generate", "readNames", "publish"],
    "the wire is read after the model answers, right before publishing",
  );
  assert.deepEqual(d.calls[2][1], {
    channelId: CHANNEL_ID,
    content: "Ledger item 104",
    sessionRef: SESSION_REF,
  });
});

test("no namer configured, or no host to ask, names nothing", async () => {
  for (const settings of [null, { ...ON, provider: "off" }]) {
    const d = deps({ settings });
    assert.deepEqual(await autoNameCodingSession(INPUT, d.deps), {
      kind: "off",
    });
    assert.deepEqual(d.calls, []);
  }
});

test("a message too short to name is not sent to the model", async () => {
  const d = deps();
  assert.deepEqual(
    await autoNameCodingSession({ ...INPUT, firstMessage: "hi there" }, d.deps),
    { kind: "too-short" },
  );
  assert.deepEqual(d.calls, []);
});

test("a name that landed while the model was thinking is kept; nothing is published over it", async () => {
  const d = deps({ names: [signedName("Typed on the phone")] });
  assert.deepEqual(await autoNameCodingSession(INPUT, d.deps), {
    kind: "named-meanwhile",
    name: "Typed on the phone",
  });
  assert.equal(
    d.calls.some(([name]) => name === "publish"),
    false,
  );
});

test("an empty answer, a model refusal and a relay refusal are stated, never invented", async () => {
  assert.deepEqual(
    await autoNameCodingSession(INPUT, deps({ generated: "  " }).deps),
    {
      kind: "empty",
    },
  );
  assert.deepEqual(
    await autoNameCodingSession(
      INPUT,
      deps({ generated: new Error("401 from the namer") }).deps,
    ),
    { kind: "failed", reason: "401 from the namer" },
  );
  assert.deepEqual(
    await autoNameCodingSession(
      INPUT,
      deps({ publishFails: new Error("relay: blocked") }).deps,
    ),
    { kind: "failed", reason: "relay: blocked" },
  );
});

test("the Name field's sentence says what a blank field means, and nothing when the host is unknown", () => {
  assert.equal(codingSessionAutoNameSentence(null), null);
  assert.match(
    codingSessionAutoNameSentence({ ...ON, provider: "off" }),
    /no naming model is set/,
  );
  assert.equal(
    codingSessionAutoNameSentence(ON),
    "Left blank, it is named from the initial prompt after Start (claude-haiku-4-5).",
  );
  assert.equal(
    codingSessionAutoNameSentence({ ...ON, model: "" }),
    "Left blank, it is named from the initial prompt after Start.",
  );
  assert.equal(
    codingSessionAutoNameApplies({ settings: ON, firstMessage: MESSAGE }),
    true,
  );
  assert.equal(
    codingSessionAutoNameApplies({ settings: ON, firstMessage: "short" }),
    false,
  );
  assert.equal(
    codingSessionAutoNameApplies({ settings: null, firstMessage: MESSAGE }),
    false,
  );
});
