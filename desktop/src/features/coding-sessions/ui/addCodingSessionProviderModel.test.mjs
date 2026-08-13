/**
 * The join path (design §B): "Add a provider to this session" publishes an
 * ordinary 44221 create carrying the umbrella's *existing* sessionRef, pinned
 * to the umbrella's channel. These pin the three things that make it a join
 * rather than a second session — the ref is reused, the channel cannot drift,
 * and the picker is honest about which runtimes the session already runs.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { groupCodingSessionCatalog } from "../lib/codingSessionUmbrellaModel.ts";
import { resolveNewCodingSessionTargets } from "../lib/newCodingSessionModel.ts";
import { buildNewCodingSessionCreateInput } from "./useNewCodingSessionCreate.ts";
import {
  addCodingSessionProviderOptionNote,
  buildAddCodingSessionProviderSubmit,
  defaultAddCodingSessionProviderKey,
  listAddCodingSessionProviderOptions,
} from "./addCodingSessionProviderModel.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";
const OTHER_CHANNEL_ID = "11111111-2222-3333-4444-555555555555";
const PROVIDER_PUBKEY = "a".repeat(64);

const CLAUDE_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "claude-instance",
  sessionId: "11111111-1111-1111-1111-111111111111",
  generation: 1,
};

function capabilities() {
  return {
    threadTurnStart: true,
    threadTurnInterrupt: true,
    threadSteer: false,
    context: false,
    diff: false,
    plan: true,
  };
}

function runtime(overrides = {}) {
  return {
    instanceRef: "claude-primary",
    runtime: "claude",
    driver: "claude-agent-acp",
    label: "Claude Code",
    authState: "ready",
    defaultModel: "claude-opus-5",
    allowedModels: ["claude-opus-5"],
    capabilities: capabilities(),
    ...overrides,
  };
}

const CODEX_RUNTIME = runtime({
  instanceRef: "codex-primary",
  runtime: "codex",
  driver: "codex-acp",
  label: "Codex",
  defaultModel: "gpt-5.3-codex",
  allowedModels: ["gpt-5.3-codex"],
});

/** A live single-execution session, exactly what a new client creates today. */
function singleClaudeUmbrella() {
  const umbrellas = groupCodingSessionCatalog([
    {
      generationId: "gen-1",
      label: "claude-agent-acp · generation 1",
      title: "Advance Buzz live sessions",
      providerAuthorityPubkey: PROVIDER_PUBKEY,
      metadataAuthorityPubkey: PROVIDER_PUBKEY,
      lastEventAt: "2026-08-12T10:10:00.000Z",
      status: "completed",
      transcript: [],
      conflictCount: 0,
      commandTarget: CLAUDE_TARGET,
      projectRef: null,
      repoRef: null,
      sessionRef: SESSION_REF,
      provider: null,
      runtime: "claude",
      model: "claude-opus-5",
      capabilities: capabilities(),
    },
  ]);
  assert.equal(umbrellas.length, 1);
  assert.equal(umbrellas[0].executions.length, 1);
  return umbrellas[0];
}

function localTargets(channelId, runtimes) {
  return resolveNewCodingSessionTargets({
    catalogs: [],
    channelId,
    localProvider: { providerPubkey: PROVIDER_PUBKEY, runtimes },
  });
}

test("the runtime already in the session is marked, not hidden, and never the default", () => {
  const umbrella = singleClaudeUmbrella();
  const options = listAddCodingSessionProviderOptions({
    targets: localTargets(CHANNEL_ID, [runtime(), CODEX_RUNTIME]),
    umbrella,
  });

  assert.equal(options.length, 2);
  const claude = options.find(
    (option) => option.target.provider.runtime === "claude",
  );
  const codex = options.find(
    (option) => option.target.provider.runtime === "codex",
  );
  assert.equal(claude.alreadyInSession, true);
  assert.equal(codex.alreadyInSession, false);
  assert.equal(
    addCodingSessionProviderOptionNote(claude),
    "already in this session",
  );
  assert.equal(addCodingSessionProviderOptionNote(codex), null);
  // The Brian demo: start Claude, add Codex — Codex is preselected.
  assert.equal(
    defaultAddCodingSessionProviderKey(options),
    codex.target.selectionKey,
  );
});

