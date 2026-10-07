import assert from "node:assert/strict";
import test from "node:test";

import {
  oracleProjectCodingSessionTranscript,
  oracleProjectTrustedCodingSessionTranscriptsToTranscript,
} from "./codingSessionProjectionOracle.testFixtures.ts";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  buildTrustedCodingSessionTranscriptProjectionContext,
  projectTrustedCodingSessionTranscriptsToTranscript,
  selectExactTrustedCodingSessionTranscriptEntries,
} from "./codingSessionTranscriptPresentation.ts";
import { createCodingSessionTranscriptFold } from "./codingSessionTranscriptProjection.ts";
import { createCodingSessionTranscriptProjector } from "./codingSessionTranscriptProjector.ts";

const CHANNEL_ID = "channel-1";
const SIGNER = "a".repeat(64);
const OTHER_SIGNER = "b".repeat(64);
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const OTHER_TARGET = { ...TARGET, generation: 2 };

let nextEventId = 1;

function hex64(n) {
  return n.toString(16).padStart(64, "0");
}

function entry({
  eventSeq,
  item,
  turnId = "turn-1",
  omitTurnId = false,
  channelId = CHANNEL_ID,
  signerPubkey = SIGNER,
  target = TARGET,
  conflictCount = 0,
  eventId = hex64(nextEventId++),
}) {
  const transcript = {
    schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
    session: target,
    eventSeq,
    timestamp: 1_800_000_000_000 + eventSeq,
    ...(omitTurnId ? {} : { turnId }),
    item,
  };
  return {
    channelId,
    targetKey: "unused-by-the-projector",
    signerPubkey,
    transcript,
    eventId,
    createdAt: 1_800_000_000 + eventSeq,
    conflictCount,
  };
}

