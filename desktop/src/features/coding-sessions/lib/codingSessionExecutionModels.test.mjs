import assert from "node:assert/strict";
import test from "node:test";

import {
  createCodingSessionExecutionModelStore,
  isCodingSessionRehydrationClaimItem,
} from "./codingSessionExecutionModels.ts";
import { CODING_SESSION_CONTINUITY_STATUSES } from "./codingSessionTranscriptItems.ts";
import {
  deriveCodingSessionBlockBackgroundTasks,
  deriveCodingSessionTranscriptModel,
} from "./codingSessionTranscriptModel.ts";
import { projectCodingSessionTranscript } from "./codingSessionTranscriptProjection.ts";
import { groupTranscriptIntoTurnBlocks } from "./codingSessionUmbrellaTimeline.ts";
import { BK_AUDIT_1006_ENVELOPES } from "../ui/CodingSessionUmbrellaTurnBlock.background.fixture.mjs";

// SV-100: one model per generation, selected by every view that shows it.

const timestamp = "2026-10-07T12:00:00.000Z";
const EXECUTION_KEY = "execution-a";

function prompt(id, turnId) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role: "user",
    title: "Brian",
    text: `prompt ${id}`,
    timestamp,
    ...(turnId === null ? {} : { turnId }),
  };
}

function say(id, turnId, text = `answer ${id}`) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role: "assistant",
    title: "Assistant",
    text,
    timestamp,
    ...(turnId === null ? {} : { turnId }),
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
    args: { command: id },
    result,
    isError: false,
    timestamp,
    startedAt: timestamp,
    completedAt: status === "completed" ? timestamp : null,
    turnId,
  };
}

function result(id, turnId) {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title: "Turn result",
    text: "Done. (1200ms)",
    timestamp,
    turnId,
  };
}

/** A whole settled turn: prompt, two tools, an answer, a result. */
function turn(name) {
  return [
    prompt(`${name}-prompt`, name),
    tool(`${name}-tool-1`, name),
    tool(`${name}-tool-2`, name),
    say(`${name}-answer`, name, "Done."),
    result(`${name}-result`, name),
  ];
}

function record(transcript, generationId = "gen-a", generation = 1) {
  return {
    generationId,
    label: `claude-agent-acp · generation ${generation}`,
    title: "SV-100",
    providerAuthorityPubkey: "a".repeat(64),
    metadataAuthorityPubkey: "a".repeat(64),
    lastEventAt: timestamp,
    status: "running",
    transcript,
    conflictCount: 0,
    commandTarget: {
      driver: "claude-agent-acp",
      instanceId: "instance-a",
      sessionId: `session-${generationId}`,
      generation,
    },
    projectRef: null,
    repoRef: null,
    sessionRef: null,
    provider: "claude-primary",
    runtime: "claude",
    model: null,
    capabilities: null,
  };
}

function input(rec, { isWorking = false, generationSuperseded = false } = {}) {
  return {
    record: rec,
    executionKey: EXECUTION_KEY,
    isWorking,
    generationSuperseded,
  };
}

/** The `blockSeq` of the first timeline block of `turnId`. */
function seqOf(rec, turnId) {
  const block = groupTranscriptIntoTurnBlocks(rec, EXECUTION_KEY).find(
    (candidate) => candidate.turnId === turnId,
  );
  assert.ok(block, `a block of ${turnId}`);
  return block.blockSeq;
}

function onlyTurn(selection) {
  assert.equal(selection.blocks.length, 1);
  const [block] = selection.blocks;
  assert.equal(block.kind, "turn");
  return block;
}

const VARIANTS = ["whole", "mission-narrative", "mission-execution"];

function allSelections(published, blockSeq) {
  return VARIANTS.map((variant) =>
    published.selectBlock(blockSeq, { variant }),
  );
}

// --- The SV-91 audit session, as the provider published it ------------------

