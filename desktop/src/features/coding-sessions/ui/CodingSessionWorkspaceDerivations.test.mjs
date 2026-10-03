/**
 * The workspace's transcript-wide derivations run only when the transcript's
 * content changes, and a burst costs one run per frame rather than one per
 * event.
 *
 * Items come from the real projection, re-run per publish exactly as the
 * catalog does it — so every publish hands the workspace a new array of new
 * objects, which is what made each derivation re-run before.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

after(() => dom.window.close());

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

function envelope(seq) {
  const item =
    seq % 3 === 0
      ? {
          kind: "tool_call",
          toolId: `tool-${seq}`,
          toolName: "Edit",
          input: { file_path: `src/file-${seq}.ts` },
        }
      : { kind: "assistant_text", text: `paragraph ${seq}` };
  return {
    target: TARGET,
    eventSeq: seq,
    timestamp: 1_800_000_000_000 + seq,
    turnId: "turn-1",
    item,
  };
}

async function loadProjection() {
  const { projectCodingSessionTranscript } = await import(
    "../lib/codingSessionTranscriptProjection.ts"
  );
  return (count) =>
    projectCodingSessionTranscript(
      Array.from({ length: count }, (_, index) => envelope(index + 1)),
      {
        channelId: "channel-1",
        generationId: "generation-1",
        bridgeSource: { pubkey: "a".repeat(64), label: "provider" },
      },
    );
}

function countingDerivers(real) {
  const counts = {};
  const derivers = {};
  for (const [name, derive] of Object.entries(real)) {
    counts[name] = 0;
    derivers[name] = (...args) => {
      counts[name] += 1;
      return derive(...args);
    };
  }
  const total = () => Object.values(counts).reduce((sum, n) => sum + n, 0);
  return { counts, derivers, total };
}

/**
 * The publish sequence a streaming turn produces: 20 transcript appends
 * interleaved with 10 events that change nothing in this transcript (a 44223
 * status, another execution's item, a receipt) — each still a re-projection.
 */
function publishSequence(initial) {
  const sequence = [];
  let count = initial;
  for (let step = 0; step < 30; step += 1) {
    if (step % 3 === 2) sequence.push(count);
    else sequence.push(++count);
  }
  return sequence;
}

