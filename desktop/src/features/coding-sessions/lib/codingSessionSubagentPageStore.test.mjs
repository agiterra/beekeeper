import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import {
  closeCodingSessionSubagentPage,
  codingSessionSubagentPageScopeKey,
  openCodingSessionSubagentPage,
  readCodingSessionSubagentPage,
  resetCodingSessionSubagentPages,
  subscribeCodingSessionSubagentPages,
  takeCodingSessionSubagentPageReturn,
} from "./codingSessionSubagentPageStore.ts";

afterEach(() => resetCodingSessionSubagentPages());

const scope = (overrides = {}) =>
  codingSessionSubagentPageScopeKey({
    communityScope: "wss://hive",
    channelId: "c1",
    sessionKey: "s1",
    layout: "single",
    generationId: "g1",
    ...overrides,
  });

test("each session view keeps its own open page", () => {
  openCodingSessionSubagentPage(scope(), "task-1");
  assert.equal(readCodingSessionSubagentPage(scope()), "task-1");
  assert.equal(readCodingSessionSubagentPage(scope({ channelId: "c2" })), null);
  assert.equal(
    readCodingSessionSubagentPage(scope({ layout: "umbrella" })),
    null,
  );
  assert.equal(
    readCodingSessionSubagentPage(scope({ communityScope: "wss://other" })),
    null,
  );
});

test("the open page survives until closed, and closing hands the parent its row once", () => {
  let notified = 0;
  const unsubscribe = subscribeCodingSessionSubagentPages(() => {
    notified += 1;
  });
  openCodingSessionSubagentPage(scope(), "task-1");
  openCodingSessionSubagentPage(scope(), "task-1"); // no-op, no notification
  assert.equal(notified, 1);
  closeCodingSessionSubagentPage(scope(), "call-item-1");
  assert.equal(notified, 2);
  assert.equal(readCodingSessionSubagentPage(scope()), null);
  assert.equal(takeCodingSessionSubagentPageReturn(scope()), "call-item-1");
  assert.equal(takeCodingSessionSubagentPageReturn(scope()), null);
  unsubscribe();
});

test("opening another page drops a pending return; a blank id opens nothing", () => {
  closeCodingSessionSubagentPage(scope(), "call-item-1");
  openCodingSessionSubagentPage(scope(), "task-2");
  assert.equal(takeCodingSessionSubagentPageReturn(scope()), null);
  openCodingSessionSubagentPage(scope({ channelId: "c9" }), "  ");
  assert.equal(readCodingSessionSubagentPage(scope({ channelId: "c9" })), null);
});

test("a community switch forgets every open page", () => {
  openCodingSessionSubagentPage(scope(), "task-1");
  resetCodingSessionSubagentPages();
  assert.equal(readCodingSessionSubagentPage(scope()), null);
});