const AUDIT_SESSION = "becbd0cb-e638-4d00-a07f-a9bc08279aa5";
const AUDIT_GENERATION = `gen-${AUDIT_SESSION}`;
const AUDIT_FIRST_TURN = "5be72eae-de29-4154-8c64-d91b91203116";

function auditRecord(lastSeq) {
  const transcript = projectCodingSessionTranscript(
    BK_AUDIT_1006_ENVELOPES.filter((envelope) => envelope.eventSeq <= lastSeq),
    { channelId: "channel-audit", generationId: AUDIT_GENERATION },
  );
  return record(transcript, AUDIT_GENERATION);
}

function entryItemIds(entries) {
  return entries.flatMap((entry) => {
    if (entry.kind === "item") return [entry.item.id];
    if (entry.kind === "tool-group") return entry.items.map((item) => item.id);
    return entry.spawns.map((spawn) => spawn.call.id);
  });
}

function proseOf(entries) {
  return entries
    .filter((entry) => entry.kind === "item" && entry.item.type !== "tool")
    .map((entry) => entry.item.text)
    .join("")
    .replace(/\s+/g, "");
}

// --- (a) -------------------------------------------------------------------

test("(a) one generation asked for by several consumers derives once", () => {
  const store = createCodingSessionExecutionModelStore();
  const rec = record([...turn("t1"), ...turn("t2")]);
  const first = store.model(input(rec));
  const views = [store.model(input(rec)), store.model(input(rec))];
  for (const published of views) assert.equal(published, first);
  for (const published of [first, ...views]) {
    for (let seq = 0; seq < 2; seq += 1) {
      allSelections(published, seq);
      published.selectMinimapTurn(seq);
    }
  }
  const again = first.selectBlock(0);
  assert.equal(again, first.selectBlock(0, { variant: "whole" }));
  const stats = store.stats();
  assert.equal(stats.derivations, 1);
  assert.equal(stats.requests, 3);
  assert.equal(stats.hits, 2);
  // 18 variant reads + 2 more; only the first read of each key computes.
  assert.equal(stats.selections, 20);
  assert.equal(stats.selectionHits, 14);
  assert.equal(first.selectMinimapTurn(1)?.id, "t2");
  assert.deepEqual(first.blockItems(1), rec.transcript.slice(5));
});

// --- (b) -------------------------------------------------------------------

test("(b) an event in generation A never re-derives generation B", () => {
  const store = createCodingSessionExecutionModelStore();
  const a1 = record(turn("a1"), "gen-a");
  const b = record(turn("b1"), "gen-b");
  const modelA = store.model(input(a1));
  const modelB = store.model(input(b));
  const selectionsB = allSelections(modelB, 0);
  const minimapB = modelB.selectMinimapTurn(0);

  const a2 = record([...a1.transcript, ...turn("a2")], "gen-a");
  const nextA = store.model(input(a2));
  const nextB = store.model(input(b));
  assert.notEqual(nextA, modelA);
  assert.equal(nextB, modelB);
  assert.deepEqual(allSelections(nextB, 0), selectionsB);
  allSelections(nextB, 0).forEach((selection, index) => {
    assert.equal(selection, selectionsB[index]);
  });
  assert.equal(nextB.selectMinimapTurn(0), minimapB);
  assert.equal(store.stats().derivations, 3);

  store.retain(["gen-b"]);
  assert.equal(store.stats().evicted, 1);
  assert.equal(store.model(input(b)), modelB, "B survives retain");
  store.reset();
  assert.equal(store.stats().evicted, 2);
});

// --- (c) -------------------------------------------------------------------

test("(c) an isWorking flip on the same array re-derives the working turn", () => {
  const store = createCodingSessionExecutionModelStore();
  const rec = record([
    ...turn("t1"),
    prompt("t2-prompt", "t2"),
    tool("t2-tool", "t2", { status: "executing", result: "" }),
  ]);
  const working = store.model(input(rec, { isWorking: true }));
  const idle = store.model(input(rec, { isWorking: false }));
  assert.equal(store.stats().derivations, 2);
  assert.notEqual(idle, working);
  const seq = seqOf(rec, "t2");
  assert.equal(onlyTurn(working.selectBlock(seq)).isWorking, true);
  assert.equal(onlyTurn(idle.selectBlock(seq)).isWorking, false);
  assert.equal(
    onlyTurn(working.selectBlock(seq, { variant: "mission-execution" }))
      .isWorking,
    true,
  );
  // The settled turn is untouched by the flip.
  assert.equal(idle.selectBlock(0), working.selectBlock(0));
});

