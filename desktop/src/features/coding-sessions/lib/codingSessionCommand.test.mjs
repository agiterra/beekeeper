import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionCommandEvent,
  CODING_SESSION_TURN_DELIVERIES,
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
    deliver: "boundary",
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
    deliver: "boundary",
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
        deliver: "boundary",
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
      deliver: "boundary",
    }),
  );
  assert.throws(
    () =>
      buildCodingSessionCommandEvent({
        channelId: "channel-1",
        commandId: `${identifierAtLimit}a`,
        target,
        text: "Steer",
        deliver: "boundary",
      }),
    /commandId exceeds 256 bytes/,
  );

  for (const invalid of [
    { commandId: "   ", target, text: "Steer", deliver: "boundary" },
    {
      commandId: "cmd",
      target: { ...target, driver: "\t" },
      text: "Steer",
      deliver: "boundary",
    },
    {
      commandId: "cmd",
      target: { ...target, instanceId: "\n" },
      text: "Steer",
      deliver: "boundary",
    },
    {
      commandId: "cmd",
      target: { ...target, sessionId: " " },
      text: "Steer",
      deliver: "boundary",
    },
    { commandId: "cmd", target, text: "\n\t", deliver: "boundary" },
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
        deliver: "boundary",
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
    {
      channelId: "channel-1",
      commandId: "cmd-1",
      target,
      text: "Steer",
      deliver: "boundary",
    },
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
        {
          channelId: "channel-1",
          commandId: "cmd-1",
          target,
          text: "Steer",
          deliver: "boundary",
        },
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

test("the default delivery class is omitted from the wire", () => {
  const event = buildCodingSessionCommandEvent({
    channelId: "channel-1",
    commandId: "cmd-deliver",
    target,
    text: "do the thing",
    deliver: "boundary",
  });
  // Pinned as the exact bytes a relay that predates `deliver` must accept:
  // that relay decodes the payload with deny_unknown_fields, so a spelled-out
  // default would be refused where absent already means boundary.
  assert.equal(
    event.content,
    '{"schema":"buzz-coding-session-command/v1","commandId":"cmd-deliver","target":{"driver":"provider-a","instanceId":"instance-1","sessionId":"session-1","generation":2},"action":{"type":"thread.turn.start","text":"do the thing"}}',
  );
  const action = JSON.parse(event.content).action;
  assert.deepEqual(Object.keys(action), ["type", "text"]);
  assert.equal("deliver" in action, false);
});

test("an escalated delivery class is on the wire, explicit, and closed", () => {
  assert.deepEqual(
    [...CODING_SESSION_TURN_DELIVERIES],
    ["boundary", "steer", "interrupt"],
  );
  for (const deliver of ["steer", "interrupt"]) {
    const event = buildCodingSessionCommandEvent({
      channelId: "channel-1",
      commandId: "cmd-deliver",
      target,
      text: "do the thing",
      deliver,
    });
    const action = JSON.parse(event.content).action;
    assert.deepEqual(Object.keys(action), ["type", "text", "deliver"]);
    assert.equal(action.deliver, deliver);
  }
});

test("an unknown or absent delivery class is refused before signing", () => {
  for (const deliver of ["queue", "BOUNDARY", "", undefined, null, 1]) {
    assert.throws(
      () =>
        buildCodingSessionCommandEvent({
          channelId: "channel-1",
          commandId: "cmd-deliver",
          target,
          text: "do the thing",
          deliver,
        }),
      /action\.deliver/,
      `deliver=${String(deliver)} must not reach the signer`,
    );
  }
});

test("an interrupt command carries no delivery class of its own", () => {
  const event = buildCodingSessionInterruptEvent({
    channelId: "channel-1",
    commandId: "interrupt-1",
    target,
  });
  assert.deepEqual(Object.keys(JSON.parse(event.content).action), ["type"]);
});

const attachment = {
  sha256: "aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd",
  mime: "image/png",
  size: 2048,
  dim: "800x600",
  filename: "shot.png",
};

test("a turn with no images is byte-identical to the pre-attachment wire", () => {
  // The whole forward-compatibility contract: the payload is validated with
  // `deny_unknown_fields` at the relay *and* the provider, so an ordinary turn
  // must not gain a key. Both the absent and the empty-array spellings collapse
  // to the same bytes.
  const baseline = buildCodingSessionCommandEvent({
    channelId: "3f2b8c1e-0000-4000-8000-000000000001",
    commandId: "cmd-1",
    target,
    text: "go",
    deliver: "boundary",
  });
  const withEmptyList = buildCodingSessionCommandEvent({
    channelId: "3f2b8c1e-0000-4000-8000-000000000001",
    commandId: "cmd-1",
    target,
    text: "go",
    attachments: [],
    deliver: "boundary",
  });

  assert.doesNotMatch(baseline.content, /attachments/);
  assert.equal(withEmptyList.content, baseline.content);
});

test("attachments ride the payload and are validated before signing", () => {
  const event = buildCodingSessionCommandEvent({
    channelId: "3f2b8c1e-0000-4000-8000-000000000001",
    commandId: "cmd-2",
    target,
    text: "why is this chart wrong?",
    attachments: [attachment],
    deliver: "boundary",
  });
  const payload = JSON.parse(event.content);
  assert.deepEqual(payload.action.attachments, [attachment]);
  // The envelope is unchanged: 44220 admits exactly three two-field tags, so
  // an attachment can only ever travel inside the content.
  assert.deepEqual(
    event.tags.map(([name]) => name),
    ["h", "cs-v", "cs-target"],
  );

  const build = (attachments) =>
    buildCodingSessionCommandEvent({
      channelId: "3f2b8c1e-0000-4000-8000-000000000001",
      commandId: "cmd-3",
      target,
      text: "go",
      attachments,
      deliver: "boundary",
    });

  for (const [bad, label] of [
    [{ ...attachment, sha256: "abc" }, "short hash"],
    [{ ...attachment, sha256: attachment.sha256.toUpperCase() }, "uppercase"],
    [{ ...attachment, mime: "application/pdf" }, "non-image mime"],
    [{ ...attachment, size: 0 }, "zero size"],
    [{ ...attachment, size: 10 * 1024 * 1024 + 1 }, "oversize"],
  ]) {
    assert.throws(() => build([bad]), undefined, `accepted ${label}`);
  }
  assert.throws(() => build(new Array(5).fill(attachment)), /exceeds 4/);
});
