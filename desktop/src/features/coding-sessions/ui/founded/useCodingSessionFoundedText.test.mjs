/**
 * Name and Initial prompt on the founded page — the honesty cases.
 *
 * No publish while the reader is unsettled; only non-empty and changed text
 * publishes; a refusal keeps the draft and shows the words; the builder's
 * own refusals are field errors, never relay trips; `flush` is name then
 * goal and stops at the first refusal.
 */
import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    localStorage: dom.window.localStorage,
    window: dom.window,
  });
});

beforeEach(() => {
  dom.window.localStorage.clear();
});

after(() => dom.window.close());

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const SESSION_REF = "22222222-2222-4222-8222-222222222222";

async function mount({
  wireName = null,
  namesResolved = true,
  goal = { kind: "absent" },
  draftName = null,
  publishName = async () => ({}),
  publishGoal = async () => ({}),
} = {}) {
  const React = (await import("react")).default;
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionFoundedText } = await import(
    "./useCodingSessionFoundedText.ts"
  );
  const calls = [];
  const deps = {
    publishName: async (input) => {
      calls.push(["name", input]);
      return publishName(input);
    },
    publishGoal: async (input) => {
      calls.push(["goal", input]);
      return publishGoal(input);
    },
  };
  const mounted = renderHook(
    (props) => {
      const [name, setName] = React.useState(draftName);
      const nameDraft = React.useMemo(() => ({ name, setName }), [name]);
      const model = useCodingSessionFoundedText({
        channelId: CHANNEL_ID,
        sessionRef: SESSION_REF,
        wireName: props.wireName,
        namesResolved: props.namesResolved,
        goal: props.goal,
        nameDraft,
        deps,
      });
      return { model, draftName: name };
    },
    { initialProps: { wireName, namesResolved, goal } },
  );
  return {
    calls,
    get model() {
      return mounted.result.current.model;
    },
    get draftName() {
      return mounted.result.current.draftName;
    },
    act: async (fn) => {
      await act(async () => {
        await fn();
      });
    },
    rerender: async (props) => {
      await act(async () => {
        mounted.rerender({ wireName, namesResolved, goal, ...props });
      });
    },
    unmount: () => mounted.unmount(),
  };
}

test("seed rules: the draft over the wire, the wire when no draft, blank when neither", async () => {
  const drafted = await mount({ wireName: "Wire name", draftName: "Typed" });
  assert.equal(drafted.model.name, "Typed");
  drafted.unmount();
  const wired = await mount({
    wireName: "Wire name",
    goal: { kind: "available", text: "Wire goal" },
  });
  assert.equal(wired.model.name, "Wire name");
  assert.equal(wired.model.prompt, "Wire goal");
  assert.equal(wired.model.nameDirty, false);
  assert.equal(wired.model.promptDirty, false);
  wired.unmount();
  const blank = await mount();
  assert.equal(blank.model.name, "");
  assert.equal(blank.model.prompt, "");
  blank.unmount();
});

test("the prompt shows a draft over the wire goal, and dirty means changed", async () => {
  const page = await mount({ goal: { kind: "available", text: "Wire goal" } });
  await page.act(() => page.model.setPrompt("Wire goal, revised"));
  assert.equal(page.model.prompt, "Wire goal, revised");
  assert.equal(page.model.promptDirty, true);
  // Typing the wire text back is not a change.
  await page.act(() => page.model.setPrompt("Wire goal"));
  assert.equal(page.model.promptDirty, false);
  page.unmount();
});

test("a blur with unchanged text publishes nothing", async () => {
  const page = await mount({
    wireName: "Same",
    goal: { kind: "available", text: "Same goal" },
  });
  await page.act(async () => {
    assert.deepEqual(await page.model.commitName(), { ok: true });
    assert.deepEqual(await page.model.commitPrompt(), { ok: true });
    assert.deepEqual(await page.model.flush(), { ok: true });
  });
  assert.deepEqual(page.calls, []);
  // Whitespace around the wire text is not a change either.
  await page.act(() => page.model.setName("  Same  "));
  await page.act(async () => {
    await page.model.commitName();
  });
  assert.deepEqual(page.calls, []);
  page.unmount();
});