test("derivations run per content change, not per publish; a frame-batched burst runs them once per frame", async () => {
  const React = (await import("react")).default;
  const { act, renderHook } = await import("@testing-library/react");
  const {
    CODING_SESSION_WORKSPACE_DERIVERS,
    useCodingSessionWorkspaceDerivations,
  } = await import("./CodingSessionWorkspaceDerivations.ts");
  const project = await loadProjection();
  const INITIAL = 50;
  const sequence = publishSequence(INITIAL);
  const derivationNames = Object.keys(CODING_SESSION_WORKSPACE_DERIVERS);

  // Before: the workspace memoized each derivation on the raw transcript
  // reference, which is new on every publish.
  const beforeCount = countingDerivers(CODING_SESSION_WORKSPACE_DERIVERS);
  {
    const { rerender } = renderHook(
      ({ transcript }) => {
        for (const name of derivationNames) {
          // biome-ignore lint/correctness/useHookAtTopLevel: fixed-length loop mirrors the old unrolled memos
          React.useMemo(
            () => beforeCount.derivers[name](transcript),
            [transcript],
          );
        }
      },
      { initialProps: { transcript: project(INITIAL) } },
    );
    const baseline = beforeCount.total();
    for (const count of sequence) {
      await act(async () => rerender({ transcript: project(count) }));
    }
    beforeCount.runs = beforeCount.total() - baseline;
  }

  // After, unbatched (one publish per event): only real changes re-derive.
  const afterCount = countingDerivers(CODING_SESSION_WORKSPACE_DERIVERS);
  let earlierItem;
  let sameItemAfterAppend;
  {
    const { result, rerender } = renderHook(
      ({ transcript }) =>
        useCodingSessionWorkspaceDerivations(transcript, afterCount.derivers),
      { initialProps: { transcript: project(INITIAL) } },
    );
    const baseline = afterCount.total();
    earlierItem = result.current.transcript[0];
    let previous = result.current.transcript;
    for (const count of sequence) {
      await act(async () => rerender({ transcript: project(count) }));
      if (count === previous.length) {
        assert.equal(
          result.current.transcript,
          previous,
          "a re-projection that changed nothing keeps the same array",
        );
      }
      previous = result.current.transcript;
    }
    sameItemAfterAppend = result.current.transcript[0];
    afterCount.runs = afterCount.total() - baseline;
  }

  // After, batched: the ingress publishes once per frame — four frames.
  const batchedCount = countingDerivers(CODING_SESSION_WORKSPACE_DERIVERS);
  {
    const { rerender } = renderHook(
      ({ transcript }) =>
        useCodingSessionWorkspaceDerivations(transcript, batchedCount.derivers),
      { initialProps: { transcript: project(INITIAL) } },
    );
    const baseline = batchedCount.total();
    const frames = 4;
    const perFrame = Math.ceil(sequence.length / frames);
    for (let frame = 0; frame < frames; frame += 1) {
      const last =
        sequence[Math.min(sequence.length, (frame + 1) * perFrame) - 1];
      await act(async () => rerender({ transcript: project(last) }));
    }
    batchedCount.runs = batchedCount.total() - baseline;
  }

  const appends = sequence.filter(
    (count, index) => count !== (sequence[index - 1] ?? INITIAL),
  ).length;
  const perChange = derivationNames.length;
  assert.equal(beforeCount.runs, sequence.length * perChange);
  assert.equal(afterCount.runs, appends * perChange);
  assert.equal(batchedCount.runs, 4 * perChange);
  assert.ok(earlierItem !== undefined);
  assert.equal(
    sameItemAfterAppend,
    earlierItem,
    "an append keeps every earlier item's identity",
  );
  console.log(
    `workspace derivations over ${sequence.length} publishes (${appends} appends): before ${beforeCount.runs} runs, after ${afterCount.runs} unbatched, ${batchedCount.runs} frame-batched`,
  );
});

test("reconcile: a changed item is replaced, its neighbours kept", async () => {
  const { reconcileCodingSessionTranscriptItems } = await import(
    "./CodingSessionWorkspaceDerivations.ts"
  );
  const previous = [
    { id: "a", text: "one", nested: { list: [1, 2] } },
    { id: "b", text: "two", nested: { list: [3] } },
    { id: "c", text: "three", nested: { list: [] } },
  ];
  const next = [
    { id: "a", text: "one", nested: { list: [1, 2] } },
    { id: "b", text: "two!", nested: { list: [3] } },
    { id: "c", text: "three", nested: { list: [] } },
  ];
  const merged = reconcileCodingSessionTranscriptItems(previous, next);
  assert.notEqual(merged, previous);
  assert.equal(merged[0], previous[0]);
  assert.equal(merged[1], next[1]);
  assert.equal(merged[2], previous[2]);

  const equal = reconcileCodingSessionTranscriptItems(
    previous,
    previous.map((item) => structuredClone(item)),
  );
  assert.equal(equal, previous);

  // A shorter transcript (a retraction, a conflict) is a change.
  const shorter = reconcileCodingSessionTranscriptItems(
    previous,
    previous.slice(0, 2).map((item) => structuredClone(item)),
  );
  assert.equal(shorter.length, 2);
  assert.notEqual(shorter, previous);
  assert.equal(shorter[0], previous[0]);

  // A key present on one side only is a change, even when its value is
  // undefined.
  const extraKey = reconcileCodingSessionTranscriptItems(
    [{ id: "a" }],
    [{ id: "a", status: undefined }],
  );
  assert.notEqual(extraKey[0].status, "x");
  assert.equal(Object.hasOwn(extraKey[0], "status"), true);
});
