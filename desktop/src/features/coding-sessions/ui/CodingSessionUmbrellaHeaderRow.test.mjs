import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { parseBeekeeperCodingSessionMetadata } from "../lib/codingSessionIngressPayloads.ts";
import { deriveSeatBeeStamps } from "../lib/codingSessionSeatBee.ts";
import { deriveCodingSessionStreamPresence } from "../lib/codingSessionStreamPresence.ts";
import { groupCodingSessionCatalog } from "../lib/codingSessionUmbrellaModel.ts";
import { UNKNOWN_CODING_SESSION_REACHABILITY } from "../hooks/useCodingSessionProviderReachability.ts";
import { CodingSessionUmbrellaHeaderRow } from "./CodingSessionUmbrellaHeaderRow.tsx";
import { fakeSurfaceShell } from "./CodingSessionHeaderPanelToggles.testFixtures.mjs";
import { codingSessionHeaderRepoName } from "./CodingSessionHeaderDetails.tsx";

/**
 * L17 gap 1, red first.
 *
 * Before this lane, `CodingSessionParticipantBar`'s `seatBeeStamps` prop
 * (mounted at `CodingSessionUmbrellaHeaderRow.tsx`, inside the `mission`
 * branch) had no production caller — `grep -rn "seatBeeStamps=" desktop/src`
 * matched only tests. This test mounts the umbrella header row — the actual
 * file that owns the `<CodingSessionParticipantBar>` JSX — with a real,
 * decoded kind:44223 `beeStamp` and asserts the chip is on the page.
 *
 * The stamp travels the real pipeline end to end except for React state: a
 * raw 44223 JSON payload goes through `parseBeekeeperCodingSessionMetadata` (the
 * real decoder this lane widened to accept `beeStamp`), the catalog record
 * carries it through `groupCodingSessionCatalog`, and `deriveSeatBeeStamps`
 * (the real L17 selector) turns the umbrella's executions into the map this
 * component threads to `CodingSessionParticipantBar`. This file fails to even
 * import on base — `deriveSeatBeeStamps` does not exist there — which is the
 * literal shape of "red first" for this gap.
 */

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CLAUDE_SIGNER = "a".repeat(64);
const CODEX_SIGNER = "b".repeat(64);

const CLAUDE_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "claude-instance",
  sessionId: "11111111-1111-1111-1111-111111111111",
  generation: 1,
};
const CODEX_TARGET = {
  driver: "codex-acp",
  instanceId: "codex-instance",
  sessionId: "22222222-2222-2222-2222-222222222222",
  generation: 1,
};

const CAPABILITIES = {
  threadTurnStart: true,
  threadTurnInterrupt: true,
  threadSteer: false,
  context: false,
  diff: false,
  plan: false,
};

/** Exactly the wire shape a host publishes on kind:44223 (L12). */
function metadataContent(target, overrides = {}) {
  return JSON.stringify({
    schema: "buzz-coding-session-metadata/v1",
    session: target,
    projectRef: null,
    repoRef: null,
    title: "Advance Beekeeper live sessions",
    agentRef: null,
    provider: null,
    runtime: target.driver,
    model: "claude-opus-5",
    status: "idle",
    branch: null,
    capabilities: CAPABILITIES,
    ...overrides,
  });
}

function catalogRecord({ target, signerPubkey, metadata }) {
  return {
    generationId: `gen-${signerPubkey.slice(0, 4)}`,
    label: `${target.driver} · generation ${target.generation}`,
    title: metadata?.title ?? "Coding session",
    providerAuthorityPubkey: signerPubkey,
    metadataAuthorityPubkey: signerPubkey,
    lastEventAt: "2026-08-12T10:10:00.000Z",
    status: metadata?.status ?? "idle",
    statusAt: null,
    statusEventId: "e".repeat(64),
    transcript: [],
    conflictCount: 0,
    commandTarget: target,
    projectRef: metadata?.projectRef ?? null,
    repoRef: metadata?.repoRef ?? null,
    sessionRef: SESSION_REF,
    provider: metadata?.provider ?? null,
    runtime: metadata?.runtime ?? target.driver,
    model: metadata?.model ?? null,
    agentRef: metadata?.agentRef ?? null,
    role: null,
    turnBudget: null,
    routing: null,
    capabilities: metadata?.capabilities ?? null,
    beeStamp: metadata?.beeStamp ?? null,
  };
}

/** One umbrella of two seats: `builder`'s 44223 carries a real beeStamp, `codex`'s carries none. */
function buildUmbrella() {
  const claudeMetadata = parseBeekeeperCodingSessionMetadata(
    metadataContent(CLAUDE_TARGET, {
      beeStamp: {
        path: "/Applications/Beekeeper.app/Contents/MacOS/bee",
        source: "bundled",
        version: "0.1.0",
        sha: "23728227b",
        dirty: false,
      },
    }),
  );
  assert.ok(
    claudeMetadata,
    "the real decoder must accept a real beeStamp payload",
  );
  assert.equal(claudeMetadata.beeStamp.sha, "23728227b");

  const codexMetadata = parseBeekeeperCodingSessionMetadata(
    metadataContent(CODEX_TARGET),
  );
  assert.ok(codexMetadata);
  assert.equal(codexMetadata.beeStamp, undefined);

  const claude = catalogRecord({
    target: CLAUDE_TARGET,
    signerPubkey: CLAUDE_SIGNER,
    metadata: claudeMetadata,
  });
  const codex = catalogRecord({
    target: CODEX_TARGET,
    signerPubkey: CODEX_SIGNER,
    metadata: codexMetadata,
  });
  const umbrellas = groupCodingSessionCatalog([claude, codex]);
  assert.equal(umbrellas.length, 1);
  assert.equal(umbrellas[0].executions.length, 2);
  return umbrellas[0];
}

