import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";

import {
  deriveCodingSessionBlockBackgroundTasks,
  deriveCodingSessionTranscriptModel,
} from "../lib/codingSessionTranscriptModel.ts";
import { projectCodingSessionTranscript } from "../lib/codingSessionTranscriptProjection.ts";
import { groupCodingSessionCatalog } from "../lib/codingSessionUmbrellaModel.ts";
import { buildUmbrellaTimeline } from "../lib/codingSessionUmbrellaTimeline.ts";
import { BK_AUDIT_1006_ENVELOPES } from "./CodingSessionUmbrellaTurnBlock.background.fixture.mjs";
import { missionRowClass } from "../lib/codingSessionMissionRowGrammar.ts";
import { CodingSessionUmbrellaTimelineView } from "./CodingSessionUmbrellaTimelineView.tsx";
import { CodingSessionUmbrellaTurnBlock } from "./CodingSessionUmbrellaTurnBlock.tsx";

/**
 * SV-91 / SV-93 (ledger 342), on the audit session's own published events.
 *
 * SV-78's clause was right offline and absent live: the umbrella renders one
 * turn block at a time, Mission Live moves the block's tool items — the Bash
 * result announcing the task — into the execution bundle, and the
 * `autonomous_turn…` row that answers it lives in the next block. The clause
 * now comes from the whole generation, in both lenses; and the turn the agent
 * began on its own opens on a quiet marker read from those status rows.
 */

const PROVIDER_SIGNER = "1958c6c4".repeat(8);
const LEAD_ACTOR = "aa25f472".repeat(8);
const SESSION_REF = "b2cb2444-e177-46c6-8429-be5305cf9a59";
const CHANNEL_ID = "2ecf77c7-55a8-4bef-857b-453d2ea5f8f8";
const SESSION_ID = "becbd0cb-e638-4d00-a07f-a9bc08279aa5";
const GENERATION_ID = `gen-${SESSION_ID}`;

const RUNNING = "1 background task running";
const WOKE = "1 background task, then the agent woke on its own";

/** The envelopes published up to and including `lastSeq`. */
function envelopesThrough(lastSeq) {
  return BK_AUDIT_1006_ENVELOPES.filter(
    (envelope) => envelope.eventSeq <= lastSeq,
  );
}

function transcriptThrough(lastSeq) {
  return projectCodingSessionTranscript(envelopesThrough(lastSeq), {
    channelId: CHANNEL_ID,
    generationId: GENERATION_ID,
  });
}

function umbrellaThrough(lastSeq, generation = 1) {
  const transcript = transcriptThrough(lastSeq);
  const umbrellas = groupCodingSessionCatalog([
    {
      generationId: GENERATION_ID,
      label: "claude-agent-acp · generation 1",
      title: "BK-AUDIT-1006",
      providerAuthorityPubkey: PROVIDER_SIGNER,
      metadataAuthorityPubkey: PROVIDER_SIGNER,
      lastEventAt: transcript.at(-1)?.timestamp ?? "2026-10-06T00:53:41.000Z",
      status: "idle",
      statusAt: null,
      transcript,
      conflictCount: 0,
      commandTarget: {
        driver: "claude-agent-acp",
        instanceId: "1958c6c448e05eed",
        sessionId: SESSION_ID,
        generation,
      },
      projectRef: null,
      repoRef: null,
      sessionRef: SESSION_REF,
      provider: "claude-primary",
      runtime: "claude",
      model: "opus",
      agentRef: LEAD_ACTOR,
      role: "lead",
      turnBudget: null,
      capabilities: null,
    },
  ]);
  assert.equal(umbrellas.length, 1);
  return { umbrella: umbrellas[0], transcript };
}

async function renderInRouter(element) {
  const rootRoute = createRootRoute({ component: () => element });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

function renderTimeline(umbrella, missionDensity) {
  return renderInRouter(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      channelId: CHANNEL_ID,
      laneMessages: [],
      missionDensity,
      onHandoff: () => {},
      umbrella,
    }),
  );
}

/**
 * One block as Mission Live renders it once a reader has opened it: the
 * execution bundle on, settled collapse off (a static render cannot click).
 */
function renderMissionBlockOpened(umbrella, text) {
  const block = buildUmbrellaTimeline(umbrella, []).find(
    (entry) =>
      entry.kind === "turn-block" &&
      entry.items.some(
        (item) => item.type === "message" && item.text.includes(text),
      ),
  );
  assert.ok(block, `a block holds "${text}"`);
  return renderInRouter(
    React.createElement(CodingSessionUmbrellaTurnBlock, {
      block,
      blockKey: "block-under-test",
      channelId: CHANNEL_ID,
      currentUserPubkey: null,
      isHighlighted: false,
      isFolded: false,
      isWorking: false,
      label: "Lead",
      labelsByExecutionKey: new Map(),
      missionExecutionBundle: true,
      missionRowClassName: missionRowClass("standard", {
        className: "border-l-2",
      }),
      onHandoff: () => {},
      onRegisterNode: () => {},
      onRevealFact: () => {},
      operatorProfiles: undefined,
      record: umbrella.executions[0].activeGeneration,
      resolveFactLocation: () => null,
      restingStatus: "stopped",
      showProvenance: false,
      stickyProvenance: false,
      umbrella,
    }),
  );
}

