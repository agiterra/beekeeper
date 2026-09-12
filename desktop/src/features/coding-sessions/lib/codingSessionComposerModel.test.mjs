import assert from "node:assert/strict";
import test from "node:test";

// `hasPrimaryShortcutModifier` reads `navigator.platform`, and `navigator` is
// a getter-only global on Node. Pin it so these assertions describe macOS
// behaviour on every host instead of quietly inverting on a Linux runner.
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: { platform: "MacIntel", userAgent: "test" },
});

const {
  getCodingSessionComposerState,
  matchCodingSessionHistoryKey,
  shouldSubmitCodingSessionComposerKey,
} = await import("./codingSessionComposerModel.ts");

test("member gate is fail-closed while the composer remains reachable", () => {
  assert.deepEqual(
    getCodingSessionComposerState({
      canSteer: false,
      isMember: false,
      isWorking: false,
      text: "Steer",
    }),
    {
      canSend: false,
      deliveryHint: null,
      primaryLabel: "Send",
      secondaryLabel: null,
      sendLabel: "Send",
      showAuthorityFailure: true,
      showStopAction: false,
    },
  );
});

test("a running turn offers Steer beside the interrupt, and Enter steers", () => {
  assert.equal(
    getCodingSessionComposerState({
      canSteer: true,
      isMember: true,
      isWorking: true,
      text: "Steer",
    }).sendLabel,
    "Steer",
  );
  assert.equal(
    getCodingSessionComposerState({
      canSteer: true,
      isMember: true,
      isWorking: true,
      text: "Steer",
    }).showStopAction,
    true,
  );
  assert.equal(
    shouldSubmitCodingSessionComposerKey({ key: "Enter", shiftKey: false }),
    true,
  );
  assert.equal(
    shouldSubmitCodingSessionComposerKey({ key: "Enter", shiftKey: true }),
    false,
  );
});

// Asked live, 2026-08-24: "why can it no longer be resumed? Should there be a
// pause and a stop?" Part of that confusion was the composer itself — the
// turn-level control and the terminal one both read "Stop", side by side.
test("only the terminal control is called Stop", () => {
  const working = getCodingSessionComposerState({
    canSteer: true,
    isMember: true,
    isWorking: true,
    text: "go",
  });
  assert.equal(working.primaryLabel, "Interrupt");
  assert.notEqual(working.primaryLabel, "Stop");
  assert.equal(working.sendLabel, "Steer");
  assert.equal(working.showStopAction, true);
});

// The label is a promise about what the provider will do with the words. An
// execution whose runtime advertised no native steering cannot keep it: the
// turn reaches the provider now and runs at the next turn boundary.
test("a non-steering execution is not offered a steer it cannot get", () => {
  assert.equal(
    getCodingSessionComposerState({
      canSteer: false,
      isMember: true,
      isWorking: true,
      text: "go",
    }).sendLabel,
    "Queue next",
  );
  assert.equal(
    getCodingSessionComposerState({
      canSteer: false,
      isMember: true,
      isWorking: false,
      text: "go",
    }).sendLabel,
    "Send",
  );
  assert.equal(
    getCodingSessionComposerState({
      canSteer: true,
      isMember: true,
      isWorking: true,
      text: "go",
    }).sendLabel,
    "Steer",
  );
});

test("a caller that says nothing about steering does not get a Steer button", () => {
  // Fail-safe, not fail-open. `canSteer` is this execution's own capability,
  // learned from its 44223 metadata; a call site that forgets to pass it used
  // to get "Steer" and a `deliver: "steer"` command against a provider that
  // cannot steer — a control that degrades one hundred per cent of the time,
  // which is exactly what the delivery classes exist to prevent.
  assert.equal(
    getCodingSessionComposerState({
      isMember: true,
      isWorking: true,
      text: "Steer",
    }).sendLabel,
    "Queue next",
  );
});

test("send is held closed while an attachment is unsettled", () => {
  const base = {
    canSteer: false,
    isMember: true,
    isWorking: false,
    text: "look at this",
  };
  assert.equal(getCodingSessionComposerState(base).canSend, true);
  // Mid-upload: publishing here would sign a turn whose attachment list is
  // short of what the composer is showing.
  assert.equal(
    getCodingSessionComposerState({ ...base, hasUnsettledAttachments: true })
      .canSend,
    false,
  );
  // The label is unaffected — only the gate closes, so the button does not
  // start claiming a different action while an upload finishes.
  assert.equal(
    getCodingSessionComposerState({ ...base, hasUnsettledAttachments: true })
      .sendLabel,
    "Send",
  );
});