test("(c) a generationSuperseded flip turns a running task unreported", () => {
  const store = createCodingSessionExecutionModelStore();
  const rec = auditRecord(34);
  const seq = seqOf(rec, AUDIT_FIRST_TURN);
  const current = store.model(input(rec));
  const stale = store.model(input(rec, { generationSuperseded: true }));
  assert.equal(store.stats().derivations, 2);
  assert.deepEqual(
    onlyTurn(current.selectBlock(seq)).backgroundTasks.map((t) => t.state),
    ["running"],
  );
  assert.deepEqual(
    onlyTurn(stale.selectBlock(seq)).backgroundTasks.map((t) => t.state),
    ["unreported"],
  );
  assert.equal(
    current.backgroundTasksByTurn.get(AUDIT_FIRST_TURN)[0].state,
    "running",
  );
  assert.equal(
    stale.backgroundTasksByTurn.get(AUDIT_FIRST_TURN)[0].state,
    "unreported",
  );
});

// --- (d) -------------------------------------------------------------------

test("(d) a middle tool_result patch changes only the selection holding it", () => {
  const store = createCodingSessionExecutionModelStore();
  const items = [...turn("t1"), ...turn("t2"), ...turn("t3")];
  items[6] = tool("t2-tool-1", "t2", { status: "executing", result: "" });
  const before = store.model(input(record(items)));
  const old = [0, 1, 2].map((seq) => allSelections(before, seq));

  const patched = [...items];
  patched[6] = tool("t2-tool-1", "t2", { result: "patched" });
  const after = store.model(input(record(patched)));
  const next = [0, 1, 2].map((seq) => allSelections(after, seq));
  for (const seq of [0, 2]) {
    next[seq].forEach((selection, index) => {
      assert.equal(selection, old[seq][index], `block ${seq} kept`);
    });
  }
  assert.notEqual(next[1][0], old[1][0], "the patched block's whole changed");
  assert.notEqual(next[1][2], old[1][2], "and its execution half");
  assert.equal(after.model.blocks[0], before.model.blocks[0]);
  assert.equal(after.model.blocks[2], before.model.blocks[2]);
});

// --- (e) -------------------------------------------------------------------

test("(e) SV-91: turn 1's task reads as woke from a later turn's row, in both halves", () => {
  const store = createCodingSessionExecutionModelStore();
  const rec = auditRecord(61);
  const published = store.model(input(rec));
  const seq = seqOf(rec, AUDIT_FIRST_TURN);
  const whole = onlyTurn(published.selectBlock(seq));
  const narrative = onlyTurn(
    published.selectBlock(seq, { variant: "mission-narrative" }),
  );
  const execution = onlyTurn(
    published.selectBlock(seq, { variant: "mission-execution" }),
  );
  assert.deepEqual(whole.backgroundTasks, [
    { id: "bi9cros3k", state: "woke", status: null },
  ]);
  assert.equal(narrative.backgroundTasks, whole.backgroundTasks);
  assert.deepEqual(execution.backgroundTasks, []);
  assert.equal(execution.completion, null);
  assert.ok(
    execution.entries.some((entry) =>
      entryItemIds([entry]).some((id) =>
        rec.transcript.some(
          (item) =>
            item.id === id &&
            item.type === "tool" &&
            item.result.includes("bi9cros3k"),
        ),
      ),
    ),
    "the announcing Bash call is in the execution half",
  );
  assert.ok(
    !narrative.entries.some(
      (entry) => entry.kind !== "item" || entry.item.type === "tool",
    ),
  );
});