/** Each turn block's markup, in render order. */
function blocks(markup) {
  return markup
    .split('data-testid="coding-session-umbrella-turn-block"')
    .slice(1);
}

/** The block of the prompted turn that started the background task. */
function startingBlock(markup) {
  const found = blocks(markup).filter((block) =>
    block.includes("Audit run BK-AUDIT-1006"),
  );
  assert.equal(found.length, 1, "one block holds the audit prompt");
  return found[0];
}

function backgroundClauses(markup) {
  return [
    ...markup.matchAll(
      /data-testid="coding-session-turn-background"[^>]*>(.*?)<\/span>/g,
    ),
  ].map((match) => match[1].replace(/<[^>]+>/g, "").replace(/^·/, ""));
}

const LENSES = [
  ["Mission Live", "live"],
  ["Conversation", undefined],
];

test("SV-91 fixture: the published events carry the task and its wake rows", () => {
  const transcript = transcriptThrough(61);
  const bash = transcript.find(
    (item) =>
      item.type === "tool" &&
      item.result.includes("Command running in background with ID: bi9cros3k"),
  );
  assert.ok(bash, "seq 31's result is the Bash tool's");
  const statuses = transcript
    .filter((item) => item.type === "lifecycle" && item.title === "Status")
    .map((item) => item.text);
  assert.deepEqual(
    statuses.filter((text) => text.startsWith("autonomous_turn")),
    [
      "autonomous_turn_started: the agent began a turn nobody prompted",
      "autonomous_turn: the agent woke on task-notification",
    ],
  );
  assert.ok(
    !transcript.some(
      (item) =>
        item.type === "message" && item.text.includes("<task-notification>"),
    ),
    "0.84.0 sent no task-notification prompt — the reason SV-93 reads the rows",
  );
});

for (const [lens, missionDensity] of LENSES) {
  test(`SV-91 ${lens}: the ended turn says its background task is running`, async () => {
    // Through seq 34: the turn's result is in, the 60-second job is not.
    const { umbrella } = umbrellaThrough(34);
    const markup = await renderTimeline(umbrella, missionDensity);
    assert.deepEqual(backgroundClauses(markup), [RUNNING]);
    assert.ok(
      startingBlock(markup).includes(RUNNING),
      "on the block that started it",
    );
    assert.ok(
      !markup.includes('data-collapsed="true"'),
      "a turn with work still going does not collapse to one line",
    );
    assert.ok(!markup.includes("coding-session-autonomous-wake"));
  });

  test(`SV-91 ${lens}: the clause changes when the next block's wake row arrives`, async () => {
    // Seq 35 is the next turn's `autonomous_turn_started` row — the first
    // item of a *different* block.
    const { umbrella } = umbrellaThrough(35);
    const markup = await renderTimeline(umbrella, missionDensity);
    assert.ok(!markup.includes(RUNNING), "never 'running' once it woke");
    if (missionDensity === "live") {
      // Nothing outstanding any more, so Live may fold the settled turn to
      // one line again; opened, it says what happened.
      assert.ok(markup.includes('data-collapsed="true"'));
      const opened = await renderMissionBlockOpened(
        umbrella,
        "Audit run BK-AUDIT-1006",
      );
      assert.deepEqual(backgroundClauses(opened), [WOKE]);
      assert.ok(opened.includes("coding-session-mission-execution-bundle"));
      return;
    }
    assert.deepEqual(backgroundClauses(markup), [WOKE]);
    assert.ok(startingBlock(markup).includes(WOKE));
  });

  test(`SV-93 ${lens}: the self-started turn opens on 'Woke on its own'`, async () => {
    // Through seq 51: started, answered, but the task-notification row
    // (seq 52) has not landed.
    const before = await renderTimeline(
      umbrellaThrough(51).umbrella,
      missionDensity,
    );
    const wakeBlocks = blocks(before).filter((block) =>
      block.includes('data-testid="coding-session-autonomous-wake"'),
    );
    assert.equal(wakeBlocks.length, 1, "one self-started turn, one marker");
    assert.ok(wakeBlocks[0].includes("Woke on its own"));
    assert.ok(
      !wakeBlocks[0].includes("coding-session-autonomous-wake-cause"),
      "no cause until a row names one",
    );
    assert.ok(
      wakeBlocks[0].indexOf("coding-session-autonomous-wake") <
        wakeBlocks[0].indexOf("Background job finished."),
      "the marker opens the turn, before its first words",
    );

    // Through seq 61: seq 52 named the task notification.
    const after = await renderTimeline(
      umbrellaThrough(61).umbrella,
      missionDensity,
    );
    if (missionDensity === "live") {
      // Settled, Live folds the turn to one line; the line keeps its origin.
      assert.match(
        after,
        /data-testid="coding-session-umbrella-collapsed-autonomous-wake"[^>]*>Woke on its own · background task<\/span>/,
      );
      const opened = await renderMissionBlockOpened(
        umbrellaThrough(61).umbrella,
        "Background job finished.",
      );
      assert.match(
        opened,
        /Woke on its own<\/span><span[^>]*data-testid="coding-session-autonomous-wake-cause"[^>]*>· background task<\/span>/,
      );
      return;
    }
    assert.deepEqual(backgroundClauses(after), [WOKE]);
    const [wake] = blocks(after).filter((block) =>
      block.includes('data-testid="coding-session-autonomous-wake"'),
    );
    assert.ok(wake);
    assert.match(
      wake,
      /Woke on its own<\/span><span[^>]*data-testid="coding-session-autonomous-wake-cause"[^>]*>· background task<\/span>/,
    );
    assert.equal(
      [...after.matchAll(/data-testid="coding-session-autonomous-wake"/g)]
        .length,
      1,
      "the prompted turn after it has no marker",
    );
  });
}

