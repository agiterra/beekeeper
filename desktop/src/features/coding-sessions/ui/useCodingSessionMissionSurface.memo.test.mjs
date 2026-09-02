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

const FOUNDER = "3d3b7169".repeat(8);
const CHANNEL = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";
const SESSION = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";

/**
 * One stable props object, built once.
 *
 * Deliberately stable: the workspace memoizes what it hands this hook, so the
 * only thing changing between these two renders is what the hook computes for
 * itself — which is exactly the F6 trap (`goalSelection.disagreements` was a
 * fresh array on every call and a `useMemo` dependency).
 */
const PROPS = (() => {
  return {
    active: false,
    channelId: CHANNEL,
    contextLoads: [],
    focusedExecutionKey: null,
    goal: {
      channelId: CHANNEL,
      content: "Ship the portable team loop.",
      createdAt: 1_788_366_485,
      eventId: "f29f23e8".repeat(8),
      founderPubkey: FOUNDER,
      sessionRef: SESSION,
    },
    isNarrow: false,
    observedChanges: { files: [], unreportedEditCount: 0 },
    onFocusParticipant: () => {},
    onOpenTrace: () => {},
    participants: [],
    resolveActorName: () => null,
    umbrella: {
      channelId: CHANNEL,
      sessionRef: SESSION,
      genesisRef: "ce5d87ed".repeat(8),
      founderPubkey: FOUNDER,
      title: "Portable team loop",
      executions: [],
    },
  };
})();

test("F6: the Mission model does not re-derive when nothing about the umbrella moved", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionMissionSurface } = await import(
    "./useCodingSessionMissionSurface.tsx"
  );
  const view = renderHook(() => useCodingSessionMissionSurface(PROPS));
  const first = {
    missionState: view.result.current.missionState,
    wakeOperations: view.result.current.wakeOperations,
    transactions: view.result.current.transactions,
  };
  await act(async () => {
    view.rerender();
  });
  // `goalSelection.disagreements` was a fresh array every render and a memo
  // dependency, so `inspectorInput` — and therefore the whole Inspector model
  // — was rebuilt on every render of every Mission surface (F6).
  assert.equal(view.result.current.missionState, first.missionState);
  assert.equal(view.result.current.wakeOperations, first.wakeOperations);
  assert.equal(view.result.current.transactions, first.transactions);
  view.unmount();
});
