/**
 * The founding hook's own re-entrancy guard.
 *
 * `isFounding` is React state, set after the first await has rendered; a
 * second `found` in the same tick would read it as false. The guard that
 * matters is the ref, and what it must do is hand the second caller the
 * first call's promise — not a second genesis, and not a rejection.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";
import React from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";

import { foundCodingSessionTopic } from "../lib/codingSessionTopicFounding.ts";
import { useCodingSessionTopicFounding } from "./useCodingSessionTopicFounding.ts";

const dom = new JSDOM(
  "<!doctype html><html><body><div id='root'></div></body></html>",
  { url: "http://localhost" },
);

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

after(() => dom.window.close());

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";

const INPUT = {
  channelId: CHANNEL_ID,
  goal: "",
  title: null,
  projectRef: null,
  repoRef: null,
};

test("a second found while one is in flight returns the first call's promise", async () => {
  let genesisCalls = 0;
  let releaseGenesis;
  const gate = new Promise((resolve) => {
    releaseGenesis = resolve;
  });
  let refs = 0;
  const deps = {
    runFounding: foundCodingSessionTopic,
    newSessionRef: () => {
      refs += 1;
      return `ref-${refs}`;
    },
    publishGenesis: async () => {
      genesisCalls += 1;
      await gate;
      return { eventId: `genesis-${genesisCalls}` };
    },
    publishGoal: async () => {},
    publishName: async () => {},
  };

  const seen = {};
  function Probe() {
    seen.hook = useCodingSessionTopicFounding({ deps });
    return null;
  }
  const root = createRoot(dom.window.document.getElementById("root"));
  await act(async () => {
    root.render(React.createElement(Probe));
  });

  let first;
  let second;
  await act(async () => {
    first = seen.hook.found(INPUT);
    second = seen.hook.found(INPUT);
  });
  assert.equal(first, second, "the same promise, not a second run");
  assert.equal(genesisCalls, 1);
  assert.equal(refs, 1, "one session ref minted");

  await act(async () => {
    releaseGenesis();
  });
  const [a, b] = await Promise.all([first, second]);
  assert.equal(a, b);
  assert.equal(a.ok, true);
  assert.equal(a.sessionRef, "ref-1");
  assert.equal(a.genesisRef, "genesis-1");

  // Once settled, the next call is a new founding.
  let third;
  await act(async () => {
    third = seen.hook.found(INPUT);
  });
  assert.notEqual(third, first);
  const c = await third;
  assert.equal(genesisCalls, 2);
  assert.equal(c.sessionRef, "ref-2");

  await act(async () => root.unmount());
});

test("a founding that settled with a failure releases the guard", async () => {
  const deps = {
    runFounding: foundCodingSessionTopic,
    newSessionRef: () => "ref",
    publishGenesis: async () => {
      throw new Error("auth-required");
    },
    publishGoal: async () => {},
    publishName: async () => {},
  };
  const seen = {};
  function Probe() {
    seen.hook = useCodingSessionTopicFounding({ deps });
    return null;
  }
  const root = createRoot(dom.window.document.getElementById("root"));
  await act(async () => {
    root.render(React.createElement(Probe));
  });
  let first;
  await act(async () => {
    first = seen.hook.found(INPUT);
  });
  const result = await first;
  assert.equal(result.ok, false);
  let second;
  await act(async () => {
    second = seen.hook.found(INPUT);
  });
  assert.notEqual(second, first);
  assert.equal((await second).ok, false);
  await act(async () => root.unmount());
});
