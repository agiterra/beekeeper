import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";
import React, { act } from "react";

import { CODING_SESSION_BOUNDARY_TITLE } from "../lib/codingSessionBoundaryStatus.ts";
import { CODING_SESSION_CONTINUITY_TITLE } from "../lib/codingSessionTranscriptItems.ts";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    window: dom.window,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
});

after(() => dom.window.close());

const timestamp = "2026-10-04T12:00:00.000Z";
const boundary = {
  id: "boundary",
  type: "lifecycle",
  renderClass: "status",
  title: CODING_SESSION_BOUNDARY_TITLE,
  text: "Sandboxed",
  timestamp,
  turnId: "turn-1",
};
const continuity = {
  id: "continuity",
  type: "lifecycle",
  renderClass: "status",
  title: CODING_SESSION_CONTINUITY_TITLE,
  text: "Fresh session",
  timestamp,
  turnId: "turn-1",
};
const prompt = {
  id: "prompt",
  type: "message",
  renderClass: "message",
  role: "user",
  title: "Brian",
  text: "Go",
  timestamp,
  turnId: "turn-1",
};
const streamed = (index) => ({
  id: `answer-${index}`,
  type: "message",
  renderClass: "message",
  role: "assistant",
  title: "Assistant",
  text: `chunk ${index}`,
  timestamp,
  turnId: "turn-1",
});

/**
 * The transcript as a proxy that counts every element read, so a pass over
 * it — `for…of`, `map`, an index loop — is counted whoever makes it.
 */
function counted(items, counter) {
  return new Proxy(items, {
    get(target, key, receiver) {
      if (typeof key === "string" && /^\d+$/.test(key)) counter.reads += 1;
      return Reflect.get(target, key, receiver);
    },
  });
}

async function renderBurst(withFacts) {
  const { createRoot } = await import("react-dom/client");
  const { useStableCodingSessionTranscriptModel } = await import(
    "./CodingSessionTranscript.tsx"
  );
  const { useCodingSessionWorkspaceSessionFacts } = await import(
    "./CodingSessionWorkspaceSessionFacts.ts"
  );
  const seen = [];
  function ModelOnly({ transcript }) {
    useStableCodingSessionTranscriptModel(transcript, true);
    return null;
  }
  function ModelAndFacts({ transcript }) {
    const model = useStableCodingSessionTranscriptModel(transcript, true);
    seen.push(useCodingSessionWorkspaceSessionFacts(model.sessionFacts, null));
    return null;
  }
  const Harness = withFacts ? ModelAndFacts : ModelOnly;
  const counter = { reads: 0 };
  const root = createRoot(document.createElement("div"));
  let items = [continuity, prompt, boundary];
  await act(async () => {
    root.render(
      React.createElement(Harness, { transcript: counted(items, counter) }),
    );
  });
  // A burst of streamed items that are not facts, one render each.
  for (let index = 0; index < 40; index += 1) {
    items = [...items, streamed(index)];
    await act(async () => {
      root.render(
        React.createElement(Harness, { transcript: counted(items, counter) }),
      );
    });
  }
  await act(async () => root.unmount());
  return { reads: counter.reads, seen };
}

test("session facts add no transcript scan to a burst of streamed appends", async () => {
  const modelOnly = await renderBurst(false);
  const withFacts = await renderBurst(true);
  // The model's own passes are the only reads: the facts come from
  // `model.sessionFacts`, not from another pass per streamed item.
  assert.ok(modelOnly.reads > 0, "the probe counts the model's passes");
  assert.equal(withFacts.reads, modelOnly.reads);

  // No streamed item was a fact, so the derived rows and the sandbox keep
  // their identity across the whole burst — nothing downstream re-renders.
  const first = withFacts.seen[0];
  assert.ok(first.continuity.length > 0);
  for (const facts of withFacts.seen) {
    assert.equal(facts.continuity, first.continuity);
    assert.equal(facts.sandbox, first.sandbox);
  }
});