// ── ⌘↑ / ⌘↓ prompt-history recall ────────────────────────────────────────────

function arrow(overrides = {}) {
  return {
    altKey: false,
    ctrlKey: false,
    key: "ArrowUp",
    metaKey: false,
    shiftKey: false,
    ...overrides,
  };
}

test("⌘↑ walks back and ⌘↓ walks forward", () => {
  assert.equal(matchCodingSessionHistoryKey(arrow({ metaKey: true })), "older");
  assert.equal(
    matchCodingSessionHistoryKey(arrow({ key: "ArrowDown", metaKey: true })),
    "newer",
  );
});

// A bare arrow still has to move the caret inside a multi-line draft.
test("a bare arrow is left to the textarea", () => {
  assert.equal(matchCodingSessionHistoryKey(arrow()), null);
  assert.equal(matchCodingSessionHistoryKey(arrow({ key: "ArrowDown" })), null);
});

test("Shift or Option makes it someone else's chord", () => {
  assert.equal(
    matchCodingSessionHistoryKey(arrow({ metaKey: true, shiftKey: true })),
    null,
  );
  assert.equal(
    matchCodingSessionHistoryKey(arrow({ metaKey: true, altKey: true })),
    null,
  );
});

// On macOS Ctrl is left to the native Emacs-style text bindings.
test("Ctrl is not the primary modifier on macOS", () => {
  assert.equal(matchCodingSessionHistoryKey(arrow({ ctrlKey: true })), null);
});

test("other keys are not history", () => {
  assert.equal(
    matchCodingSessionHistoryKey(arrow({ key: "Enter", metaKey: true })),
    null,
  );
});

// The point of the second control: an execution that *can* steer now offers
// both classes, and the person picks. Before this the composer decided, and
// "look at the other file when you're done" went into the middle of a running
// turn because that is what the only button did.
test("a steering execution offers the boundary as a named second choice", () => {
  const working = getCodingSessionComposerState({
    canSteer: true,
    isMember: true,
    isWorking: true,
    text: "when you finish, run the gate",
  });
  assert.equal(working.sendLabel, "Steer");
  assert.equal(working.secondaryLabel, "Queue next");
  assert.equal(
    working.deliveryHint,
    "Steer joins the turn that is running. Queue next runs after it.",
  );
});

// And the converse: where the primary already queues, a second button beside
// it would be the same act under two names.
test("an execution that cannot steer offers one choice and explains it", () => {
  const working = getCodingSessionComposerState({
    canSteer: false,
    isMember: true,
    isWorking: true,
    text: "when you finish, run the gate",
  });
  assert.equal(working.sendLabel, "Queue next");
  assert.equal(working.secondaryLabel, null);
  assert.equal(
    working.deliveryHint,
    "Runs after the current turn — this execution cannot steer.",
  );
});

// An idle execution has no turn to steer or to wait for, so neither the
// second control nor the sentence explaining it has anything to say.
test("an idle execution says nothing about delivery classes", () => {
  for (const canSteer of [true, false]) {
    const idle = getCodingSessionComposerState({
      canSteer,
      isMember: true,
      isWorking: false,
      text: "start here",
    });
    assert.equal(idle.sendLabel, "Send");
    assert.equal(idle.secondaryLabel, null);
    assert.equal(idle.deliveryHint, null);
  }
});

// "Queue next" and not "Send next": the command *is* sent — signed,
// published, irrevocable — and then waits. "Send next" reads as "send the
// next one", which is the one thing this control does not do.
test("the mid-turn boundary control is never labelled as a deferred send", () => {
  for (const canSteer of [true, false]) {
    const state = getCodingSessionComposerState({
      canSteer,
      isMember: true,
      isWorking: true,
      text: "go",
    });
    for (const label of [state.sendLabel, state.secondaryLabel]) {
      assert.notEqual(label, "Send next");
    }
  }
});