// --- (f) -------------------------------------------------------------------

test("(f) a turn with no completion followed by a later turn is superseded", () => {
  const store = createCodingSessionExecutionModelStore();
  const rec = record([
    prompt("t1-prompt", "t1"),
    say("t1-answer", "t1"),
    prompt("t2-prompt", "t2"),
  ]);
  const published = store.model(input(rec, { isWorking: true }));
  const earlier = onlyTurn(published.selectBlock(0));
  assert.equal(earlier.superseded, true);
  assert.equal(earlier.isWorking, false);
  assert.equal(
    onlyTurn(published.selectBlock(0, { variant: "mission-narrative" }))
      .superseded,
    true,
  );
  assert.equal(onlyTurn(published.selectBlock(1)).isWorking, true);
});

// --- (g) -------------------------------------------------------------------

test("(g) a late earlier item lands in the right selection", () => {
  const store = createCodingSessionExecutionModelStore();
  const base = [
    prompt("t1-prompt", "t1"),
    say("t1-answer", "t1"),
    result("t1-result", "t1"),
    ...turn("t2"),
  ];
  const before = store.model(input(record(base)));
  const t2Before = before.selectBlock(1);

  const late = [base[0], tool("t1-late-tool", "t1"), ...base.slice(1)];
  const after = store.model(input(record(late)));
  const t1 = onlyTurn(after.selectBlock(0));
  assert.ok(entryItemIds(t1.entries).includes("t1-late-tool"));
  assert.equal(after.selectBlock(1), t2Before, "t2's selection kept");

  // An unturned row arriving first shifts every blockSeq by one.
  const shifted = [say("loose", null), ...late];
  const third = store.model(input(record(shifted)));
  const loose = third.selectBlock(0);
  assert.equal(loose.blocks.length, 1);
  assert.equal(loose.blocks[0].kind, "standalone");
  assert.equal(onlyTurn(third.selectBlock(1)).id, "t1");
  assert.equal(third.selectBlock(2).blocks[0], t2Before.blocks[0]);
  assert.equal(third.blockItems(2)[0].id, "t2-prompt");
});

// --- (h) -------------------------------------------------------------------

test("(h) a split turn renders whole at its first block", () => {
  const store = createCodingSessionExecutionModelStore();
  const rec = record([
    prompt("p", "T"),
    say("a1", "T", "first half"),
    say("loose", null, "an unturned row"),
    tool("tool", "T"),
    say("a2", "T", "Done."),
    result("r", "T"),
  ]);
  const blocks = groupTranscriptIntoTurnBlocks(rec, EXECUTION_KEY);
  assert.deepEqual(
    blocks.map((block) => block.turnId),
    ["T", null, "T"],
  );
  const published = store.model(input(rec));
  const whole = onlyTurn(published.selectBlock(0));
  assert.deepEqual(entryItemIds(whole.entries), ["p", "a1", "tool", "a2"]);
  assert.notEqual(whole.completion, null);
  assert.equal(published.selectBlock(1).blocks[0].kind, "standalone");
  for (const variant of VARIANTS) {
    assert.equal(published.selectBlock(2, { variant }).blocks.length, 0);
  }
  assert.equal(published.selectMinimapTurn(2), null);
  assert.equal(published.selectMinimapTurn(0), whole);
});

// --- (i) -------------------------------------------------------------------

test("(i) appending to the last turn keeps every earlier selection", () => {
  const store = createCodingSessionExecutionModelStore();
  const items = [...turn("t1"), ...turn("t2"), prompt("t3-prompt", "t3")];
  const before = store.model(input(record(items), { isWorking: true }));
  const old = [0, 1, 2].map((seq) => allSelections(before, seq));
  const hitsBefore = store.stats().selectionHits;

  const after = store.model(
    input(record([...items, say("t3-answer", "t3")]), { isWorking: true }),
  );
  const next = [0, 1, 2].map((seq) => allSelections(after, seq));
  for (const seq of [0, 1]) {
    next[seq].forEach((selection, index) => {
      assert.equal(selection, old[seq][index]);
    });
  }
  assert.notEqual(next[2][0], old[2][0]);
  // Six kept selections, plus t3's execution half: empty before and after.
  assert.equal(store.stats().selectionHits - hitsBefore, 7);
});

