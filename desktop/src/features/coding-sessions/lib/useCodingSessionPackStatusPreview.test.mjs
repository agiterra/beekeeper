import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

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

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

const AGENT = "c".repeat(64);

const RESULT = {
  schema: "buzz-coding-session-pack-status/v1",
  implementation: "buzz-core",
  hasSource: true,
  repo: `30617:${"a".repeat(64)}:agiterra-packs`,
  sha: "b".repeat(40),
  path: "personas/roles",
  role: "builder",
  rolePath: "personas/roles/builder",
  roleFound: true,
  overlayFromCheckout: false,
  note: "Staging personas/roles/builder from agiterra-packs.",
};

async function renderPreview(props, probe) {
  const { renderHook } = await import("@testing-library/react");
  const { useCodingSessionPackStatusPreview } = await import(
    "./useCodingSessionPackStatusPreview.ts"
  );
  return renderHook(
    (input) => useCodingSessionPackStatusPreview({ ...input, probe }),
    { initialProps: props },
  );
}

test("no project known yet: no probe is made, and nothing is shown", async () => {
  let called = false;
  const { result } = await renderPreview(
    { agentPubkey: AGENT, projectRef: null, role: "builder" },
    async () => {
      called = true;
      return RESULT;
    },
  );
  assert.equal(result.current, null);
  assert.equal(called, false);
});

test("no role typed yet: no probe is made either", async () => {
  let called = false;
  const { result } = await renderPreview(
    { agentPubkey: AGENT, projectRef: "30621:owner:agiterra", role: "" },
    async () => {
      called = true;
      return RESULT;
    },
  );
  assert.equal(result.current, null);
  assert.equal(called, false);
});

test("a probe that throws renders nothing — never an error in the form", async () => {
  const { act } = await import("@testing-library/react");
  const { result } = await renderPreview(
    { agentPubkey: AGENT, projectRef: "30621:owner:agiterra", role: "builder" },
    async () => {
      throw new Error("agent is not a managed agent on this computer");
    },
  );
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  assert.equal(result.current, null);
});

test("a real answer is returned once the probe settles", async () => {
  const { act } = await import("@testing-library/react");
  const { result } = await renderPreview(
    { agentPubkey: AGENT, projectRef: "30621:owner:agiterra", role: "builder" },
    async () => RESULT,
  );
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  assert.deepEqual(result.current, RESULT);
});

test("changing the role re-probes and answers for the new role", async () => {
  const { renderHook, act } = await import("@testing-library/react");
  const { useCodingSessionPackStatusPreview } = await import(
    "./useCodingSessionPackStatusPreview.ts"
  );
  const probe = async ({ role }) => ({ ...RESULT, role });
  const { result, rerender } = renderHook(
    (input) => useCodingSessionPackStatusPreview({ ...input, probe }),
    {
      initialProps: {
        agentPubkey: AGENT,
        projectRef: "30621:owner:agiterra",
        role: "builder",
      },
    },
  );

  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  assert.equal(result.current?.role, "builder");

  act(() => {
    rerender({
      agentPubkey: AGENT,
      projectRef: "30621:owner:agiterra",
      role: "lead",
    });
  });
  // Cleared synchronously with the role change, before the new probe settles
  // — never the previous role's answer shown under the new role's label.
  assert.equal(result.current, null);

  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  assert.equal(result.current?.role, "lead");
});

test("no agent seated yet: no probe is made, because there is no seat to preview", async () => {
  // The host previews *this agent at this role*. An unseated form has no
  // question to ask, and an answer computed for nobody would be a guess.
  let called = false;
  const { result } = await renderPreview(
    { agentPubkey: null, projectRef: "30621:owner:agiterra", role: "builder" },
    async () => {
      called = true;
      return RESULT;
    },
  );
  assert.equal(result.current, null);
  assert.equal(called, false);
});

test("the seated agent reaches the host, so the preview is about that seat", async () => {
  const { act } = await import("@testing-library/react");
  let seen = null;
  const { result } = await renderPreview(
    { agentPubkey: AGENT, projectRef: "30621:owner:agiterra", role: "builder" },
    async (input) => {
      seen = input;
      return RESULT;
    },
  );
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  assert.equal(seen?.agentPubkey, AGENT);
  assert.equal(seen?.role, "builder");
  assert.deepEqual(result.current, RESULT);
});
