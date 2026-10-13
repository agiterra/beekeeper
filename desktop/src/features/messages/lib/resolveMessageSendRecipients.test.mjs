import assert from "node:assert/strict";
import test from "node:test";
import { resolveMessageSendRecipients } from "./resolveMessageSendRecipients.ts";

const input = (overrides = {}) => ({
  channelType: "dm",
  senderPubkey: "OWNER",
  explicitMentions: [],
  resolveMembership: async () => ({
    kind: "resolved",
    memberPubkeys: new Set(["owner", "AGENT"]),
  }),
  ...overrides,
});

test("plain DM waits for confirmed membership and prepares its semantic recipient", async () => {
  let resolve;
  let finished = false;
  const pending = resolveMessageSendRecipients(
    input({
      resolveMembership: () =>
        new Promise((done) => {
          resolve = done;
        }),
    }),
  ).then((result) => {
    finished = true;
    return result;
  });
  await Promise.resolve();
  assert.equal(finished, false);
  resolve({ kind: "resolved", memberPubkeys: new Set(["owner", "AGENT"]) });
  assert.deepEqual(await pending, ["agent"]);
});

test("DM explicit mentions dedupe with recipients; sender and empty keys never wake", async () => {
  assert.deepEqual(
    await resolveMessageSendRecipients(
      input({ explicitMentions: ["OWNER", "agent", "AGENT", ""] }),
    ),
    ["agent"],
  );
});

test("unknown membership, missing sender and unconfirmed sender refuse preparation", async () => {
  await assert.rejects(
    resolveMessageSendRecipients(
      input({
        resolveMembership: async () => ({ kind: "unknown", reason: "network" }),
      }),
    ),
    /not been sent/,
  );
  await assert.rejects(
    resolveMessageSendRecipients(input({ senderPubkey: null })),
    /No identity/,
  );
  await assert.rejects(
    resolveMessageSendRecipients(input({ senderPubkey: "outsider" })),
    /membership.*not been sent/,
  );
});

test("stream/plain messages never infer members or request their membership", async () => {
  const resolveMembership = async () => {
    throw new Error("stream must not read implicit recipients");
  };
  assert.deepEqual(
    await resolveMessageSendRecipients(
      input({ channelType: "stream", resolveMembership }),
    ),
    [],
  );
  assert.deepEqual(
    await resolveMessageSendRecipients(
      input({
        channelType: "stream",
        explicitMentions: ["agent"],
        resolveMembership,
      }),
    ),
    ["agent"],
  );
});
