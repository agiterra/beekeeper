import assert from "node:assert/strict";
import { test } from "node:test";

import {
  activeCodingSessionKey,
  codingSessionProjectRef,
  codingSessionRowKey,
} from "./activeCodingSession.ts";

const CHANNEL = "9422dafe-618d-4563-b33d-f2f634683e28";
const GENERATION = "gen-1";
const PROJECT = `30621:${"a".repeat(64)}:test-p`;

test("the active key is the pair a session row is opened by", () => {
  assert.equal(
    activeCodingSessionKey(`/coding-sessions/${CHANNEL}/${GENERATION}`, {
      channelId: CHANNEL,
      generationId: GENERATION,
    }),
    codingSessionRowKey(CHANNEL, GENERATION),
  );
});

test("no coding session route means no active session", () => {
  // `channelId` is a route param on other surfaces too. Matching on params
  // alone would light up a session row while a channel was on screen.
  assert.equal(
    activeCodingSessionKey(`/channels/${CHANNEL}`, { channelId: CHANNEL }),
    null,
  );
  assert.equal(activeCodingSessionKey("/projects/p1", {}), null);
});

test("a half-specified route selects nothing", () => {
  assert.equal(
    activeCodingSessionKey(`/coding-sessions/${CHANNEL}`, {
      channelId: CHANNEL,
    }),
    null,
  );
});

test("a session route resolves its project through the transport channel", () => {
  // The regression: a session's channel is a transport, which the default
  // channel view hides, so the shell's activeChannel was null here and the
  // hotkey scope resolved to nothing.
  assert.equal(
    codingSessionProjectRef(
      `/coding-sessions/${CHANNEL}/${GENERATION}`,
      CHANNEL,
      [{ id: CHANNEL, projectRef: PROJECT, channelType: "transport" }],
    ),
    PROJECT,
  );
});

test("off a session route it defers, so the active channel keeps its meaning", () => {
  assert.equal(
    codingSessionProjectRef(`/channels/${CHANNEL}`, CHANNEL, [
      { id: CHANNEL, projectRef: PROJECT },
    ]),
    null,
  );
});

test("an unknown or unlinked channel resolves to nothing rather than guessing", () => {
  assert.equal(
    codingSessionProjectRef(
      `/coding-sessions/${CHANNEL}/${GENERATION}`,
      CHANNEL,
      [{ id: "other", projectRef: PROJECT }],
    ),
    null,
  );
  assert.equal(
    codingSessionProjectRef(
      `/coding-sessions/${CHANNEL}/${GENERATION}`,
      CHANNEL,
      [{ id: CHANNEL, projectRef: null }],
    ),
    null,
  );
  assert.equal(
    codingSessionProjectRef(
      `/coding-sessions/${CHANNEL}/${GENERATION}`,
      CHANNEL,
      undefined,
    ),
    null,
  );
});

test("a founded route selects the founded shelf row by its row id", async () => {
  const { foundedCodingSessionRowId } = await import(
    "@/features/coding-sessions/lib/codingSessionRoute.ts"
  );
  const REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
  assert.equal(
    activeCodingSessionKey(`/coding-sessions/${CHANNEL}/founded/${REF}`, {
      channelId: CHANNEL,
      sessionRef: REF,
    }),
    codingSessionRowKey(CHANNEL, foundedCodingSessionRowId(REF)),
  );
  // Never the generation key for the same ref: the founded row and a
  // started row of one umbrella are different rows.
  assert.notEqual(
    activeCodingSessionKey(`/coding-sessions/${CHANNEL}/founded/${REF}`, {
      channelId: CHANNEL,
      sessionRef: REF,
    }),
    codingSessionRowKey(CHANNEL, REF),
  );
  // Off a session route, a sessionRef param selects nothing.
  assert.equal(
    activeCodingSessionKey(`/channels/${CHANNEL}`, {
      channelId: CHANNEL,
      sessionRef: REF,
    }),
    null,
  );
});
