import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

/**
 * SV-100: an umbrella turn block and the umbrella minimap SELECT from their
 * generation's one shared model (`codingSessionExecutionModels.ts`); they do
 * not re-derive it. These tests hold the shared path to what the local path
 * rendered, and pin the derivation count.
 */

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    IS_REACT_ACT_ENVIRONMENT: true,
    self: dom.window,
    window: dom.window,
  });
  dom.window.matchMedia = () => ({
    matches: false,
    addEventListener() {},
    removeEventListener() {},
  });
  if (!globalThis.ResizeObserver) {
    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
  }
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

const PROVIDER_SIGNER = "1958c6c4".repeat(8);
const SESSION_REF = "7d0c6a8e-4f0b-4a51-9a52-2f7c1d3e9b10";
const CHANNEL_ID = "2ecf77c7-55a8-4bef-857b-453d2ea5f8f8";
const timestamp = "2026-10-07T12:00:00.000Z";

// --- Items ------------------------------------------------------------------

function prompt(id, turnId, text = `Please do ${id}.`) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role: "user",
    title: "Brian",
    text,
    timestamp,
    turnId,
  };
}

function say(id, turnId, text) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role: "assistant",
    title: "Assistant",
    text,
    timestamp,
    turnId,
  };
}

function tool(id, turnId, { status = "completed", result = "ok" } = {}) {
  return {
    id,
    type: "tool",
    renderClass: "shell",
    descriptor: { renderClass: "shell", label: "Ran command", preview: id },
    title: "Bash",
    toolName: "Bash",
    buzzToolName: null,
    status,
    args: { command: `echo ${id}` },
    result,
    isError: false,
    timestamp,
    startedAt: timestamp,
    completedAt: status === "completed" ? timestamp : null,
    turnId,
  };
}

function turnResult(id, turnId) {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title: "Turn result",
    text: "Completed in 2s",
    timestamp,
    turnId,
    durationMs: 2_000,
    costUsd: null,
  };
}

/** A settled turn: prompt, prose, two tools, closing prose, a result. */
function settledTurn(name) {
  return [
    prompt(`${name}-prompt`, name),
    say(`${name}-opening`, name, `Looking at ${name} first.`),
    tool(`${name}-tool-1`, name),
    tool(`${name}-tool-2`, name),
    say(`${name}-answer`, name, `Finished ${name} cleanly.`),
    turnResult(`${name}-result`, name),
  ];
}

// --- Records and umbrellas --------------------------------------------------

function record(transcript, { generationId, sessionId, agentRef }) {
  return {
    generationId,
    label: "claude-agent-acp · generation 1",
    title: "SV-100",
    providerAuthorityPubkey: PROVIDER_SIGNER,
    metadataAuthorityPubkey: PROVIDER_SIGNER,
    lastEventAt: timestamp,
    lastTranscriptAt: Date.parse(timestamp),
    status: "idle",
    statusAt: null,
    statusEventId: null,
    transcript,
    conflictCount: 0,
    commandTarget: {
      driver: "claude-agent-acp",
      instanceId: "claude-instance",
      sessionId,
      generation: 1,
    },
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    provider: "claude-primary",
    runtime: "claude",
    model: "opus",
    agentRef,
    role: "lead",
    turnBudget: null,
    routing: null,
    capabilities: null,
  };
}

const SEAT_A = {
  generationId: "gen-a",
  sessionId: "11111111-1111-1111-1111-111111111111",
  agentRef: "aa25f472".repeat(8),
};
const SEAT_B = {
  generationId: "gen-b",
  sessionId: "22222222-2222-2222-2222-222222222222",
  agentRef: "bb36a583".repeat(8),
};

async function lib() {
  const [executionModels, umbrellaModel, timeline, rowGrammar, minimapItems] =
    await Promise.all([
      import("../lib/codingSessionExecutionModels.ts"),
      import("../lib/codingSessionUmbrellaModel.ts"),
      import("../lib/codingSessionUmbrellaTimeline.ts"),
      import("../lib/codingSessionMissionRowGrammar.ts"),
      import("../lib/codingSessionTranscriptMinimapItems.ts"),
    ]);
  return {
    ...executionModels,
    ...umbrellaModel,
    ...timeline,
    ...rowGrammar,
    ...minimapItems,
  };
}

async function umbrellaOf(records) {
  const { groupCodingSessionCatalog } = await lib();
  const umbrellas = groupCodingSessionCatalog(records);
  assert.equal(umbrellas.length, 1);
  return umbrellas[0];
}

