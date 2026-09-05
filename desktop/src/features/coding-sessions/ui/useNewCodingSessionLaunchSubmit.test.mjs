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
      rememberPrompt: () => {},
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
      rememberPrompt: () => {},
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

/**
 * The governed path forgot the draft it had just launched.
 *
 * `clearDraft` lived on the ungoverned branch only, so a session led by an
 * agent left its goal sitting in the dialog: reopening it offered to launch
 * an instruction that had already been carried out. The same success is now
 * also what puts the goal into ⌘↑ recall, so both are asserted together —
 * they are one act, "this prompt was launched", seen from two sides.
 */
test("N3: a governed launch clears the draft and remembers the prompt", async () => {
  const { act, renderHook, useNewCodingSessionLaunchSubmit } = await harness();

  let cleared = 0;
  const remembered = [];
  const READY_TARGET = {
    ...SELECTED_TARGET,
    signerPubkey: "a".repeat(64),
    provider: {
      allowedModels: ["sonnet"],
      providerInstanceRef: "instance-1",
    },
  };

  const mounted = renderHook(() =>
    useNewCodingSessionLaunchSubmit({
      canLaunch: true,
      candidates: [],
      channelId: SELECTED_TARGET.channelId,
      clearDraft: () => {
        cleared += 1;
      },
      leadModel: "sonnet",
      goCodingSession: () => {},
      goal: "Fix the flaky timeout.",
      governed: true,
      launch: async () => ({
        ok: true,
        channelId: SELECTED_TARGET.channelId,
        seats: [],
        hireableSeats: [],
      }),
      lead: {
        kind: "agent",
        actor: "b".repeat(64),
        role: "lead",
        label: "Lead",
      },
      onDone: () => {},
      policySet: false,
      projectContext: null,
      refreshRuntimeTarget: async () => READY_TARGET,
      rememberPrompt: (prompt) => remembered.push(prompt),
      selectedTarget: READY_TARGET,
      setIsPreparingChannel: () => {},
      setLaunchError: () => {},
      setSetupError: () => {},
      submit: async () => {},
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

  assert.equal(cleared, 1, "a launched goal is not a draft any more");
  assert.deepEqual(remembered, ["Fix the flaky timeout."]);

  mounted.unmount();
});

/** And a launch that failed keeps both: the text is still the person's to send. */
test("N4: a refused governed launch keeps the draft and remembers nothing", async () => {
  const { act, renderHook, useNewCodingSessionLaunchSubmit } = await harness();

  let cleared = 0;
  const remembered = [];

  const mounted = renderHook(() =>
    useNewCodingSessionLaunchSubmit({
      canLaunch: true,
      candidates: [],
      channelId: SELECTED_TARGET.channelId,
      clearDraft: () => {
        cleared += 1;
      },
      leadModel: "sonnet",
      goCodingSession: () => {},
      goal: "Fix the flaky timeout.",
      governed: true,
      launch: async () => ({ ok: false, failureReason: "no runtime" }),
      lead: {
        kind: "agent",
        actor: "b".repeat(64),
        role: "lead",
        label: "Lead",
      },
      onDone: () => {},
      policySet: false,
      projectContext: null,
      refreshRuntimeTarget: async () => ({ ...SELECTED_TARGET, provider: {} }),
      rememberPrompt: (prompt) => remembered.push(prompt),
      selectedTarget: SELECTED_TARGET,
      setIsPreparingChannel: () => {},
      setLaunchError: () => {},
      setSetupError: () => {},
      submit: async () => {},
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

  assert.equal(cleared, 0, "a refused launch must not eat the goal");
  assert.deepEqual(remembered, []);

  mounted.unmount();
});
