import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionCommandEvent,
  CODING_SESSION_CI_CONTINUATION_ACTION_TYPE,
  CODING_SESSION_HOST_ANSWER_TAG_UNSUPPORTED_MESSAGE,
  CODING_SESSION_TURN_DELIVERIES,
  buildCodingSessionInterruptEvent,
  buildCodingSessionTargetKey,
  codingSessionTargetSupportsInterrupt,
  isCodingSessionCiContinuationAction,
  isCodingSessionHostAnswerTagUnsupportedRejection,
  isCodingSessionTextAttachmentUnsupportedRejection,
  MAX_CODING_SESSION_TEXT_ATTACHMENT_BYTES,
  MAX_CODING_SESSION_IDENTIFIER_BYTES,
  MAX_CODING_SESSION_TEXT_BYTES,
  publishCodingSessionCommand,
  publishCodingSessionInterrupt,
  validateCodingSessionCommandInput,
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

test("hostAnswer adds exactly one buzz-host-answer tag, and is omitted by default", () => {
  const marked = buildCodingSessionCommandEvent({
    channelId: "channel-1",
    commandId: "cmd-host-answer",
    target,
    text: "hire refused: HIRE_OFF — hiring is switched off",
    deliver: "boundary",
    hostAnswer: true,
  });
  assert.deepEqual(marked.tags, [
    ["h", "channel-1"],
    ["cs-v", "csc1-1"],
    ["cs-target", "coding-session/v1|10:provider-a10:instance-19:session-11:2"],
    ["buzz-host-answer", "hire"],
  ]);
  // It never touches the signed content: `bee sessions hire` still parses
  // `action.text` structurally, and a provider's turn intake reads the tag,
  // never the content, to recognize the answer.
  assert.equal("hostAnswer" in JSON.parse(marked.content).action, false);

  const ordinary = buildCodingSessionCommandEvent({
    channelId: "channel-1",
    commandId: "cmd-ordinary",
    target,
    text: "do the thing",
    deliver: "boundary",
  });
  assert.equal(
    ordinary.tags.some((tag) => tag[0] === "buzz-host-answer"),
    false,
  );
});

/**
 * Ledger 200 (adversarial review, finding 8): the unknown-tag rejection
 * ("unsupported coding-session command tag") is a substring of the
 * *different* unsupported-tag-*version* rejection ("unsupported
 * coding-session command tag version"), so a substring match wrongly
 * recognized the version refusal too and triggered the same untagged retry
 * for it. Matching must strip the rejection's classifier prefix and compare
 * the remainder exactly.
 */
test("isCodingSessionHostAnswerTagUnsupportedRejection: matches only the exact unknown-tag rejection, prefixed as the relay sends it", () => {
  assert.equal(
    isCodingSessionHostAnswerTagUnsupportedRejection(
      new Error(
        `invalid: ${CODING_SESSION_HOST_ANSWER_TAG_UNSUPPORTED_MESSAGE}`,
      ),
    ),
    true,
  );
  // Trimmed and prefix-stripped, but otherwise exact — no fuzzier than the
  // relay's own wire format requires.
  assert.equal(
    isCodingSessionHostAnswerTagUnsupportedRejection(
      new Error(
        `  invalid:   ${CODING_SESSION_HOST_ANSWER_TAG_UNSUPPORTED_MESSAGE}  `,
      ),
    ),
    true,
  );
});

test("isCodingSessionHostAnswerTagUnsupportedRejection: the tag-*version* rejection is a superstring, and must not match", () => {
  assert.equal(
    isCodingSessionHostAnswerTagUnsupportedRejection(
      new Error(
        `invalid: ${CODING_SESSION_HOST_ANSWER_TAG_UNSUPPORTED_MESSAGE} version`,
      ),
    ),
    false,
  );
});

test("isCodingSessionHostAnswerTagUnsupportedRejection: unrelated rejections and non-Error values never match", () => {
  assert.equal(
    isCodingSessionHostAnswerTagUnsupportedRejection(
      new Error("restricted: not a member"),
    ),
    false,
  );
  assert.equal(
    isCodingSessionHostAnswerTagUnsupportedRejection(
      new Error(
        CODING_SESSION_HOST_ANSWER_TAG_UNSUPPORTED_MESSAGE.slice(0, -1),
      ),
    ),
    false,
  );
  assert.equal(
    isCodingSessionHostAnswerTagUnsupportedRejection("not an error"),
    false,
  );
  assert.equal(
    isCodingSessionHostAnswerTagUnsupportedRejection(undefined),
    false,
  );
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
    [{ ...attachment, mime: "application/pdf" }, "a mime on no allowlist"],
    [{ ...attachment, size: 0 }, "zero size"],
    [{ ...attachment, size: 10 * 1024 * 1024 + 1 }, "oversize"],
  ]) {
    assert.throws(() => build([bad]), undefined, `accepted ${label}`);
  }
  assert.throws(() => build(new Array(5).fill(attachment)), /exceeds 4/);
});