/** Every generation's model, exactly as the umbrella timeline builds them. */
function modelsFor(store, umbrella) {
  const models = new Map();
  for (const execution of umbrella.executions) {
    for (const rec of [
      ...execution.priorGenerations,
      execution.activeGeneration,
    ]) {
      models.set(
        rec.generationId,
        store.model({
          record: rec,
          executionKey: execution.executionKey,
          isWorking: false,
          generationSuperseded:
            execution.activeGeneration.generationId !== rec.generationId,
        }),
      );
    }
  }
  return models;
}

function recordOf(umbrella, generationId) {
  for (const execution of umbrella.executions) {
    for (const rec of [
      ...execution.priorGenerations,
      execution.activeGeneration,
    ]) {
      if (rec.generationId === generationId) return rec;
    }
  }
  throw new Error(`no record ${generationId}`);
}

async function turnBlocks(umbrella, generationId) {
  const { buildUmbrellaTimeline } = await lib();
  return buildUmbrellaTimeline(umbrella, []).filter(
    (entry) =>
      entry.kind === "turn-block" &&
      (generationId === undefined || entry.generationId === generationId),
  );
}

async function blockProps(umbrella, block, { live }) {
  const { missionRowClass } = await lib();
  return {
    block,
    blockKey: `block:${block.generationId}:${block.blockSeq}`,
    channelId: CHANNEL_ID,
    currentUserPubkey: null,
    isHighlighted: false,
    isFolded: false,
    isWorking: false,
    label: "Lead",
    labelsByExecutionKey: new Map(),
    ...(live
      ? {
          missionExecutionBundle: true,
          missionRowClassName: missionRowClass("standard", {
            className: "border-l-2",
          }),
        }
      : {}),
    onHandoff: () => {},
    onRegisterNode: () => {},
    onRevealFact: () => {},
    operatorProfiles: undefined,
    record: recordOf(umbrella, block.generationId),
    resolveFactLocation: () => null,
    restingStatus: "stopped",
    showProvenance: true,
    stickyProvenance: false,
    umbrella,
  };
}

// --- Rendering --------------------------------------------------------------

async function mount(element) {
  const React = (await import("react")).default;
  const { render } = await import("@testing-library/react");
  const { createMemoryHistory, createRootRoute, createRouter, RouterProvider } =
    await import("@tanstack/react-router");
  const rootRoute = createRootRoute({ component: () => element });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return render(React.createElement(RouterProvider, { router }));
}

async function blockElements(propsList) {
  const React = (await import("react")).default;
  const { CodingSessionUmbrellaTurnBlock } = await import(
    "./CodingSessionUmbrellaTurnBlock.tsx"
  );
  return propsList.map((props) =>
    React.createElement(CodingSessionUmbrellaTurnBlock, {
      key: props.blockKey,
      ...props,
    }),
  );
}

async function mountBlocks(propsList) {
  const React = (await import("react")).default;
  return mount(
    React.createElement(
      "div",
      { "data-testid": "harness" },
      ...(await blockElements(propsList)),
    ),
  );
}

async function openBundles() {
  const { act } = await import("@testing-library/react");
  for (const toggle of document.querySelectorAll(
    '[data-testid="coding-session-mission-execution-bundle-toggle"]',
  )) {
    await act(async () => {
      toggle.dispatchEvent(
        new dom.window.MouseEvent("click", { bubbles: true, cancelable: true }),
      );
    });
  }
}

/** Mount, open every bundle, return the visible text; then unmount. */
async function visibleText(propsList) {
  const { cleanup } = await import("@testing-library/react");
  await mountBlocks(propsList);
  await openBundles();
  const text = document.body.textContent;
  cleanup();
  return text;
}

function count(haystack, needle) {
  return haystack.split(needle).length - 1;
}

const BUNDLE = '[data-testid="coding-session-mission-execution-bundle"]';

/** A block's narrative text (the block minus its bundle) and bundle text. */
function splitBlockText(blockNode) {
  const bundle = blockNode.querySelector(BUNDLE);
  const whole = blockNode.textContent;
  const bundleText = bundle?.textContent ?? "";
  return { narrative: whole.replace(bundleText, ""), bundle: bundleText };
}

// --- (a) shared vs local, ordinary settled turns ----------------------------

