import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionCommandEvent,
  buildCodingSessionInterruptEvent,
  buildCodingSessionTargetKey,
  codingSessionTargetSupportsInterrupt,
  MAX_CODING_SESSION_IDENTIFIER_BYTES,
  MAX_CODING_SESSION_TEXT_BYTES,
  publishCodingSessionCommand,
  publishCodingSessionInterrupt,
} from "./codingSessionCommand.ts";

const target = {
  driver: "provider-a",
  instanceId: "instance-1",
  sessionId: "session-1",
  generation: 2,
};

const textAtLimit = "é".repeat(MAX_CODING_SESSION_TEXT_BYTES / 2);
const textOverLimit = `${textAtLimit}a`;

function signerStub(collected) {
  return async (input) => {
    collected.push(input);
    return {
      id: `event-${input.kind}`,
      kind: input.kind,
      pubkey: "p",
      created_at: 1,
      tags: input.tags,
      content: input.content,
      sig: "s",
    };
  };
}

test("coding-session command content and tags are deterministic", () => {
  const event = buildCodingSessionCommandEvent({
    channelId: "channel-1",
    commandId: "cmd-1",
    target,
    text: "Steer this turn",
  });
  assert.equal(event.kind, 44220);
  assert.equal(
    event.content,
    '{"schema":"buzz-coding-session-command/v1","commandId":"cmd-1","target":{"driver":"provider-a","instanceId":"instance-1","sessionId":"session-1","generation":2},"action":{"type":"thread.turn.start","text":"Steer this turn"}}',
  );
  assert.deepEqual(event.tags, [
    ["h", "channel-1"],
    ["cs-v", "csc1-1"],
    ["cs-target", "coding-session/v1|10:provider-a10:instance-19:session-11:2"],
  ]);
});

test("target key is length-prefixed so field boundaries never collide", () => {
  assert.equal(
    buildCodingSessionTargetKey(target),
    "coding-session/v1|10:provider-a10:instance-19:session-11:2",
  );
  assert.notEqual(
    buildCodingSessionTargetKey({
      driver: "a",
      instanceId: "bc",
      sessionId: "d",
      generation: 1,
    }),
    buildCodingSessionTargetKey({
      driver: "ab",
      instanceId: "c",
      sessionId: "d",
      generation: 1,
    }),
  );
});

test("interrupt command is exact-generation fenced and carries no text", () => {
  const event = buildCodingSessionInterruptEvent({
    channelId: "channel-1",
    commandId: "interrupt-1",
    target,
  });
  assert.equal(
    event.content,
    '{"schema":"buzz-coding-session-command/v1","commandId":"interrupt-1","target":{"driver":"provider-a","instanceId":"instance-1","sessionId":"session-1","generation":2},"action":{"type":"thread.turn.interrupt"}}',
  );
  assert.deepEqual(event.tags, [
    ["h", "channel-1"],
    ["cs-v", "csc1-1"],
    ["cs-target", "coding-session/v1|10:provider-a10:instance-19:session-11:2"],
  ]);
  assert.equal(codingSessionTargetSupportsInterrupt(target), true);
  assert.equal(
    codingSessionTargetSupportsInterrupt({ ...target, generation: 0 }),
    false,
  );
});

test("turn text is bounded by UTF-8 bytes, not code units", () => {
  const atLimit = buildCodingSessionCommandEvent({
    channelId: "channel-1",
    commandId: "cmd-boundary",
    target,
    text: textAtLimit,
  });
  assert.equal(
    new TextEncoder().encode(JSON.parse(atLimit.content).action.text)
      .byteLength,
    MAX_CODING_SESSION_TEXT_BYTES,
  );
  assert.throws(
    () =>
      buildCodingSessionCommandEvent({
        channelId: "channel-1",
        commandId: "cmd-boundary",
        target,
        text: textOverLimit,
      }),
    /action\.text exceeds 12288 bytes/,
  );
});