const pastedFile = {
  sha256: "bb0011223344556677889900aabbccddeeff00112233445566778899aabbccdd",
  mime: "text/plain",
  size: 4096,
  filename: "pasted-text-1.txt",
};

test("a pasted file rides the same payload, bounded by the text ceiling", () => {
  const event = buildCodingSessionCommandEvent({
    channelId: "3f2b8c1e-0000-4000-8000-000000000001",
    commandId: "cmd-text",
    target,
    text: "fix the crash in [pasted-text-1.txt](http://relay/media/x.txt)",
    attachments: [pastedFile],
    deliver: "boundary",
  });
  assert.deepEqual(JSON.parse(event.content).action.attachments, [pastedFile]);

  const build = (attachments) =>
    buildCodingSessionCommandEvent({
      channelId: "3f2b8c1e-0000-4000-8000-000000000001",
      commandId: "cmd-text-2",
      target,
      text: "go",
      attachments,
      deliver: "boundary",
    });

  // Each kind against its own ceiling. The byte count an image is allowed is
  // refused for text, and the message names the bound that refused it rather
  // than the other one.
  assert.doesNotThrow(() =>
    build([{ ...pastedFile, size: MAX_CODING_SESSION_TEXT_ATTACHMENT_BYTES }]),
  );
  assert.throws(
    () =>
      build([
        { ...pastedFile, size: MAX_CODING_SESSION_TEXT_ATTACHMENT_BYTES + 1 },
      ]),
    new RegExp(
      `between 1 and ${MAX_CODING_SESSION_TEXT_ATTACHMENT_BYTES} bytes`,
    ),
  );
  assert.doesNotThrow(() =>
    build([
      { ...attachment, size: MAX_CODING_SESSION_TEXT_ATTACHMENT_BYTES + 1 },
    ]),
  );

  // One turn can carry both, and the shared count cap covers them together.
  assert.doesNotThrow(() => build([attachment, pastedFile]));
  assert.throws(
    () => build([attachment, attachment, pastedFile, pastedFile, pastedFile]),
    /exceeds 4/,
  );
});

/**
 * The rollout hazard this predicate exists for: the desktop can publish a
 * pasted file the moment it ships, and a relay that has not been rebuilt
 * refuses it by reciting its own allowlist. That sentence blames the paste.
 */
test("isCodingSessionTextAttachmentUnsupportedRejection: only a relay whose allowlist lacks text/plain", () => {
  assert.equal(
    isCodingSessionTextAttachmentUnsupportedRejection(
      new Error(
        "invalid: action.attachments[0].mime must be one of image/jpeg, image/png, image/gif, image/webp",
      ),
    ),
    true,
  );
  // A relay that *does* list text/plain refused this for some other reason —
  // a genuinely bad MIME — and must keep its own message.
  assert.equal(
    isCodingSessionTextAttachmentUnsupportedRejection(
      new Error(
        "invalid: action.attachments[1].mime must be one of image/jpeg, image/png, image/gif, image/webp, text/plain",
      ),
    ),
    false,
  );
  for (const other of [
    new Error(
      "invalid: action.attachments[0].size must be between 1 and 10 bytes",
    ),
    new Error("restricted: not a member"),
    "not an error",
    undefined,
  ]) {
    assert.equal(
      isCodingSessionTextAttachmentUnsupportedRejection(other),
      false,
    );
  }
});

// ── A CI-continuation registration is a known action, strictly shaped ───────
//
// `thread.turn.continue_on_ci` (`CodingSessionAction::ThreadTurnContinueOnCi`,
// crates/buzz-core/src/coding_session_command.rs) is a third closed action on
// the same 44220. It is neither a turn start nor an interrupt: nothing is
// queued and no turn exists until the named CI result is recorded. The relay
// and the provider both decode it with `deny_unknown_fields`, so this reader
// is exact on both the action's four keys and the identity's eight.

const CI_OWNER = "ab".repeat(32);