for (const live of [false, true]) {
  const lens = live ? "Mission Live" : "Conversation";
  test(`(a) ${lens}: the shared selection renders the local derivation's text`, async () => {
    const { createCodingSessionExecutionModelStore } = await lib();
    const umbrella = await umbrellaOf([
      record([...settledTurn("t1"), ...settledTurn("t2")], SEAT_A),
    ]);
    const models = modelsFor(
      createCodingSessionExecutionModelStore(),
      umbrella,
    );
    const blocks = await turnBlocks(umbrella);
    assert.equal(blocks.length, 2);
    for (const block of blocks) {
      const props = await blockProps(umbrella, block, { live });
      const local = await visibleText([props]);
      const shared = await visibleText([
        { ...props, executionModel: models.get(block.generationId) },
      ]);
      assert.ok(local.includes(`Finished ${block.turnId} cleanly.`), local);
      assert.equal(shared, local);
    }
  });
}

// --- (b) SV-91 on the audit session, through the shared model ---------------

test("(b) SV-91: with the shared model the narrative says the task, the bundle does not", async () => {
  const { createCodingSessionExecutionModelStore } = await lib();
  const { projectCodingSessionTranscript } = await import(
    "../lib/codingSessionTranscriptProjection.ts"
  );
  const { BK_AUDIT_1006_ENVELOPES } = await import(
    "./CodingSessionUmbrellaTurnBlock.background.fixture.mjs"
  );
  const sessionId = "becbd0cb-e638-4d00-a07f-a9bc08279aa5";
  const generationId = `gen-${sessionId}`;
  const transcript = projectCodingSessionTranscript(
    BK_AUDIT_1006_ENVELOPES.filter((envelope) => envelope.eventSeq <= 34),
    { channelId: CHANNEL_ID, generationId },
  );
  const umbrella = await umbrellaOf([
    record(transcript, {
      generationId,
      sessionId,
      agentRef: "aa25f472".repeat(8),
    }),
  ]);
  const models = modelsFor(createCodingSessionExecutionModelStore(), umbrella);
  const block = (await turnBlocks(umbrella)).find((entry) =>
    entry.items.some(
      (item) =>
        item.type === "message" &&
        item.text.includes("Audit run BK-AUDIT-1006"),
    ),
  );
  assert.ok(block, "a block holds the audit prompt");
  const props = await blockProps(umbrella, block, { live: true });
  await mountBlocks([{ ...props, executionModel: models.get(generationId) }]);
  await openBundles();
  const node = document.querySelector(
    '[data-testid="coding-session-umbrella-turn-block"]',
  );
  assert.ok(node.querySelector(BUNDLE), "the Bash call is in the bundle");
  const clauses = [
    ...node.querySelectorAll('[data-testid="coding-session-turn-background"]'),
  ];
  assert.equal(clauses.length, 1, "said exactly once");
  assert.ok(clauses[0].textContent.includes("1 background task running"));
  assert.equal(
    node
      .querySelector(BUNDLE)
      .querySelector('[data-testid="coding-session-turn-background"]'),
    null,
    "never in the bundle",
  );
  assert.equal(
    node.getAttribute("data-collapsed"),
    null,
    "a turn with work still going does not collapse",
  );
});

// --- (c) one derivation per generation revision ------------------------------

