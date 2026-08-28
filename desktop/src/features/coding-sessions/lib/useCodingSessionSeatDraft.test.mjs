/**
 * The seat draft is shared by founding a session and joining one, so the
 * wiring — not just the pure default — is pinned here: choosing an agent
 * fills the role, typing pins it, and clearing the agent forgets both.
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
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

after(() => dom.window.close());

const ADA = "aa11bb22".repeat(8);
const BEN = "bb22cc33".repeat(8);
const AGENTS = [
  { pubkey: ADA, name: "Ada", homeRole: "builder" },
  { pubkey: BEN, name: "Ben" },
];

async function mountDraft() {
  const React = (await import("react")).default;
  const { act, cleanup, render } = await import("@testing-library/react");
  const { useCodingSessionSeatDraft } = await import(
    "./useCodingSessionSeatDraft.ts"
  );
  let latest = null;
  function Probe() {
    latest = useCodingSessionSeatDraft(AGENTS);
    return null;
  }
  await act(async () => {
    render(React.createElement(Probe));
  });
  return {
    cleanup,
    act,
    read: () => latest,
  };
}

test("choosing an agent fills the role from its home role, and clearing forgets it", async () => {
  const { act, cleanup, read } = await mountDraft();
  try {
    assert.equal(read().role, "");
    assert.equal(read().seat, null);
    assert.equal(read().error, null);

    await act(async () => read().onActorChange(ADA));
    assert.equal(read().role, "builder");
    assert.deepEqual(read().seat, { actor: ADA, role: "builder" });
    assert.equal(read().label, "Ada");

    // An agent whose home role this build cannot see fills nothing — and a
    // seat with no role is refused before anything is signed.
    await act(async () => read().onActorChange(BEN));
    assert.equal(read().role, "");
    assert.equal(read().seat, null);
    assert.match(read().error, /Give this seat a role/);

    await act(async () => read().onActorChange(null));
    assert.equal(read().role, "");
    assert.equal(read().error, null);
  } finally {
    cleanup();
  }
});

test("a role the person typed survives switching agents", async () => {
  const { act, cleanup, read } = await mountDraft();
  try {
    await act(async () => read().onActorChange(ADA));
    await act(async () => read().onRoleChange("lead"));
    assert.equal(read().role, "lead");

    await act(async () => read().onActorChange(BEN));
    assert.equal(read().role, "lead");
    assert.deepEqual(read().seat, { actor: BEN, role: "lead" });

    // Clearing the seat is the one thing that forgets the typed role.
    await act(async () => read().onActorChange(null));
    await act(async () => read().onActorChange(ADA));
    assert.equal(read().role, "builder");
  } finally {
    cleanup();
  }
});