test("a changed name publishes once and the draft yields to the wire", async () => {
  const page = await mount({ wireName: "Old" });
  await page.act(() => page.model.setName("New name"));
  assert.equal(page.model.nameDirty, true);
  await page.act(async () => {
    assert.deepEqual(await page.model.commitName(), { ok: true });
  });
  assert.deepEqual(page.calls, [
    [
      "name",
      { channelId: CHANNEL_ID, content: "New name", sessionRef: SESSION_REF },
    ],
  ]);
  assert.equal(page.draftName, null, "the wire now carries it");
  assert.equal(page.model.nameError, null);
  page.unmount();
});

test("a relay refusal keeps the draft and shows the relay's words under the field", async () => {
  const page = await mount({
    wireName: "Old",
    publishName: async () => {
      throw new Error("rate-limited: slow down");
    },
  });
  await page.act(() => page.model.setName("New name"));
  await page.act(async () => {
    assert.deepEqual(await page.model.commitName(), {
      ok: false,
      field: "name",
      reason: "rate-limited: slow down",
    });
  });
  assert.equal(page.draftName, "New name");
  assert.equal(page.model.nameError, "rate-limited: slow down");
  // Typing again clears the words; the draft is still the person's.
  await page.act(() => page.model.setName("New name 2"));
  assert.equal(page.model.nameError, null);
  page.unmount();
});

test("a name the builder refuses is a field error, never a relay trip", async () => {
  const page = await mount();
  await page.act(() => page.model.setName("two\nlines"));
  await page.act(async () => {
    const outcome = await page.model.commitName();
    assert.equal(outcome.ok, false);
    assert.match(outcome.reason, /one line between 1 and 256 UTF-8 bytes/);
  });
  assert.deepEqual(page.calls, []);
  assert.match(page.model.nameError, /one line/);
  page.unmount();
});

test("honesty: no name is published while names have not settled", async () => {
  const page = await mount({ namesResolved: false });
  await page.act(() => page.model.setName("Typed early"));
  await page.act(async () => {
    assert.deepEqual(await page.model.commitName(), { ok: true });
  });
  assert.deepEqual(page.calls, [], "the wire name has not been read yet");
  assert.equal(page.draftName, "Typed early", "the draft stands");
  // Once settled with no wire name, the same commit publishes.
  await page.rerender({ namesResolved: true });
  await page.act(async () => {
    await page.model.commitName();
  });
  assert.equal(page.calls.length, 1);
  page.unmount();
});

test("honesty: no goal is published while the goal reader is unresolved or errored", async () => {
  for (const goal of [
    { kind: "unresolved" },
    { kind: "errored", message: "relay closed the subscription" },
  ]) {
    const page = await mount({ goal });
    await page.act(() => page.model.setPrompt("Typed early"));
    assert.equal(page.model.goalReader, goal.kind);
    await page.act(async () => {
      assert.deepEqual(await page.model.commitPrompt(), { ok: true });
    });
    assert.deepEqual(page.calls, [], goal.kind);
    assert.equal(page.model.prompt, "Typed early", "the draft stands");
    page.unmount();
  }
});

test("honesty: the title suggestion is off until names settle and while a wire name exists", async () => {
  const unsettled = await mount({ namesResolved: false });
  assert.equal(unsettled.model.suggestion, null);
  unsettled.unmount();
  const named = await mount({ wireName: "From the phone" });
  assert.equal(named.model.suggestion, null);
  named.unmount();
  const open = await mount();
  assert.notEqual(open.model.suggestion, null);
  open.unmount();
});

