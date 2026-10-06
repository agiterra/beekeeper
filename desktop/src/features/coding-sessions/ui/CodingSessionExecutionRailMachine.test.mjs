import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionExecutionMachine,
  latestActivity,
} from "./CodingSessionExecutionRail.tsx";

const PROVIDER = "ab".repeat(32);
const execution = {
  signerPubkey: PROVIDER,
  activeGeneration: { providerAuthorityPubkey: PROVIDER },
};

test("a seat run by this computer's provider says so (SV-24)", () => {
  const machine = codingSessionExecutionMachine(execution, {
    localProviderPubkey: PROVIDER.toUpperCase(),
  });
  assert.equal(machine.label, "on this computer");
  assert.equal(machine.local, true);
});

test("another provider is named, or shown by key, never as this computer", () => {
  const named = codingSessionExecutionMachine(execution, {
    localProviderPubkey: "cd".repeat(32),
    resolveName: () => "Andy's provider",
  });
  assert.equal(named.label, "on Andy's provider");
  assert.equal(named.local, false);
  assert.match(named.title, /not on this computer/);
  const bare = codingSessionExecutionMachine(execution, {
    localProviderPubkey: null,
  });
  assert.equal(bare.label, `on provider ${PROVIDER.slice(0, 8)}`);
});

test("while this computer's key is unread, nothing claims to be here", () => {
  const unknown = codingSessionExecutionMachine(execution, {
    localProviderPubkey: undefined,
  });
  assert.equal(unknown.local, false);
  assert.match(unknown.title, /was not read/);
});

test("latest activity reads an assistant's markdown as plain words (SV-95)", () => {
  const transcript = [
    { type: "message", role: "user", text: "go" },
    {
      type: "message",
      role: "assistant",
      text: "**Failed commands:** only `python3 check.py`\n\n- one\n- two",
    },
  ];
  assert.equal(
    latestActivity(transcript),
    "Failed commands: only python3 check.py one two",
  );
});

test("latest activity is null for an assistant message of only markup", () => {
  assert.equal(
    latestActivity([{ type: "message", role: "assistant", text: "  \n\n " }]),
    null,
  );
});