// --- (j) -------------------------------------------------------------------

test("(j) published models and selections are frozen and never edited", () => {
  const store = createCodingSessionExecutionModelStore();
  const items = [...turn("t1"), prompt("t2-prompt", "t2")];
  const first = store.model(input(record(items), { isWorking: true }));
  const selections = [0, 1].flatMap((seq) => allSelections(first, seq));
  const snapshot = JSON.stringify([first.model, selections]);

  assert.ok(Object.isFrozen(first));
  assert.ok(Object.isFrozen(first.model));
  assert.ok(Object.isFrozen(first.model.blocks));
  for (const selection of selections) {
    assert.ok(Object.isFrozen(selection));
    assert.ok(Object.isFrozen(selection.blocks));
    assert.ok(Object.isFrozen(selection.diagnostics));
    for (const block of selection.blocks) {
      assert.ok(Object.isFrozen(block));
      if (block.kind === "turn") assert.ok(Object.isFrozen(block.entries));
    }
  }
  assert.throws(() => {
    first.model.blocks.push(null);
  }, TypeError);

  let transcript = items;
  for (const next of [
    say("t2-a", "t2"),
    tool("t2-t", "t2"),
    result("t2-r", "t2"),
  ]) {
    transcript = [...transcript, next];
    const published = store.model(
      input(record(transcript), { isWorking: true }),
    );
    for (let seq = 0; seq < 2; seq += 1) allSelections(published, seq);
  }
  store.model(input(record(transcript), { isWorking: false }));
  assert.equal(JSON.stringify([first.model, selections]), snapshot);
});

// --- (k) -------------------------------------------------------------------

test("(k) every unsplit turn block's whole selection is the full model's turn", () => {
  for (const rec of [
    auditRecord(61),
    auditRecord(34),
    record([...turn("t1"), ...turn("t2")]),
  ]) {
    for (const isWorking of [false, true]) {
      const store = createCodingSessionExecutionModelStore();
      const published = store.model(input(rec, { isWorking }));
      const oracle = deriveCodingSessionTranscriptModel(rec.transcript, {
        isWorking,
        backgroundTasksByTurn: deriveCodingSessionBlockBackgroundTasks({
          transcript: rec.transcript,
          blockItems: rec.transcript,
          generationSuperseded: false,
        }),
      });
      const blocks = groupTranscriptIntoTurnBlocks(rec, EXECUTION_KEY);
      let checked = 0;
      for (const block of blocks) {
        if (block.turnId === null) continue;
        if (
          blocks.filter((other) => other.turnId === block.turnId).length > 1
        ) {
          continue;
        }
        const expected = oracle.blocks.find(
          (candidate) =>
            candidate.kind === "turn" && candidate.id === block.turnId,
        );
        if (!expected) continue;
        const selected = onlyTurn(published.selectBlock(block.blockSeq));
        assert.deepEqual(selected, expected);
        checked += 1;
      }
      assert.ok(checked >= 1, "a turn compared");
    }
  }
});

// --- (l) -------------------------------------------------------------------