test("a blank prompt is never a call into the goal builder", async () => {
  const page = await mount({ goal: { kind: "available", text: "Wire goal" } });
  await page.act(() => page.model.setPrompt("   "));
  await page.act(async () => {
    assert.deepEqual(await page.model.commitPrompt(), { ok: true });
  });
  assert.deepEqual(page.calls, []);
  assert.equal(page.model.promptAttempted, false);
  await page.act(() => page.model.markPromptAttempted());
  assert.equal(page.model.promptAttempted, true);
  // Typing clears the on-press blocker.
  await page.act(() => page.model.setPrompt("Now something"));
  assert.equal(page.model.promptAttempted, false);
  page.unmount();
});

test("a changed prompt publishes once and clears the draft", async () => {
  const page = await mount({ goal: { kind: "available", text: "Wire goal" } });
  await page.act(() => page.model.setPrompt("Wire goal, revised"));
  await page.act(async () => {
    assert.deepEqual(await page.model.commitPrompt(), { ok: true });
  });
  assert.deepEqual(page.calls, [
    [
      "goal",
      {
        channelId: CHANNEL_ID,
        content: "Wire goal, revised",
        sessionRef: SESSION_REF,
      },
    ],
  ]);
  // The draft is gone; until the wire echoes, the field shows what was just
  // published — never the old wire goal, which a Start would otherwise send.
  assert.equal(page.model.prompt, "Wire goal, revised");
  assert.equal(page.model.promptDirty, false);
  page.unmount();
});

test("the echo window: a committed name stands in for the wire until the wire carries it", async () => {
  const page = await mount({ wireName: "Old" });
  await page.act(() => page.model.setName("Typed"));
  await page.act(async () => {
    await page.model.commitName();
  });
  assert.equal(page.calls.length, 1);
  // Before the echo: the field still reads the typed name, nothing is dirty,
  // and the namer stays off — a generated name must not land in this gap.
  assert.equal(page.model.name, "Typed");
  assert.equal(page.model.nameDirty, false);
  assert.equal(page.model.suggestion, null);
  await page.act(async () => {
    assert.deepEqual(await page.model.flush(), { ok: true });
  });
  assert.equal(
    page.calls.length,
    1,
    "a blur in the window republishes nothing",
  );
  // The echo arrives: the wire is the source again, still not dirty.
  await page.rerender({ wireName: "Typed" });
  assert.equal(page.model.name, "Typed");
  assert.equal(page.model.nameDirty, false);
  assert.equal(page.model.suggestion, null, "a wire name exists");
  page.unmount();
});

test("the echo window: a committed prompt stands in for the wire goal until the wire carries it", async () => {
  const page = await mount({ goal: { kind: "available", text: "Old goal" } });
  await page.act(() => page.model.setPrompt("New goal"));
  await page.act(async () => {
    await page.model.commitPrompt();
  });
  assert.equal(page.model.prompt, "New goal");
  assert.equal(page.model.promptDirty, false);
  await page.act(async () => {
    await page.model.flush();
  });
  assert.equal(page.calls.length, 1, "nothing republished in the window");
  await page.rerender({ goal: { kind: "available", text: "New goal" } });
  assert.equal(page.model.prompt, "New goal");
  assert.equal(page.model.promptDirty, false);
  // A first goal on a session with none: the field does not blank after the
  // publish, so a Start in the window is not the blank-prompt blocker.
  const first = await mount();
  await first.act(() => first.model.setPrompt("First goal"));
  await first.act(async () => {
    await first.model.commitPrompt();
  });
  assert.equal(first.model.prompt, "First goal");
  first.unmount();
  page.unmount();
});

test("a refused prompt keeps the draft and shows the words", async () => {
  const page = await mount({
    publishGoal: async () => {
      throw new Error("forbidden: not a member");
    },
  });
  await page.act(() => page.model.setPrompt("Try this"));
  await page.act(async () => {
    assert.deepEqual(await page.model.commitPrompt(), {
      ok: false,
      field: "prompt",
      reason: "forbidden: not a member",
    });
  });
  assert.equal(page.model.prompt, "Try this");
  assert.equal(page.model.promptError, "forbidden: not a member");
  page.unmount();
});