function renderHeaderRow(umbrella, overrides = {}) {
  const streamParticipants = deriveCodingSessionStreamPresence({
    umbrella,
    resolveStatus: () => ({ kind: "idle", label: "Idle" }),
  }).participants;
  const seatBeeStamps = deriveSeatBeeStamps(umbrella.executions);

  return renderToStaticMarkup(
    React.createElement(CodingSessionUmbrellaHeaderRow, {
      agentFocusItems: [],
      authoritativeTitle: umbrella.title,
      canRename: false,
      channelName: "engineering",
      composerTaskDock: {
        activeModel: null,
        open: false,
        close() {},
        toggle() {},
      },
      contextLoads: undefined,
      focusedExecution: umbrella.executions[0],
      focusedExecutionKey: umbrella.executions[0].executionKey,
      goal: null,
      handleFocusExecution() {},
      handleLensChange() {},
      handleMissionDensityChange() {},
      handlePopout() {},
      handleStopAll() {},
      handleToggleRouteRail() {},
      headerCompact: false,
      isMultiExecution: true,
      isNarrow: false,
      lens: "mission",
      mission: true,
      missionDensity: "brief",
      peopleCount: 2,
      resolveReachability: UNKNOWN_CODING_SESSION_REACHABILITY,
      routedSeats: undefined,
      routeRail: { roomForRail: true, fits: true },
      seatBeeStamps,
      sessionClosed: false,
      setRenameOpen() {},
      streamParticipants,
      stopAll: { kind: "unavailable" },
      surface: "main",
      surfaceHostId: "surface-host-1",
      surfaceShell: fakeSurfaceShell({ surfaces: [] }).shell,
      teamWake: { deliveries: [], seatAuthorities: [], refusal: null },
      umbrella,
      workspaceActorName: () => null,
      ...overrides,
    }),
  );
}

test("a seat's real, decoded 44223 beeStamp reaches the participant bar's chip", () => {
  const umbrella = buildUmbrella();
  const markup = renderHeaderRow(umbrella);
  assert.match(
    markup,
    /data-testid="coding-session-seat-bee"/,
    "the chip's own data-testid must be on the page at all",
  );
  assert.match(
    markup,
    /bee 23728227b \(bundled\)/,
    "the exact words the real stamp resolves to",
  );
});

test("a seat whose 44223 carried no beeStamp renders no chip for it — absence, not unknown", () => {
  const umbrella = buildUmbrella();
  const markup = renderHeaderRow(umbrella);
  // The codex seat published no `beeStamp` key at all; the sentence reserved
  // for an observed-but-unparsed `--version` must never appear for it.
  assert.doesNotMatch(markup, /bee build unknown/);
});

test("SV-20: Mission keeps its Route toggle, gains the panel toggles, and loses the Inspector toggle", () => {
  const markup = renderHeaderRow(buildUmbrella());
  assert.match(markup, /data-testid="coding-session-route-toggle"/);
  assert.match(markup, /data-testid="coding-session-panel-toggle-right"/);
  assert.match(markup, /data-testid="coding-session-panel-toggle-bottom"/);
  assert.doesNotMatch(markup, /coding-session-surface-toggle/);
  assert.doesNotMatch(markup, /coding-session-task-rail-toggle/);
});

test("SV-20: a team session's header leads with its project crumb, from the shell's project", () => {
  const { shell } = fakeSurfaceShell({ surfaces: [] });
  const withProject = {
    ...shell,
    ctx: { ...shell.ctx, project: { id: "project-1", name: "Beekeeper Glue" } },
  };
  const markup = renderHeaderRow(buildUmbrella(), {
    onOpenProject() {},
    surfaceShell: withProject,
  });
  const project = markup.indexOf(">Beekeeper Glue<");
  const separator = markup.indexOf(">/</li>");
  assert.ok(project >= 0 && separator > project, markup);
  assert.match(markup, /data-testid="coding-session-project-crumb"/);

  // No project resolved: no crumb, and no invented one.
  const bare = renderHeaderRow(buildUmbrella());
  assert.doesNotMatch(bare, /coding-session-project-crumb/);
});

test("SV-20: Details names a NIP-34 repository by its identifier, and an unfamiliar ref verbatim", () => {
  assert.equal(
    codingSessionHeaderRepoName(`30617:${"d".repeat(64)}:beekeeper`),
    "beekeeper",
  );
  assert.equal(
    codingSessionHeaderRepoName("  local-checkout  "),
    "local-checkout",
  );
  assert.equal(codingSessionHeaderRepoName(null), null);
  assert.equal(codingSessionHeaderRepoName("   "), null);
});