test("identifiers use UTF-8 byte bounds and all required strings are trimmed-nonempty", () => {
  const identifierAtLimit = "é".repeat(MAX_CODING_SESSION_IDENTIFIER_BYTES / 2);
  assert.doesNotThrow(() =>
    buildCodingSessionCommandEvent({
      channelId: "channel-1",
      commandId: identifierAtLimit,
      target,
      text: "Steer",
    }),
  );
  assert.throws(
    () =>
      buildCodingSessionCommandEvent({
        channelId: "channel-1",
        commandId: `${identifierAtLimit}a`,
        target,
        text: "Steer",
      }),
    /commandId exceeds 256 bytes/,
  );

  for (const invalid of [
    { commandId: "   ", target, text: "Steer" },
    { commandId: "cmd", target: { ...target, driver: "\t" }, text: "Steer" },
    {
      commandId: "cmd",
      target: { ...target, instanceId: "\n" },
      text: "Steer",
    },
    {
      commandId: "cmd",
      target: { ...target, sessionId: " " },
      text: "Steer",
    },
    { commandId: "cmd", target, text: "\n\t" },
  ]) {
    assert.throws(() =>
      buildCodingSessionCommandEvent({ channelId: "channel-1", ...invalid }),
    );
  }
});

test("frontend rejects invalid content before invoking the signer", async () => {
  let signerCalls = 0;
  await assert.rejects(
    publishCodingSessionCommand(
      {
        channelId: "channel-1",
        commandId: "cmd-1",
        target,
        text: textOverLimit,
      },
      {
        signer: async () => {
          signerCalls += 1;
          throw new Error("signer must not run");
        },
      },
    ),
    /action\.text exceeds 12288 bytes/,
  );
  assert.equal(signerCalls, 0);
});

test("publish signs the native kind exactly once and reports the accepted identity", async () => {
  const signed = [];
  const accepted = [];
  const result = await publishCodingSessionCommand(
    { channelId: "channel-1", commandId: "cmd-1", target, text: "Steer" },
    {
      signer: signerStub(signed),
      publisher: {
        publishEvent: async (event) => {
          accepted.push(event);
          return event;
        },
      },
    },
  );
  assert.deepEqual(
    signed.map((event) => event.kind),
    [44220],
  );
  assert.equal(accepted.length, 1);
  // The command id comes back with the accepted identity: it is the only key
  // a provider refusal for this command will carry.
  assert.deepEqual(result, {
    eventId: "event-44220",
    kind: 44220,
    commandId: "cmd-1",
  });
});

test("interrupt publishes the same signed command contract", async () => {
  const signed = [];
  const result = await publishCodingSessionInterrupt(
    { channelId: "channel-1", commandId: "interrupt-1", target },
    {
      signer: async (input) => {
        signed.push(input);
        return {
          id: "interrupt-event",
          kind: input.kind,
          pubkey: "p",
          created_at: 1,
          tags: input.tags,
          content: input.content,
          sig: "s",
        };
      },
      publisher: {
        publishEvent: async (event) => event,
      },
    },
  );
  assert.equal(signed.length, 1);
  assert.equal(
    JSON.parse(signed[0].content).action.type,
    "thread.turn.interrupt",
  );
  assert.equal("text" in JSON.parse(signed[0].content).action, false);
  assert.deepEqual(result, {
    eventId: "interrupt-event",
    kind: 44220,
    commandId: "interrupt-1",
  });
});

test("native-only publish never re-signs under any relay rejection", async () => {
  for (const message of [
    "restricted: unknown event kind",
    "restricted: unsupported event kind",
    "restricted: not a channel member",
    "Timed out while sending the coding-session command.",
  ]) {
    const signed = [];
    await assert.rejects(
      publishCodingSessionCommand(
        { channelId: "channel-1", commandId: "cmd-1", target, text: "Steer" },
        {
          signer: signerStub(signed),
          publisher: {
            publishEvent: async () => {
              throw new Error(message);
            },
          },
        },
      ),
      new RegExp(message.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
    );
    assert.deepEqual(
      signed.map((event) => event.kind),
      [44220],
    );
  }
});