test("flush publishes name then goal, only the dirty ones, and stops at the first refusal", async () => {
  const both = await mount({
    wireName: "Old",
    goal: { kind: "available", text: "Old goal" },
  });
  await both.act(() => both.model.setName("New"));
  await both.act(() => both.model.setPrompt("New goal"));
  await both.act(async () => {
    assert.deepEqual(await both.model.flush(), { ok: true });
  });
  assert.deepEqual(
    both.calls.map(([kind]) => kind),
    ["name", "goal"],
  );
  both.unmount();

  const goalOnly = await mount({
    wireName: "Old",
    goal: { kind: "available", text: "Old goal" },
  });
  await goalOnly.act(() => goalOnly.model.setPrompt("New goal"));
  await goalOnly.act(async () => {
    await goalOnly.model.flush();
  });
  assert.deepEqual(
    goalOnly.calls.map(([kind]) => kind),
    ["goal"],
  );
  goalOnly.unmount();

  const refused = await mount({
    wireName: "Old",
    goal: { kind: "available", text: "Old goal" },
    publishName: async () => {
      throw new Error("name refused");
    },
  });
  await refused.act(() => refused.model.setName("New"));
  await refused.act(() => refused.model.setPrompt("New goal"));
  await refused.act(async () => {
    assert.deepEqual(await refused.model.flush(), {
      ok: false,
      field: "name",
      reason: "name refused",
    });
  });
  assert.deepEqual(
    refused.calls.map(([kind]) => kind),
    ["name"],
    "the goal is not attempted after the name was refused",
  );
  refused.unmount();
});

test("the busy sentence names which field is publishing", async () => {
  let release = () => {};
  const page = await mount({
    publishName: () =>
      new Promise((resolve) => {
        release = resolve;
      }),
  });
  await page.act(() => page.model.setName("Slow"));
  let pending = null;
  await page.act(async () => {
    pending = page.model.commitName();
    await Promise.resolve();
  });
  assert.equal(page.model.busySentence, "Publishing the name…");
  await page.act(async () => {
    release({});
    await pending;
  });
  assert.equal(page.model.busySentence, null);
  page.unmount();
});

test("the echo window ends when the wire moves to somebody else's record, not only to ours", async () => {
  const page = await mount({ wireName: "Old" });
  await page.act(() => page.model.setName("Mine"));
  await page.act(async () => {
    await page.model.commitName();
  });
  assert.equal(page.model.name, "Mine");
  // The phone renamed it before our echo arrived: the wire is the truth
  // again, and our stand-in does not shadow it.
  await page.rerender({ wireName: "From the phone" });
  assert.equal(page.model.name, "From the phone");
  assert.equal(page.model.nameDirty, false);
  page.unmount();
});

test("a Start pressed during the blur's publish rides that publish: one 44227, not two", async () => {
  // Pressing Start right after typing the prompt fires the field's blur
  // first; its publish is in flight when Start's flush runs. The flush must
  // wait for it and publish nothing of its own (Lane C finding 4).
  let release = null;
  const page = await mount({
    goal: { kind: "available", text: "Wire goal" },
    publishGoal: () =>
      new Promise((resolve) => {
        release = resolve;
      }),
  });
  await page.act(() => page.model.setPrompt("Typed, then Start"));
  let blurOutcome = null;
  let flushOutcome = null;
  await page.act(async () => {
    const blur = page.model.commitPrompt();
    const flush = page.model.flush();
    await Promise.resolve();
    release({});
    blurOutcome = await blur;
    flushOutcome = await flush;
  });
  assert.deepEqual(blurOutcome, { ok: true });
  assert.deepEqual(flushOutcome, { ok: true });
  assert.equal(page.calls.filter(([kind]) => kind === "goal").length, 1);
  assert.equal(page.model.prompt, "Typed, then Start");
  assert.equal(page.model.promptDirty, false);
  page.unmount();
});