test("(c) N blocks plus the minimap derive a generation once; another generation's append leaves it alone", async () => {
  const React = (await import("react")).default;
  const { createCodingSessionExecutionModelStore } = await lib();
  const { useCodingSessionUmbrellaTimelineMinimapItems } = await import(
    "./CodingSessionUmbrellaTimelineViewMinimap.ts"
  );
  const store = createCodingSessionExecutionModelStore();
  const recordA = record(
    [...settledTurn("a1"), ...settledTurn("a2"), ...settledTurn("a3")],
    SEAT_A,
  );
  const recordB1 = record(settledTurn("b1"), SEAT_B);
  const umbrella1 = await umbrellaOf([recordA, recordB1]);
  assert.equal(umbrella1.executions.length, 2);

  const minimapSeen = [];
  function Minimap({ entries, executionModels }) {
    minimapSeen.push(
      useCodingSessionUmbrellaTimelineMinimapItems({
        enabled: true,
        entries,
        executionModels,
        currentUserPubkey: null,
      }),
    );
    return null;
  }
  const renderAll = async (umbrella) => {
    const models = modelsFor(store, umbrella);
    const entries = await turnBlocks(umbrella);
    const blocksA = entries.filter((entry) => entry.generationId === "gen-a");
    const propsList = await Promise.all(
      blocksA.map(async (block) => ({
        ...(await blockProps(umbrella, block, { live: true })),
        executionModel: models.get("gen-a"),
      })),
    );
    await mount(
      React.createElement(
        "div",
        null,
        React.createElement(Minimap, { entries, executionModels: models }),
        ...(await blockElements(propsList)),
      ),
    );
    return { models, blocksA };
  };

  const first = await renderAll(umbrella1);
  assert.equal(first.blocksA.length, 3);
  assert.equal(
    store.stats().derivations,
    2,
    "one per generation, not per view",
  );
  const selectionsBefore = store.stats().selections;
  assert.ok(selectionsBefore >= 6, "every block selected (narrative + bundle)");
  const modelA = first.models.get("gen-a");
  const narrativeA = first.blocksA.map((block) =>
    modelA.selectBlock(block.blockSeq, { variant: "mission-narrative" }),
  );
  // The minimap's turns are the shared model's own blocks, by reference.
  const dashes = minimapSeen.at(-1);
  assert.equal(dashes.length, 4);
  for (const block of first.blocksA) {
    const turn = modelA.selectMinimapTurn(block.blockSeq);
    assert.ok(modelA.model.blocks.includes(turn));
  }
  const textBefore = document.body.textContent;
  const { cleanup } = await import("@testing-library/react");
  cleanup();

  // B appends a turn; A's record (and its transcript array) is untouched.
  const recordB2 = record(
    [...recordB1.transcript, ...settledTurn("b2")],
    SEAT_B,
  );
  const umbrella2 = await umbrellaOf([recordA, recordB2]);
  const second = await renderAll(umbrella2);
  assert.equal(store.stats().derivations, 3, "only B re-derived");
  assert.equal(
    second.models.get("gen-a"),
    modelA,
    "A's model is the same object",
  );
  second.blocksA.forEach((block, index) => {
    assert.equal(
      modelA.selectBlock(block.blockSeq, { variant: "mission-narrative" }),
      narrativeA[index],
      "A's selections are the same objects",
    );
  });
  assert.equal(minimapSeen.at(-1).length, 5);
  assert.ok(document.body.textContent.includes("Finished a3 cleanly."));
  assert.equal(
    count(textBefore, "Finished a2 cleanly."),
    count(document.body.textContent, "Finished a2 cleanly."),
  );
});

// --- (d) a middle tool_result patch -----------------------------------------

test("(d) a tool_result patch in a middle turn updates that block's row only", async () => {
  const { createCodingSessionExecutionModelStore } = await lib();
  const store = createCodingSessionExecutionModelStore();
  const running = [
    ...settledTurn("m1"),
    prompt("m2-prompt", "m2"),
    say("m2-opening", "m2", "Running the middle job."),
    tool("m2-tool", "m2", { status: "executing", result: "" }),
    turnResult("m2-result", "m2"),
    ...settledTurn("m3"),
  ];
  const patched = running.map((item) =>
    item.id === "m2-tool"
      ? {
          ...item,
          status: "failed",
          isError: true,
          result: "exit 1",
          completedAt: timestamp,
        }
      : item,
  );
  const textOf = async (transcript) => {
    const umbrella = await umbrellaOf([record(transcript, SEAT_A)]);
    const models = modelsFor(store, umbrella);
    const blocks = await turnBlocks(umbrella);
    assert.equal(blocks.length, 3);
    const texts = [];
    const selections = [];
    for (const block of blocks) {
      const props = await blockProps(umbrella, block, { live: false });
      const model = models.get("gen-a");
      selections.push(model.selectBlock(block.blockSeq));
      texts.push(await visibleText([{ ...props, executionModel: model }]));
      // The shared path still equals the local one after the patch.
      assert.equal(texts.at(-1), await visibleText([props]));
    }
    return { texts, selections };
  };
  const before = await textOf(running);
  const after = await textOf(patched);
  assert.equal(store.stats().derivations, 2);
  assert.equal(after.texts[0], before.texts[0]);
  assert.equal(after.texts[2], before.texts[2]);
  assert.notEqual(after.texts[1], before.texts[1], "the patched row changed");
  assert.equal(
    after.selections[0],
    before.selections[0],
    "m1 kept its selection",
  );
  assert.equal(
    after.selections[2],
    before.selections[2],
    "m3 kept its selection",
  );
  assert.notEqual(after.selections[1], before.selections[1]);
});

