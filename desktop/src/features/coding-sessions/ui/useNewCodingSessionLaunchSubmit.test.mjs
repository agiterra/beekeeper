/**
 * The launch button's own re-entrancy guard.
 *
 * Live-run finding 51 (2026-09-03): a single launch created two sessions
 * three seconds apart. `canLaunch` only turns false once
 * `useCodingSessionCrewLaunch`'s `isLaunching` flips, and that flip happens
 * *inside* the async work this hook runs — after the channel is prepared and
 * the runtime target is re-read, both awaits. A second click landing in that
 * window read a still-`true` `canLaunch` and started a second, independent
 * launch. This test reproduces exactly that: two synchronous calls to the
 * returned callback, before either await has resolved, and asserts the
 * underlying submit only ever runs once.
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

async function harness() {
  const { act, renderHook } = await import("@testing-library/react");
  const { useNewCodingSessionLaunchSubmit } = await import(
    "./useNewCodingSessionLaunchSubmit.ts"
  );
  return { act, renderHook, useNewCodingSessionLaunchSubmit };
}

const SELECTED_TARGET = {
  channelId: "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1",
};

const YOU_LEAD = { kind: "you" };

test("N1: two clicks before the first submit resolves launch exactly one session", async () => {
  const { act, renderHook, useNewCodingSessionLaunchSubmit } = await harness();

  let submitCalls = 0;
  let resolveSubmit;
  const submit = () =>
    new Promise((resolve) => {
      submitCalls += 1;
      resolveSubmit = resolve;
    });

  const mounted = renderHook(() =>
    useNewCodingSessionLaunchSubmit({
      canLaunch: true,
      candidates: [],
      channelId: SELECTED_TARGET.channelId,
      clearDraft: () => {},
      leadModel: null,
      goCodingSession: () => {},
      goal: "",
      governed: false,
      launch: async () => ({ ok: false, failureReason: "unused" }),
      lead: YOU_LEAD,
      onDone: () => {},
      policySet: false,
      projectContext: null,
      refreshRuntimeTarget: async () => null,
      selectedTarget: SELECTED_TARGET,
      setIsPreparingChannel: () => {},
      setLaunchError: () => {},
      setSetupError: () => {},
      submit,
      title: "",
      useWorktree: false,
      workdir: "",
      worktreeName: "",
      worktreeSource: null,
    }),
  );

  await act(async () => {
    // Both calls run synchronously, before `submit`'s promise — the async
    // work's first and only await here — has had any chance to settle. A
    // guard that only reacted to hook-level state (as the code did before
    // finding 51's fix) would let both through.
    mounted.result.current();
    mounted.result.current();
    resolveSubmit?.({});
    await new Promise((resolve) => setTimeout(resolve, 1));
  });

  assert.equal(
    submitCalls,
    1,
    "a second click before the first settles must be a no-op",
  );

  mounted.unmount();
});

test("N2: a click after the previous submit settled launches again", async () => {
  const { act, renderHook, useNewCodingSessionLaunchSubmit } = await harness();

  let submitCalls = 0;
  const submit = async () => {
    submitCalls += 1;
  };

  const mounted = renderHook(() =>
    useNewCodingSessionLaunchSubmit({
      canLaunch: true,
      candidates: [],
      channelId: SELECTED_TARGET.channelId,
      clearDraft: () => {},
      leadModel: null,
      goCodingSession: () => {},
      goal: "",
      governed: false,
      launch: async () => ({ ok: false, failureReason: "unused" }),
      lead: YOU_LEAD,
      onDone: () => {},
      policySet: false,
      projectContext: null,
      refreshRuntimeTarget: async () => null,
      selectedTarget: SELECTED_TARGET,
      setIsPreparingChannel: () => {},
      setLaunchError: () => {},
      setSetupError: () => {},
      submit,
      title: "",
      useWorktree: false,
      workdir: "",
      worktreeName: "",
      worktreeSource: null,
    }),
  );

  await act(async () => {
    mounted.result.current();
    await new Promise((resolve) => setTimeout(resolve, 1));
  });
  await act(async () => {
    mounted.result.current();
    await new Promise((resolve) => setTimeout(resolve, 1));
  });

  assert.equal(
    submitCalls,
    2,
    "the guard must release once the in-flight launch has finished, or a real retry could never launch",
  );

  mounted.unmount();
});