function mulberry32(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function pick(rand, values) {
  return values[Math.floor(rand() * values.length)];
}

const TOOL_IDS = ["t1", "t2", "t3", "task-1", "bg-1"];

/** One random wire item covering every fold-relevant shape. */
function randomItem(rand) {
  const roll = rand();
  if (roll < 0.14) {
    return rand() < 0.25
      ? { kind: "user_prompt", content: "steer", steered: true }
      : { kind: "user_prompt", content: `prompt ${Math.floor(rand() * 99)}` };
  }
  if (roll < 0.3) {
    return { kind: "assistant_text", text: `text ${Math.floor(rand() * 999)}` };
  }
  if (roll < 0.48) {
    const toolId = pick(rand, TOOL_IDS);
    return {
      kind: "tool_call",
      ...(rand() < 0.2 ? { parentToolId: "task-1" } : {}),
      tool: {
        toolName: toolId === "task-1" ? "Task" : "bash",
        toolId,
        input: { command: `run ${Math.floor(rand() * 99)}` },
      },
    };
  }
  if (roll < 0.66) {
    const toolId = pick(rand, TOOL_IDS);
    return {
      kind: "tool_result",
      toolId,
      content: `out ${Math.floor(rand() * 99)}`,
      isError: rand() < 0.15,
      ...(toolId === "task-1" || toolId === "bg-1"
        ? { subagent: { type: "general", totalTokens: 12, toolUseCount: 2 } }
        : {}),
      ...(rand() < 0.1 ? { parentToolId: "task-1" } : {}),
    };
  }
  if (roll < 0.72) {
    return {
      kind: "tool_call",
      tool: {
        toolName: "ExitPlanMode",
        toolId: "plan-1",
        input: { plan: "1. do it" },
      },
    };
  }
  if (roll < 0.8) {
    return { kind: "result", subtype: "success", result: "done" };
  }
  if (roll < 0.85) {
    return { kind: "interrupted" };
  }
  if (roll < 0.95) {
    return { kind: "status", status: pick(rand, ["thinking", "idle"]) };
  }
  return { kind: "mystery_kind", payload: { nested: [1, 2, 3] } };
}

function randomTurn(rand) {
  const roll = rand();
  if (roll < 0.3) return { turnId: null };
  if (roll < 0.5) return { omitTurnId: true };
  return { turnId: pick(rand, ["turn-a", "turn-b", "turn-c"]) };
}

function json(value) {
  return JSON.stringify(value);
}

function assertDeepFrozen(value, seen = new Set()) {
  if (typeof value !== "object" || value === null || seen.has(value)) return;
  seen.add(value);
  assert.ok(Object.isFrozen(value), "every reachable object is frozen");
  for (const key of Object.keys(value)) assertDeepFrozen(value[key], seen);
}

function contextFor(label) {
  return buildTrustedCodingSessionTranscriptProjectionContext(
    CHANNEL_ID,
    SIGNER,
    TARGET,
    label === null ? null : { pubkey: SIGNER, label },
  );
}

function sourceFor(label) {
  return label === null ? null : { pubkey: SIGNER, label };
}

/** What the projector must have done, judged independently of it. */
function expectedOutcome(previous, selected, previousLabel, label) {
  if (previous === null || previousLabel !== label) return "rebuilt";
  if (selected.length < previous.length) return "rebuilt";
  for (let i = 0; i < previous.length; i += 1) {
    if (previous[i] !== selected[i]) return "rebuilt";
  }
  return selected.length === previous.length ? "retained" : "appended";
}

function runRandomScenario(seed, steps) {
  const rand = mulberry32(seed);
  const projector = createCodingSessionTranscriptProjector();
  let store = [];
  let seq = 10;
  let label = null;
  let previousSelected = null;
  let previousLabel = null;
  let previousOut = null;
  const history = [];
  const totals = { retained: 0, appended: 0, rebuilt: 0, patchedSameLength: 0 };

  for (let step = 0; step < steps; step += 1) {
    const roll = rand();
    let forceExpect = null;
    if (roll < 0.45) {
      // Ordinary streaming append.
      seq += 10;
      store.push(
        entry({ eventSeq: seq, item: randomItem(rand), ...randomTurn(rand) }),
      );
    } else if (roll < 0.52) {
      // Duplicate re-delivery: the same entry object arrives again, and the
      // catalog hands the same selection back (a fresh array, same refs).
    } else if (roll < 0.58) {
      // Same eventSeq as the newest: the event id decides, so this is either
      // an append or an insertion before the last entry.
      store.push(
        entry({ eventSeq: seq, item: randomItem(rand), ...randomTurn(rand) }),
      );
    } else if (roll < 0.64 && seq > 20) {
      // Late backfill into an earlier gap.
      const earlier =
        10 * (1 + Math.floor(rand() * (seq / 10 - 1))) +
        1 +
        Math.floor(rand() * 8);
      store.push(
        entry({
          eventSeq: earlier,
          item: randomItem(rand),
          ...randomTurn(rand),
        }),
      );
    } else if (roll < 0.69 && store.length > 0) {
      // Conflict: an entry is withheld once a competing claim appears.
      const index = Math.floor(rand() * store.length);
      store = store.map((candidate, i) =>
        i === index
          ? { ...candidate, conflictCount: candidate.conflictCount + 1 }
          : candidate,
      );
    } else if (roll < 0.76) {
      // Noise that must never reach this generation.
      seq += 10;
      store.push(
        entry({
          eventSeq: seq,
          item: randomItem(rand),
          ...pick(rand, [
            { target: OTHER_TARGET },
            { signerPubkey: OTHER_SIGNER },
            { channelId: "channel-2" },
            { conflictCount: 1 },
          ]),
        }),
      );
    } else if (roll < 0.79) {
      label = pick(rand, [null, "This computer", "Other label"]);
    } else if (roll < 0.81) {
      projector.reset();
      previousSelected = null;
      forceExpect = "rebuilt";
    } else if (roll < 0.82) {
      store = [];
    } else {
      seq += 10;
      store.push(
        entry({ eventSeq: seq, item: randomItem(rand), ...randomTurn(rand) }),
      );
    }

    const selected = selectExactTrustedCodingSessionTranscriptEntries(
      store,
      CHANNEL_ID,
      SIGNER,
      TARGET,
    );
    const before = { ...projector.stats };
    const out = projector.update(selected, contextFor(label));
    const after = { ...projector.stats };
    const expected =
      forceExpect ??
      expectedOutcome(previousSelected, selected, previousLabel, label);

    // Exactness against the frozen oracle and the production full path.
    const oracle = oracleProjectTrustedCodingSessionTranscriptsToTranscript(
      store,
      CHANNEL_ID,
      SIGNER,
      TARGET,
      sourceFor(label),
    );
    assert.equal(
      json(out),
      json(oracle),
      `seed ${seed} step ${step}: oracle parity`,
    );
    assert.equal(
      json(
        projectTrustedCodingSessionTranscriptsToTranscript(
          store,
          CHANNEL_ID,
          SIGNER,
          TARGET,
          sourceFor(label),
        ),
      ),
      json(oracle),
    );
    assert.ok(Object.isFrozen(out));

    // Work counters.
    const delta = Object.fromEntries(
      Object.keys(after).map((key) => [key, after[key] - before[key]]),
    );
    assert.equal(delta.updates, 1);
    assert.equal(
      delta[expected],
      1,
      `seed ${seed} step ${step}: expected ${expected}`,
    );
    const prevLength = previousSelected?.length ?? 0;
    if (expected === "retained") {
      assert.equal(out, previousOut);
      assert.deepEqual(delta, {
        ...zeroStats(),
        updates: 1,
        retained: 1,
        prefixComparisons: prevLength,
      });
    } else if (expected === "appended") {
      assert.equal(delta.presentedEntries, selected.length - prevLength);
      assert.equal(delta.prefixComparisons, prevLength);
      assert.equal(delta.copiedItems, out.length);
      assert.equal(delta.rebuilt + delta.retained, 0);
      // Unchanged items keep their identity; a changed slot can only be an
      // executing call completed by a newly appended result.
      let patched = 0;
      for (let i = 0; i < previousOut.length; i += 1) {
        if (out[i] === previousOut[i]) continue;
        patched += 1;
        assert.equal(previousOut[i].type, "tool");
        assert.equal(previousOut[i].status, "executing");
        assert.notEqual(json(out[i]), json(previousOut[i]));
      }
      assert.ok(patched <= selected.length - prevLength);
      if (out.length === previousOut.length) {
        // Only pairing can append without growing the list, and it must still
        // publish a new array whose middle differs.
        assert.notEqual(out, previousOut);
        assert.ok(patched > 0);
        totals.patchedSameLength += 1;
      }
    } else {
      assert.equal(delta.presentedEntries, selected.length);
      assert.equal(delta.copiedItems, out.length);
      assert.ok(delta.prefixComparisons <= prevLength);
      assert.equal(delta.retained + delta.appended, 0);
    }
    totals[expected] += 1;

    // Immutability of everything ever published.
    if (out !== previousOut) {
      assertDeepFrozen(out);
      history.push({ array: out, refs: [...out], snapshot: json(out) });
    }
    for (const record of history.slice(-5)) {
      assert.equal(json(record.array), record.snapshot);
    }
    for (const record of history) {
      assert.ok(Object.isFrozen(record.array));
      assert.equal(record.array.length, record.refs.length);
      for (const [i, ref] of record.refs.entries()) {
        assert.equal(record.array[i], ref);
      }
    }
    if (step % 25 === 0 || step === steps - 1) {
      for (const record of history) {
        assert.equal(
          json(record.array),
          record.snapshot,
          `seed ${seed} step ${step}: history`,
        );
        assertDeepFrozen(record.array);
      }
    }

    previousSelected = selected;
    previousLabel = label;
    previousOut = out;
  }
  return totals;
}

function zeroStats() {
  return {
    updates: 0,
    retained: 0,
    appended: 0,
    rebuilt: 0,
    presentedEntries: 0,
    prefixComparisons: 0,
    copiedItems: 0,
  };
}

for (const seed of [1, 7, 42, 1337, 90210, 31337]) {
  test(`seeded random stream ${seed}: oracle parity, immutability and work after every step`, () => {
    const totals = runRandomScenario(seed, 300);
    // The scenario must actually exercise every path, or it proves nothing.
    assert.ok(totals.appended > 50, `appended ${totals.appended}`);
    assert.ok(totals.rebuilt > 10, `rebuilt ${totals.rebuilt}`);
    assert.ok(totals.retained > 10, `retained ${totals.retained}`);
  });
}

test("the random streams include same-length pairing patches", () => {
  let patched = 0;
  for (const seed of [2, 3, 5])
    patched += runRandomScenario(seed, 200).patchedSameLength;
  assert.ok(patched > 0);
});

// --- deterministic cases ----------------------------------------------------

function stream(items) {
  return items.map((item, index) =>
    entry({ eventSeq: (index + 1) * 10, item }),
  );
}

test("identical input returns the same array and does no work", () => {
  const projector = createCodingSessionTranscriptProjector();
  const entries = stream([
    { kind: "user_prompt", content: "hi" },
    { kind: "assistant_text", text: "hello" },
  ]);
  const first = projector.update(entries, contextFor(null));
  const before = { ...projector.stats };
  // A fresh array and a fresh, field-equal context still count as identical.
  const second = projector.update([...entries], contextFor(null));
  assert.equal(second, first);
  assert.deepEqual(
    Object.fromEntries(
      Object.keys(before).map((k) => [k, projector.stats[k] - before[k]]),
    ),
    { ...zeroStats(), updates: 1, retained: 1, prefixComparisons: 2 },
  );
});

test("a result patches a middle call: same length and endpoints, new array and object", () => {
  const projector = createCodingSessionTranscriptProjector();
  const entries = stream([
    { kind: "user_prompt", content: "go" },
    {
      kind: "tool_call",
      tool: { toolName: "bash", toolId: "t1", input: { command: "ls" } },
    },
    { kind: "assistant_text", text: "waiting" },
  ]);
  const before = projector.update(entries, contextFor(null));
  const beforeSnapshot = json(before);
  const call = before[1];
  const callSnapshot = json(call);
  const after = projector.update(
    [
      ...entries,
      entry({
        eventSeq: 40,
        item: { kind: "tool_result", toolId: "t1", content: "a b" },
      }),
    ],
    contextFor(null),
  );
  assert.equal(after.length, before.length);
  assert.notEqual(after, before);
  assert.equal(after[0], before[0]);
  assert.equal(after[2], before[2]);
  assert.notEqual(after[1], call);
  assert.equal(after[1].status, "completed");
  assert.equal(after[1].result, "a b");
  assert.equal(json(before), beforeSnapshot);
  assert.equal(json(call), callSnapshot);
  assert.equal(call.status, "executing");
  assert.equal(projector.stats.appended, 1);
  assert.equal(projector.stats.presentedEntries, 4);
});

test("an ordinary append presents only the new entries", () => {
  const projector = createCodingSessionTranscriptProjector();
  const entries = stream(
    Array.from({ length: 50 }, (_, i) => ({
      kind: "assistant_text",
      text: `t${i}`,
    })),
  );
  projector.update(entries.slice(0, 40), contextFor(null));
  assert.equal(projector.stats.presentedEntries, 40);
  projector.update(entries.slice(0, 41), contextFor(null));
  projector.update(entries, contextFor(null));
  assert.equal(projector.stats.presentedEntries, 50);
  assert.equal(projector.stats.appended, 2);
  assert.equal(projector.stats.rebuilt, 1);
  // Proving the prefix and publishing are linear in history, by design.
  assert.equal(projector.stats.prefixComparisons, 40 + 41);
  assert.equal(projector.stats.copiedItems, 40 + 41 + 50);
});

test("a late insert rebuilds and presents the whole generation", () => {
  const projector = createCodingSessionTranscriptProjector();
  const entries = stream([
    { kind: "user_prompt", content: "a" },
    { kind: "assistant_text", text: "b" },
    { kind: "assistant_text", text: "c" },
  ]);
  projector.update(entries, contextFor(null));
  const late = entry({
    eventSeq: 15,
    item: { kind: "assistant_text", text: "late" },
  });
  const all = [...entries, late];
  const out = projector.update(
    selectExactTrustedCodingSessionTranscriptEntries(
      all,
      CHANNEL_ID,
      SIGNER,
      TARGET,
    ),
    contextFor(null),
  );
  assert.equal(projector.stats.rebuilt, 2);
  assert.equal(projector.stats.presentedEntries, 3 + 4);
  assert.deepEqual(
    out.map((item) => item.text),
    ["a", "late", "b", "c"],
  );
});

test("a context change rebuilds under the new scope", () => {
  const projector = createCodingSessionTranscriptProjector();
  const entries = stream([{ kind: "assistant_text", text: "x" }]);
  projector.update(entries, contextFor(null));
  const out = projector.update(entries, contextFor("This computer"));
  assert.equal(projector.stats.rebuilt, 2);
  assert.equal(out[0].bridgeSource.label, "This computer");
});

test("empty input returns a frozen empty array and drops state; reset rebuilds", () => {
  const projector = createCodingSessionTranscriptProjector();
  const entries = stream([{ kind: "assistant_text", text: "x" }]);
  projector.update(entries, contextFor(null));
  const empty = projector.update([], contextFor(null));
  assert.equal(empty.length, 0);
  assert.ok(Object.isFrozen(empty));
  assert.equal(projector.stats.rebuilt, 2);
  projector.update(entries, contextFor(null));
  assert.equal(projector.stats.appended, 1);
  projector.reset();
  projector.update(entries, contextFor(null));
  assert.equal(projector.stats.rebuilt, 3);
});

test("projectors are independent instances", () => {
  const left = createCodingSessionTranscriptProjector();
  const right = createCodingSessionTranscriptProjector();
  const entries = stream([{ kind: "assistant_text", text: "x" }]);
  left.update(entries, contextFor(null));
  right.update(entries, contextFor(null));
  right.update(entries, contextFor(null));
  assert.equal(left.stats.updates, 1);
  assert.equal(right.stats.retained, 1);
});

test("a declared-null turn never takes a synthetic id; result-before-call stays standalone", () => {
  const projector = createCodingSessionTranscriptProjector();
  const entries = [
    entry({
      eventSeq: 1,
      turnId: null,
      item: { kind: "user_prompt", content: "p" },
    }),
    entry({
      eventSeq: 2,
      turnId: null,
      item: { kind: "tool_result", toolId: "t9", content: "early" },
    }),
    entry({
      eventSeq: 3,
      turnId: null,
      item: { kind: "tool_call", tool: { toolName: "bash", toolId: "t9" } },
    }),
  ];
  const out = projector.update(entries, contextFor(null));
  assert.equal(out.length, 3);
  assert.equal(out[0].turnId, undefined);
  assert.equal(out[1].status, "completed");
  assert.equal(out[2].status, "executing");
});

test("a hostile entry degrades to a bounded item instead of throwing", () => {
  const projector = createCodingSessionTranscriptProjector();
  const hostile = {};
  Object.defineProperty(hostile, "transcript", {
    get() {
      throw new Error("boom");
    },
  });
  const out = projector.update(
    [...stream([{ kind: "assistant_text", text: "ok" }]), hostile],
    contextFor(null),
  );
  assert.equal(out.length, 2);
  assert.equal(out[1].title, "Unrecognized transcript event");
});

// --- the shared fold, resumed one raw envelope at a time ---------------------
//
// Trusted entries always carry a `turnId` key, so synthetic turn
// reconstruction and target fencing are unreachable through the projector.
// The fold they share is driven here with raw envelopes instead, one push at
// a time, against the oracle's full rebuild of the same prefix.

function randomEnvelope(rand, seq) {
  const roll = rand();
  if (roll < 0.04) return pick(rand, [null, 42, "junk"]);
  if (roll < 0.07)
    return {
      eventSeq: seq,
      item: { kind: "assistant_text", text: "no target" },
    };
  const turn = rand();
  return {
    target: rand() < 0.15 ? OTHER_TARGET : TARGET,
    eventSeq: rand() < 0.05 ? seq - 1 : seq,
    timestamp: 1_700_000_000_000 + seq,
    item: randomItem(rand),
    ...(turn < 0.5
      ? {}
      : turn < 0.7
        ? { turnId: null }
        : { turnId: pick(rand, ["x", "y", ""]) }),
    ...(rand() < 0.5 ? { sourceEventId: hex64(seq) } : {}),
  };
}

for (const seed of [11, 23, 99]) {
  test(`resumed fold equals the oracle rebuild after every push (seed ${seed})`, () => {
    const rand = mulberry32(seed);
    const options = {
      channelId: CHANNEL_ID,
      generationId: "gen-1",
      bridgeSource: { pubkey: SIGNER, label: "fold" },
    };
    const fold = createCodingSessionTranscriptFold(options);
    const envelopes = [];
    let synthetic = 0;
    for (let step = 1; step <= 300; step += 1) {
      const envelope = randomEnvelope(rand, step);
      envelopes.push(envelope);
      fold.push(envelope);
      const oracle = oracleProjectCodingSessionTranscript(envelopes, options);
      assert.equal(json(fold.items), json(oracle), `seed ${seed} step ${step}`);
    }
    for (const item of fold.items) {
      if (
        typeof item.turnId === "string" &&
        item.turnId.includes("presentation-turn")
      )
        synthetic += 1;
    }
    assert.ok(synthetic > 0, "synthetic turns were exercised");
  });
}