// --- (e) Mission Live split ---------------------------------------------------

test("(e) Mission Live: every tool and prose item appears exactly once across narrative and bundle", async () => {
  const { createCodingSessionExecutionModelStore } = await lib();
  // Prose between the two calls, so each is its own row (not a tool group)
  // and the narrative has to re-join the prose the calls kept apart.
  const umbrella = await umbrellaOf([
    record(
      [
        prompt("s1-prompt", "s1"),
        say("s1-opening", "s1", "Looking at s1 first."),
        tool("s1-tool-1", "s1"),
        say("s1-middle", "s1", "Halfway through s1."),
        tool("s1-tool-2", "s1"),
        say("s1-answer", "s1", "Finished s1 cleanly."),
        turnResult("s1-result", "s1"),
      ],
      SEAT_A,
    ),
  ]);
  const models = modelsFor(createCodingSessionExecutionModelStore(), umbrella);
  const [block] = await turnBlocks(umbrella);
  const props = await blockProps(umbrella, block, { live: true });
  await mountBlocks([{ ...props, executionModel: models.get("gen-a") }]);
  await openBundles();
  const node = document.querySelector(
    '[data-testid="coding-session-umbrella-turn-block"]',
  );
  const { narrative, bundle } = splitBlockText(node);
  assert.ok(bundle.length > 0, "the bundle is open");
  for (const id of ["s1-tool-1", "s1-tool-2"]) {
    assert.equal(count(bundle, id), 1, `${id} in the bundle once`);
    assert.equal(count(narrative, id), 0, `${id} not in the narrative`);
  }
  for (const prose of [
    "Please do s1-prompt.",
    "Looking at s1 first.",
    "Halfway through s1.",
    "Finished s1 cleanly.",
  ]) {
    assert.equal(count(narrative, prose), 1, `${prose} in the narrative once`);
    assert.equal(count(bundle, prose), 0, `${prose} not in the bundle`);
  }
  // Not compared with the local path: the local bundle derives its own model
  // from the tool items alone, where the two calls are adjacent and group
  // into one "Ran 2 commands" row; the shared `mission-execution` variant
  // keeps the whole turn's entries, where prose kept them apart, so it lists
  // each call. Reported to the model owner (SV-100 view lane).
});

// --- (f) minimap dashes match the per-block derivation ------------------------

test("(f) the minimap returns the dashes the per-block derivation did", async () => {
  const React = (await import("react")).default;
  const { renderToStaticMarkup } = await import("react-dom/server");
  const {
    codingSessionUmbrellaEntryKey,
    createCodingSessionExecutionModelStore,
    deriveCodingSessionMinimapItems,
  } = await lib();
  const { deriveCodingSessionTranscriptModel } = await import(
    "../lib/codingSessionTranscriptModel.ts"
  );
  const { useCodingSessionUmbrellaTimelineMinimapItems } = await import(
    "./CodingSessionUmbrellaTimelineViewMinimap.ts"
  );
  const umbrella = await umbrellaOf([
    record(
      [
        ...settledTurn("f1"),
        // A turn nobody prompted: no dash.
        say("f2-answer", "f2", "Woke up and checked."),
        turnResult("f2-result", "f2"),
        ...settledTurn("f3"),
      ],
      SEAT_A,
    ),
    record(settledTurn("g1"), SEAT_B),
  ]);
  const { buildUmbrellaTimeline } = await lib();
  const entries = buildUmbrellaTimeline(umbrella, []);
  const models = modelsFor(createCodingSessionExecutionModelStore(), umbrella);

  // Today's algorithm, verbatim: each block's own model, its first prompted turn.
  const expectedSources = [];
  entries.forEach((entry, rowIndex) => {
    if (entry.kind !== "turn-block") return;
    const model = deriveCodingSessionTranscriptModel(entry.items, {
      isWorking: false,
    });
    const turn = model.blocks.find(
      (block) =>
        block.kind === "turn" &&
        block.entries.some(
          (candidate) =>
            candidate.kind === "item" &&
            candidate.item.type === "message" &&
            candidate.item.role === "user",
        ),
    );
    if (turn === undefined) return;
    expectedSources.push({
      key: codingSessionUmbrellaEntryKey(entry),
      rowIndex,
      turn,
      fallbackStartedAtMs: entry.timestampMs,
    });
  });
  const expected = deriveCodingSessionMinimapItems(expectedSources, null);

  let actual = null;
  function Probe() {
    actual = useCodingSessionUmbrellaTimelineMinimapItems({
      enabled: true,
      entries,
      executionModels: models,
      currentUserPubkey: null,
    });
    return null;
  }
  renderToStaticMarkup(React.createElement(Probe));
  assert.equal(actual.length, 3, "one per prompted block");
  assert.deepEqual(actual, expected);
  assert.deepEqual(
    actual.map((item) => item.key),
    expectedSources.map((source) => source.key),
  );

  // Off is off, and a generation with no model (Brief) is skipped.
  renderToStaticMarkup(
    React.createElement(function Off() {
      actual = useCodingSessionUmbrellaTimelineMinimapItems({
        enabled: true,
        entries,
        executionModels: new Map(),
        currentUserPubkey: null,
      });
      return null;
    }),
  );
  assert.deepEqual(actual, []);
});