test("(l) narrative and execution together hold every item of the turn once", () => {
  const store = createCodingSessionExecutionModelStore();
  for (const rec of [auditRecord(61), record([...turn("t1"), ...turn("t2")])]) {
    const published = store.model(input(rec));
    for (const block of published.model.blocks) {
      if (block.kind !== "turn") continue;
      const seq = seqOfModelTurn(rec, block.id);
      const whole = onlyTurn(published.selectBlock(seq));
      assert.equal(whole, block);
      const narrative = published
        .selectBlock(seq, { variant: "mission-narrative" })
        .blocks.find((candidate) => candidate.id === block.id);
      const execution = published
        .selectBlock(seq, { variant: "mission-execution" })
        .blocks.find((candidate) => candidate.id === block.id);
      assert.ok(narrative, "the narrative keeps every turn");

      const toolIds = entryItemIds(
        whole.entries.filter(
          (entry) => entry.kind !== "item" || entry.item.type === "tool",
        ),
      );
      assert.deepEqual(entryItemIds(execution?.entries ?? []), toolIds);
      const narrativeIds = entryItemIds(narrative.entries);
      const all = [...narrativeIds, ...toolIds];
      assert.equal(new Set(all).size, all.length, "nothing twice");
      const wholeIds = new Set(entryItemIds(whole.entries));
      for (const id of narrativeIds) assert.ok(wholeIds.has(id));
      assert.equal(
        proseOf(narrative.entries),
        proseOf(whole.entries),
        "no prose lost",
      );

      for (const fact of [
        "completion",
        "backgroundTasks",
        "superseded",
        "isWorking",
        "startedAt",
        "autonomousWake",
        "diagnostics",
      ]) {
        assert.equal(narrative[fact], whole[fact], fact);
      }
      assert.deepEqual(narrative.changedFiles, []);
      if (execution) {
        assert.equal(execution.changedFiles, whole.changedFiles);
        assert.equal(execution.isWorking, whole.isWorking);
        assert.equal(execution.superseded, whole.superseded);
        assert.equal(execution.fold, null);
      }
    }
  }
});

function seqOfModelTurn(rec, turnId) {
  return seqOf(rec, turnId);
}

// --- Mission's rehydration rule --------------------------------------------

test("hideRehydrationClaim drops exactly the rehydration row", () => {
  const rehydrated =
    CODING_SESSION_CONTINUITY_STATUSES.get("session_rehydrated");
  const claim = {
    id: "claim",
    type: "lifecycle",
    renderClass: "status",
    title: "Session continuity",
    text: `${rehydrated} (an unrecognized_reason)`,
    timestamp,
    turnId: "t1",
  };
  const fresh = {
    ...claim,
    id: "fresh",
    text: CODING_SESSION_CONTINUITY_STATUSES.get("session_fresh"),
  };
  assert.equal(isCodingSessionRehydrationClaimItem(claim), true);
  assert.equal(isCodingSessionRehydrationClaimItem(fresh), false);

  const store = createCodingSessionExecutionModelStore();
  const rec = record([
    prompt("t1-prompt", "t1"),
    claim,
    say("t1-answer", "t1", "Done."),
    result("t1-result", "t1"),
  ]);
  const published = store.model(input(rec));
  const shown = onlyTurn(published.selectBlock(0));
  assert.ok(
    entryItemIds(shown.entries).includes("claim"),
    "on the wire, shown",
  );
  for (const variant of ["whole", "mission-narrative"]) {
    const hidden = onlyTurn(
      published.selectBlock(0, { variant, hideRehydrationClaim: true }),
    );
    assert.deepEqual(entryItemIds(hidden.entries), ["t1-prompt", "t1-answer"]);
    assert.equal(hidden.completion, shown.completion);
  }
});

test("workingTurnId picks the working turn and is part of the revision", async () => {
  const { createCodingSessionExecutionModelStore } = await import(
    "./codingSessionExecutionModels.ts"
  );
  const item = (id, turnId, type, extra = {}) => ({
    id,
    type,
    renderClass: type === "tool" ? "shell" : "message",
    role: type === "message" ? "user" : undefined,
    title: "x",
    text: id,
    timestamp: "2026-10-07T12:00:00.000Z",
    turnId,
    ...extra,
  });
  const transcript = Object.freeze([
    item("a1", "A", "message"),
    item("b1", "B", "message"),
    item("a2", "A", "message", { role: "assistant" }),
  ]);
  const record = {
    generationId: "g",
    providerAuthorityPubkey: "f".repeat(64),
    commandTarget: {
      driver: "d",
      instanceId: "i",
      sessionId: "s",
      generation: 1,
    },
    transcript,
    lastEventAt: "2026-10-07T12:00:00.000Z",
  };
  const store = createCodingSessionExecutionModelStore();
  const input = {
    record,
    executionKey: "e",
    isWorking: true,
    generationSuperseded: false,
  };
  const byLast = store.model({ ...input, workingTurnId: null });
  const byA = store.model({ ...input, workingTurnId: "A" });
  assert.notEqual(byA, byLast, "a different working turn is a new revision");
  const working = (model) =>
    model.model.blocks.filter((block) => block.isWorking).map((b) => b.id);
  assert.deepEqual(working(byA), ["A"]);
  assert.equal(store.model({ ...input, workingTurnId: "A" }), byA);
  assert.equal(store.stats().derivations, 2);
});