test("a signed-out runtime is never the join default", () => {
  const options = listAddCodingSessionProviderOptions({
    targets: localTargets(CHANNEL_ID, [
      runtime(),
      { ...CODEX_RUNTIME, authState: "needs_auth" },
    ]),
    umbrella: singleClaudeUmbrella(),
  });
  // Claude is already in the session and Codex cannot serve: nothing is
  // preselected rather than silently queuing a create that must fail.
  assert.equal(defaultAddCodingSessionProviderKey(options), null);
});

test("the join is pinned to the session's channel", () => {
  const umbrella = singleClaudeUmbrella();
  // Targets are resolved for the umbrella's channel only, so no option can
  // land the joining execution somewhere the session does not live.
  const options = listAddCodingSessionProviderOptions({
    targets: localTargets(CHANNEL_ID, [runtime(), CODEX_RUNTIME]),
    umbrella,
  });
  for (const option of options) {
    assert.equal(option.target.channelId, CHANNEL_ID);
  }

  // And a target from elsewhere is refused outright rather than published.
  const [foreign] = localTargets(OTHER_CHANNEL_ID, [CODEX_RUNTIME]);
  assert.equal(
    buildAddCodingSessionProviderSubmit({
      umbrella,
      channelId: CHANNEL_ID,
      target: foreign,
      model: null,
      initialTurn: "",
      workdir: "",
    }),
    null,
  );
});

test("the join create carries the umbrella's existing sessionRef", () => {
  const umbrella = singleClaudeUmbrella();
  const [codex] = localTargets(CHANNEL_ID, [CODEX_RUNTIME]);
  const payload = buildAddCodingSessionProviderSubmit({
    umbrella,
    channelId: CHANNEL_ID,
    target: codex,
    model: "gpt-5.3-codex",
    initialTurn: "  Take Claude's finding and regenerate the fixture.  ",
    workdir: " /Users/brian/checkout ",
  });

  assert.equal(payload.sessionRef, SESSION_REF);
  assert.equal(payload.target.channelId, CHANNEL_ID);
  assert.equal(payload.model, "gpt-5.3-codex");
  // The session's title is inherited, not re-invented.
  assert.equal(payload.title, "Advance Buzz live sessions");
  assert.equal(payload.workdir, "/Users/brian/checkout");

  // …and that ref reaches the signed command unchanged: this is the whole
  // difference between joining an umbrella and founding a second session.
  const created = buildNewCodingSessionCreateInput({
    channelId: payload.target.channelId,
    commandId: "csc-join-1",
    providerInstanceRef: payload.target.provider.providerInstanceRef,
    providerAuthorityPubkey: payload.target.signerPubkey,
    model: payload.model,
    title: payload.title,
    initialTurn: payload.initialTurn,
    sessionRef: payload.sessionRef,
  });
  assert.equal(created.sessionRef, SESSION_REF);
  assert.equal(created.channelId, CHANNEL_ID);
  assert.equal(created.providerInstanceRef, "codex-primary");
});

test("a pre-umbrella session has no ref to join and publishes nothing", () => {
  const legacy = groupCodingSessionCatalog([
    {
      generationId: "gen-legacy",
      label: "claude-agent-acp · generation 1",
      title: "Old session",
      providerAuthorityPubkey: PROVIDER_PUBKEY,
      metadataAuthorityPubkey: PROVIDER_PUBKEY,
      lastEventAt: "2026-08-12T10:10:00.000Z",
      status: "completed",
      transcript: [],
      conflictCount: 0,
      commandTarget: CLAUDE_TARGET,
      projectRef: null,
      repoRef: null,
      sessionRef: null,
      provider: null,
      runtime: "claude",
      model: null,
      capabilities: capabilities(),
    },
  ])[0];
  const [codex] = localTargets(CHANNEL_ID, [CODEX_RUNTIME]);

  assert.equal(legacy.sessionRef, null);
  assert.equal(
    buildAddCodingSessionProviderSubmit({
      umbrella: legacy,
      channelId: CHANNEL_ID,
      target: codex,
      model: null,
      initialTurn: "",
      workdir: "",
    }),
    null,
  );
});