test("(g) a split turn renders once, whole, at its first part; its settled tail claims nothing", async () => {
  const { createCodingSessionExecutionModelStore } = await lib();
  const unturned = {
    id: "mid-status",
    type: "lifecycle",
    renderClass: "status",
    title: "Provider note",
    text: "Reconnected to the relay.",
    timestamp,
  };
  const transcript = [
    prompt("p1-prompt", "p1"),
    tool("p1-tool", "p1"),
    unturned,
    say("p1-answer", "p1", "The split turn's answer."),
    turnResult("p1-result", "p1"),
  ];
  const umbrella = await umbrellaOf([record(transcript, SEAT_A)]);
  const blocks = await turnBlocks(umbrella, SEAT_A.generationId);
  assert.deepEqual(
    blocks.map((block) => block.turnId),
    ["p1", null, "p1"],
  );
  const store = createCodingSessionExecutionModelStore();
  const models = modelsFor(store, umbrella);
  const propsList = await Promise.all(
    blocks.map(async (block) => ({
      ...(await blockProps(umbrella, block, { live: false })),
      executionModel: models.get(block.generationId),
    })),
  );
  const text = await visibleText(propsList);
  assert.equal(count(text, "The split turn's answer."), 1);
  assert.equal(count(text, "No conversation yet"), 0, "no false empty state");
  // The tail renders nothing of its own, working or not: its turn — and that
  // turn's working line — is on screen at the first part.
  assert.equal(await visibleText([propsList[2]]), "");
  assert.equal(await visibleText([{ ...propsList[2], isWorking: true }]), "");
});

test("(h) interleaved turns: the working line stays on the working block's turn, not a queued later one", async () => {
  const { createCodingSessionExecutionModelStore } = await lib();
  // A runs; a queued prompt gets its own turn B; A keeps going. The working
  // block is the last one (A's tail), so A is the running turn.
  const transcript = [
    prompt("a-prompt", "A"),
    tool("a-tool", "A", { status: "running", result: "" }),
    prompt("b-prompt", "B", "Queued follow-up."),
    say("a-more", "A", "Still working on A."),
  ];
  const rec = { ...record(transcript, SEAT_A), status: "running" };
  const umbrella = await umbrellaOf([rec]);
  const blocks = await turnBlocks(umbrella);
  assert.deepEqual(
    blocks.map((block) => block.turnId),
    ["A", "B", "A"],
  );
  const last = blocks.length - 1;
  const execution = umbrella.executions[0];
  // Exactly what the timeline view hands the store: the working block's turn.
  const executionModel = createCodingSessionExecutionModelStore().model({
    record: execution.activeGeneration,
    executionKey: execution.executionKey,
    isWorking: true,
    workingTurnId: blocks[last].turnId,
    generationSuperseded: false,
  });
  const turnA = executionModel.model.blocks.find((block) => block.id === "A");
  const turnB = executionModel.model.blocks.find((block) => block.id === "B");
  assert.equal(turnA.isWorking, true, "A is the running turn");
  assert.equal(turnB.isWorking, false, "the queued turn is not working");
  const texts = [];
  for (const [index, block] of blocks.entries()) {
    texts.push(
      await visibleText([
        {
          ...(await blockProps(umbrella, block, { live: false })),
          isWorking: index === last,
          restingStatus: "running",
          executionModel,
        },
      ]),
    );
  }
  assert.match(texts[0], /Still working on A\./);
  assert.match(texts[0], /Working for/, "A carries the working line");
  assert.doesNotMatch(texts[1], /Working for|Thinking/, "B is not working");
  assert.equal(texts[2], "", "A's tail renders nothing a second time");
});