// ── SV-36 S5: liveness is a revision input, per generation ─────────────────

/** An open turn: a prompt and two paragraph pieces, no result yet. */
function openTurn(name) {
  return [
    prompt(`${name}-prompt`, name),
    say(`${name}-p1`, name, "One.\n\n"),
    say(`${name}-p2`, name, "Two."),
  ];
}

function arrivingProse(selection) {
  return onlyTurn(selection)
    .entries.filter(
      (entry) =>
        entry.kind === "item" &&
        entry.item.type === "message" &&
        entry.item.role === "assistant",
    )
    .map((entry) => [entry.item.text, entry.item.arriving === true]);
}

test("a live lease marks the selected block's answer arriving; a liveness change is a new revision", () => {
  const store = createCodingSessionExecutionModelStore();
  const rec = record(openTurn("t1"));
  const seq = seqOf(rec, "t1");
  const quiet = store.model(input(rec));
  assert.deepEqual(arrivingProse(quiet.selectBlock(seq)), [
    ["One.\n\nTwo.", false],
  ]);
  const live = store.model({ ...input(rec), producerWriting: true });
  assert.notEqual(live, quiet, "liveness is in the revision key");
  assert.deepEqual(arrivingProse(live.selectBlock(seq)), [
    ["One.\n\nTwo.", true],
  ]);
  const again = store.model({ ...input(rec), producerWriting: true });
  assert.equal(again, live, "identical inputs keep the revision");
  assert.equal(store.stats().derivations, 2);
  // The lease lapses: the same transcript reads settled again.
  const lapsed = store.model(input(rec));
  assert.deepEqual(arrivingProse(lapsed.selectBlock(seq)), [
    ["One.\n\nTwo.", false],
  ]);
});

test("one execution's liveness never changes another's model", () => {
  const store = createCodingSessionExecutionModelStore();
  const a = record(openTurn("a1"), "gen-a", 1);
  const b = record(openTurn("b1"), "gen-b", 1);
  const inputB = { ...input(b), executionKey: "execution-b" };
  const modelB = store.model(inputB);
  const liveA = store.model({ ...input(a), producerWriting: true });
  assert.deepEqual(arrivingProse(liveA.selectBlock(seqOf(a, "a1"))), [
    ["One.\n\nTwo.", true],
  ]);
  const derivations = store.stats().derivations;
  assert.equal(store.model(inputB), modelB, "B's revision is untouched");
  store.model(input(a));
  assert.equal(store.model(inputB), modelB, "A lapsing leaves B alone");
  assert.equal(store.stats().derivations, derivations + 1);
  assert.deepEqual(
    arrivingProse(modelB.selectBlock(0)),
    [["One.\n\nTwo.", false]],
    "the sibling with no lease shows no Writing…",
  );
});

test("isWorking alone never makes the answer arriving", () => {
  const store = createCodingSessionExecutionModelStore();
  const rec = record(openTurn("t1"));
  const working = store.model({
    ...input(rec, { isWorking: true }),
    workingTurnId: "t1",
  });
  assert.deepEqual(arrivingProse(working.selectBlock(seqOf(rec, "t1"))), [
    ["One.\n\nTwo.", false],
  ]);
});
