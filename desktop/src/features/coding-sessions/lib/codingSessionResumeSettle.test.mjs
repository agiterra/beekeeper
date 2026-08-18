import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_RESUME_CONFLICT_MESSAGE,
  CODING_SESSION_RESUME_REFUSED_MESSAGE,
  resolveCodingSessionResumeSettlement,
} from "./codingSessionResumeSettle.ts";
import { buildCodingSessionTranscriptGenerationId } from "./codingSessionTranscriptPresentation.ts";
import { RESUMED_WITHOUT_CONTEXT_HEADING } from "./newCodingSessionModel.ts";

const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";
const PROVIDER = "a".repeat(64);
const COMMAND_ID = "csl-resume-1";

const DEAD_GENERATION = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const RESUMED_GENERATION = { ...DEAD_GENERATION, generation: 2 };

const METADATA = { title: "Coding session", status: "idle" };

function settlement(lifecycle) {
  return resolveCodingSessionResumeSettlement({
    channelId: CHANNEL_ID,
    lifecycle,
    providerAuthorityPubkey: PROVIDER,
  });
}

test("a resumed receipt settles onto the generation the provider minted", () => {
  const resolved = settlement({
    state: "created",
    commandId: COMMAND_ID,
    target: RESUMED_GENERATION,
    metadata: METADATA,
  });
  assert.equal(resolved.kind, "established");
  assert.equal(resolved.commandId, COMMAND_ID);
  assert.equal(resolved.notice, null);
  // Exactly the id the catalog builds for generation N+1 — and never the dead
  // generation the person pressed Reconnect from.
  assert.equal(
    resolved.generationId,
    buildCodingSessionTranscriptGenerationId(
      CHANNEL_ID,
      PROVIDER,
      RESUMED_GENERATION,
    ),
  );
  assert.notEqual(
    resolved.generationId,
    buildCodingSessionTranscriptGenerationId(
      CHANNEL_ID,
      PROVIDER,
      DEAD_GENERATION,
    ),
  );
});

test("a resume that recovered no context still opens, and says so", () => {
  const resolved = settlement({
    state: "resumed-without-context",
    commandId: COMMAND_ID,
    target: RESUMED_GENERATION,
    metadata: METADATA,
    error: {
      code: "CONTEXT_NOT_RECOVERED",
      message: "The prior session file was pruned.",
    },
  });
  assert.equal(resolved.kind, "established");
  assert.equal(
    resolved.generationId,
    buildCodingSessionTranscriptGenerationId(
      CHANNEL_ID,
      PROVIDER,
      RESUMED_GENERATION,
    ),
  );
  // The loss is reported with the provider's own reason kept — never dropped,
  // and never used to withhold the session that does exist.
  assert.match(resolved.notice, new RegExp(RESUMED_WITHOUT_CONTEXT_HEADING));
  assert.match(resolved.notice, /The prior session file was pruned\./);
});

test("a refused resume surfaces the provider's reason instead of silence", () => {
  const resolved = settlement({
    state: "failed",
    commandId: COMMAND_ID,
    error: {
      code: "STALE_GENERATION",
      message: "Generation 1 is not the active generation.",
    },
  });
  assert.deepEqual(resolved, {
    kind: "failed",
    commandId: COMMAND_ID,
    message: "Generation 1 is not the active generation.",
  });

  // A refusal with no readable message still has to say something.
  assert.equal(
    settlement({
      state: "failed",
      commandId: COMMAND_ID,
      error: { code: "STALE_GENERATION", message: "   " },
    }).message,
    CODING_SESSION_RESUME_REFUSED_MESSAGE,
  );
  assert.equal(
    settlement({ state: "conflict", commandId: COMMAND_ID }).message,
    CODING_SESSION_RESUME_CONFLICT_MESSAGE,
  );
});

test("every unsettled state stays pending rather than guessing", () => {
  assert.deepEqual(settlement(null), { kind: "pending" });
  assert.deepEqual(settlement({ state: "pending", commandId: COMMAND_ID }), {
    kind: "pending",
  });
  // A named target with no metadata yet is not a place to navigate to.
  assert.deepEqual(
    settlement({
      state: "awaiting-metadata",
      commandId: COMMAND_ID,
      target: RESUMED_GENERATION,
      malformedMetadataCount: 0,
    }),
    { kind: "pending" },
  );
  // Without an exact provider authority there is no generation identity to
  // build, so there is nothing to settle either.
  assert.deepEqual(
    resolveCodingSessionResumeSettlement({
      channelId: CHANNEL_ID,
      lifecycle: {
        state: "created",
        commandId: COMMAND_ID,
        target: RESUMED_GENERATION,
        metadata: METADATA,
      },
      providerAuthorityPubkey: null,
    }),
    { kind: "pending" },
  );
});
