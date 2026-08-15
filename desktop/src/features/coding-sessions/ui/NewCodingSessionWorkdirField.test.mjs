import assert from "node:assert/strict";
import test from "node:test";

import { preferredWorkdirPrefill } from "./NewCodingSessionWorkdirField.tsx";

// The prefill mirrors the provider's own resolution (remembered project dir,
// then channel dir), then falls back to the project's local repo checkout,
// and only then to "whatever directory some session used last".

const state = {
  byProject: { "aa:proj": { path: "/remembered/project" } },
  byChannel: { "chan-1": { path: "/remembered/channel" } },
  mru: [{ path: "/mru/latest" }],
};

test("a remembered project directory outranks everything", () => {
  assert.equal(
    preferredWorkdirPrefill({
      channelId: "chan-1",
      fallbackPath: "/repos/checkout",
      projectKey: "aa:proj",
      state,
    }),
    "/remembered/project",
  );
});

test("a remembered channel directory outranks the repo checkout", () => {
  assert.equal(
    preferredWorkdirPrefill({
      channelId: "chan-1",
      fallbackPath: "/repos/checkout",
      projectKey: null,
      state,
    }),
    "/remembered/channel",
  );
});

test("the repo checkout outranks an unrelated most-recently-used directory", () => {
  assert.equal(
    preferredWorkdirPrefill({
      channelId: "chan-unknown",
      fallbackPath: "/repos/checkout",
      projectKey: "aa:other",
      state,
    }),
    "/repos/checkout",
  );
});

test("with nothing remembered and no checkout, MRU still prefills", () => {
  assert.equal(
    preferredWorkdirPrefill({
      channelId: null,
      fallbackPath: null,
      projectKey: null,
      state,
    }),
    "/mru/latest",
  );
});

test("no sources at all leaves the field empty", () => {
  assert.equal(
    preferredWorkdirPrefill({
      channelId: null,
      fallbackPath: null,
      projectKey: null,
      state: { byProject: {}, byChannel: {}, mru: [] },
    }),
    "",
  );
});