function ciContinuationAction(overrides = {}) {
  return {
    type: CODING_SESSION_CI_CONTINUATION_ACTION_TYPE,
    identity: {
      project: `30621:${CI_OWNER}:beekeeper`,
      repository: `30617:${CI_OWNER}:beekeeper`,
      commit: "abcdef0123456789abcdef0123456789abcdef01",
      check: "main-validation",
      run: "136",
      attempt: 1,
      workflow: "d3e440ea-89f8-4aee-8a02-17edc3e7272e",
      phase: "build",
    },
    continuation: "Report the failing test",
    expiresAt: 1_788_800_000,
    ...overrides,
  };
}

test("a CI continuation is a known action, distinct from a start or interrupt", () => {
  const action = ciContinuationAction();
  assert.equal(isCodingSessionCiContinuationAction(action), true);
  assert.notEqual(action.type, "thread.turn.start");
  assert.notEqual(action.type, "thread.turn.interrupt");
  assert.equal(
    CODING_SESSION_CI_CONTINUATION_ACTION_TYPE,
    "thread.turn.continue_on_ci",
  );
  // The wire shape is exactly the four keys, in the order buzz-core writes.
  assert.deepEqual(Object.keys(action), [
    "type",
    "identity",
    "continuation",
    "expiresAt",
  ]);
  assert.deepEqual(Object.keys(action.identity), [
    "project",
    "repository",
    "commit",
    "check",
    "run",
    "attempt",
    "workflow",
    "phase",
  ]);
  // A start and an interrupt are not continuations, so no surface can mistake
  // one for the other.
  assert.equal(
    isCodingSessionCiContinuationAction({
      type: "thread.turn.start",
      text: "go",
    }),
    false,
  );
  assert.equal(
    isCodingSessionCiContinuationAction({ type: "thread.turn.interrupt" }),
    false,
  );
});

test("a CI continuation reader is exact about its keys and its bounds", () => {
  const rejected = [
    // A key the relay would refuse under deny_unknown_fields.
    ciContinuationAction({ deliver: "steer" }),
    // The three required keys, each missing in turn.
    (() => {
      const value = ciContinuationAction();
      delete value.identity;
      return value;
    })(),
    (() => {
      const value = ciContinuationAction();
      delete value.continuation;
      return value;
    })(),
    (() => {
      const value = ciContinuationAction();
      delete value.expiresAt;
      return value;
    })(),
    // No horizon is not "waits forever".
    ciContinuationAction({ expiresAt: 0 }),
    ciContinuationAction({ expiresAt: -1 }),
    ciContinuationAction({ expiresAt: 1.5 }),
    ciContinuationAction({ expiresAt: "1788800000" }),
    // A registration with no words is a wake with nothing to say.
    ciContinuationAction({ continuation: "   " }),
    ciContinuationAction({
      continuation: `${"é".repeat(MAX_CODING_SESSION_TEXT_BYTES / 2)}a`,
    }),
    // A ninth identity key names a run no recorded result could satisfy.
    ciContinuationAction({
      identity: { ...ciContinuationAction().identity, branch: "main" },
    }),
    ciContinuationAction({
      identity: (() => {
        const identity = { ...ciContinuationAction().identity };
        delete identity.phase;
        return identity;
      })(),
    }),
    ciContinuationAction({
      identity: { ...ciContinuationAction().identity, phase: "release" },
    }),
    ciContinuationAction({
      identity: { ...ciContinuationAction().identity, attempt: 0 },
    }),
  ];
  for (const value of rejected) {
    assert.equal(
      isCodingSessionCiContinuationAction(value),
      false,
      JSON.stringify(value),
    );
  }
  // The largest continuation the contract admits is still admitted.
  assert.equal(
    isCodingSessionCiContinuationAction(
      ciContinuationAction({
        continuation: "é".repeat(MAX_CODING_SESSION_TEXT_BYTES / 2),
      }),
    ),
    true,
  );
});

test("a malformed CI continuation is refused before anything is signed", () => {
  assert.throws(
    () =>
      validateCodingSessionCommandInput({
        commandId: "cic-1",
        target,
        action: ciContinuationAction({ expiresAt: 0 }),
      }),
    /action must carry exactly type, identity, continuation, and expiresAt/,
  );
  assert.doesNotThrow(() =>
    validateCodingSessionCommandInput({
      commandId: "cic-1",
      target,
      action: ciContinuationAction(),
    }),
  );
});