test("SV-91 Mission Live: the clause is said once, not again in the bundle", async () => {
  const { umbrella } = umbrellaThrough(34);
  const entries = buildUmbrellaTimeline(umbrella, []);
  const block = entries.find((entry) => entry.kind === "turn-block");
  assert.ok(block);
  const markup = await renderTimeline(umbrella, "live");
  assert.equal(backgroundClauses(markup).length, 1);
  assert.ok(
    markup.includes("coding-session-mission-execution-bundle"),
    "the Bash call did move into the bundle",
  );
});

test("SV-91: the block derivation reads the whole generation, keyed by turn", () => {
  const transcript = transcriptThrough(61);
  const blockItems = transcript.filter(
    (item) => item.turnId === "5be72eae-de29-4154-8c64-d91b91203116",
  );
  const byTurn = deriveCodingSessionBlockBackgroundTasks({
    transcript,
    blockItems,
    generationSuperseded: false,
  });
  assert.deepEqual(
    [...byTurn.entries()],
    [
      [
        "5be72eae-de29-4154-8c64-d91b91203116",
        [{ id: "bi9cros3k", state: "woke", status: null }],
      ],
    ],
  );
  // The bug, kept as a witness: the block's items alone never see the wake.
  const alone = deriveCodingSessionTranscriptModel(blockItems, {
    isWorking: false,
  });
  assert.deepEqual(
    alone.blocks.find((entry) => entry.kind === "turn").backgroundTasks,
    [{ id: "bi9cros3k", state: "running", status: null }],
  );
  // A block that started nothing gets nothing.
  const later = deriveCodingSessionBlockBackgroundTasks({
    transcript,
    blockItems: transcript.filter(
      (item) => item.turnId === "7bebdbef-8a4b-4b29-ac3c-015dd85afee7",
    ),
    generationSuperseded: false,
  });
  assert.equal(later.size, 0);
});

test("SV-91: a superseded generation's unanswered task never reads running", () => {
  const transcript = transcriptThrough(34);
  const byTurn = deriveCodingSessionBlockBackgroundTasks({
    transcript,
    blockItems: transcript,
    generationSuperseded: true,
  });
  assert.deepEqual(byTurn.get("5be72eae-de29-4154-8c64-d91b91203116"), [
    { id: "bi9cros3k", state: "unreported", status: null },
  ]);
});

test("SV-91: an explicit map is the whole truth for the model", () => {
  const transcript = transcriptThrough(34);
  const model = deriveCodingSessionTranscriptModel(
    transcript.filter((item) => item.type !== "tool"),
    {
      isWorking: false,
      backgroundTasksByTurn: new Map([
        [
          "5be72eae-de29-4154-8c64-d91b91203116",
          [{ id: "bi9cros3k", state: "running", status: null }],
        ],
      ]),
    },
  );
  const turn = model.blocks.find((entry) => entry.kind === "turn");
  assert.deepEqual(turn.backgroundTasks, [
    { id: "bi9cros3k", state: "running", status: null },
  ]);
  // An empty map silences a fragment that does hold the tool (the bundle).
  const silenced = deriveCodingSessionTranscriptModel(transcript, {
    isWorking: false,
    backgroundTasksByTurn: new Map(),
  });
  assert.deepEqual(
    silenced.blocks.find((entry) => entry.kind === "turn").backgroundTasks,
    [],
  );
});
