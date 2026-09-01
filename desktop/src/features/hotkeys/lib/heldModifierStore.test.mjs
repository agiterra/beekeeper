import assert from "node:assert/strict";
import test from "node:test";

// A minimal event target: the store only ever adds and removes listeners on
// `window` and `document`, so nothing here needs a real DOM.
class FakeTarget {
  listeners = new Map();
  addEventListener(type, listener) {
    const existing = this.listeners.get(type) ?? new Set();
    existing.add(listener);
    this.listeners.set(type, existing);
  }
  removeEventListener(type, listener) {
    this.listeners.get(type)?.delete(listener);
  }
  dispatch(type, event = {}) {
    for (const listener of [...(this.listeners.get(type) ?? [])]) {
      listener(event);
    }
  }
}

const fakeWindow = new FakeTarget();
const fakeDocument = new FakeTarget();
// The store asks the DOM whether a modal is up rather than trusting a flag.
let openDialog = false;
fakeDocument.querySelector = () => (openDialog ? {} : null);
globalThis.window = fakeWindow;
globalThis.document = fakeDocument;

const {
  ARM_DELAY_MS,
  __resetHeldModifierForTests,
  getHeldModifier,
  subscribeHeldModifier,
} = await import("./heldModifierStore.ts");

function key(overrides = {}) {
  return {
    altKey: false,
    ctrlKey: false,
    key: "Alt",
    metaKey: false,
    repeat: false,
    shiftKey: false,
    ...overrides,
  };
}

const HOLD_ALT = key({ altKey: true, key: "Alt" });
const HOLD_META = key({ metaKey: true, key: "Meta" });
const RELEASE = key({ key: "Alt" });

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function armed(run) {
  __resetHeldModifierForTests();
  const changes = [];
  const unsubscribe = subscribeHeldModifier(() => {
    changes.push(getHeldModifier());
  });
  try {
    return await run(changes);
  } finally {
    unsubscribe();
    __resetHeldModifierForTests();
  }
}

// A tap is not a hold: ⌘K, ⌘F and ⌘, all start with the modifier down, and
// flashing the whole badge set for 80ms on each of them is strobing.
test("a held modifier arms only after the delay", async () => {
  await armed(async () => {
    fakeWindow.dispatch("keydown", HOLD_ALT);
    assert.equal(getHeldModifier(), null);
    await wait(ARM_DELAY_MS + 40);
    assert.equal(getHeldModifier(), "alt");
  });
});

test("a key pressed with the modifier disarms immediately", async () => {
  await armed(async () => {
    fakeWindow.dispatch("keydown", HOLD_META);
    await wait(ARM_DELAY_MS + 40);
    assert.equal(getHeldModifier(), "meta");
    fakeWindow.dispatch("keydown", key({ metaKey: true, key: "k" }));
    assert.equal(getHeldModifier(), null);
  });
});

test("releasing the modifier disarms", async () => {
  await armed(async () => {
    fakeWindow.dispatch("keydown", HOLD_ALT);
    await wait(ARM_DELAY_MS + 40);
    fakeWindow.dispatch("keyup", RELEASE);
    assert.equal(getHeldModifier(), null);
  });
});

// ⌘-Tab sends keydown and then takes the window away, so the matching keyup
// never arrives. Without this the badges stay painted on forever.
test("losing the window disarms", async () => {
  await armed(async () => {
    fakeWindow.dispatch("keydown", HOLD_META);
    await wait(ARM_DELAY_MS + 40);
    fakeWindow.dispatch("blur");
    assert.equal(getHeldModifier(), null);
  });
});

test("tab visibility change disarms", async () => {
  await armed(async () => {
    fakeWindow.dispatch("keydown", HOLD_ALT);
    await wait(ARM_DELAY_MS + 40);
    fakeDocument.dispatch("visibilitychange");
    assert.equal(getHeldModifier(), null);
  });
});

test("two modifiers together are a different chord, not either one", async () => {
  await armed(async () => {
    fakeWindow.dispatch(
      "keydown",
      key({ altKey: true, metaKey: true, key: "Meta" }),
    );
    await wait(ARM_DELAY_MS + 40);
    assert.equal(getHeldModifier(), null);
  });
});

test("Shift alongside the modifier arms nothing", async () => {
  await armed(async () => {
    fakeWindow.dispatch("keydown", key({ altKey: true, shiftKey: true }));
    await wait(ARM_DELAY_MS + 40);
    assert.equal(getHeldModifier(), null);
  });
});

// Numbering rows behind a scrim points at things nobody can click, and the
// dialog's own shortcuts are the ones in play.
test("a modal owning the keyboard holds the badges off", async () => {
  await armed(async () => {
    openDialog = true;
    fakeWindow.dispatch("keydown", HOLD_ALT);
    await wait(ARM_DELAY_MS + 40);
    assert.equal(getHeldModifier(), null);
    openDialog = false;
  });
});

test("subscribers are told once per transition, not per event", async () => {
  await armed(async (changes) => {
    fakeWindow.dispatch("keydown", HOLD_ALT);
    fakeWindow.dispatch("keydown", { ...HOLD_ALT, repeat: true });
    await wait(ARM_DELAY_MS + 40);
    fakeWindow.dispatch("keyup", RELEASE);
    assert.deepEqual(changes, ["alt", null]);
  });
});
