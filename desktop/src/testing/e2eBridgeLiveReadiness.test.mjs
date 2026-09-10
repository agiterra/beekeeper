import assert from "node:assert/strict";
import test from "node:test";
import { mockLiveReadinessMatches as ready } from "./e2eBridgeLiveReadiness.ts";

const background = { kinds: [9, 40002], "#h": ["general", "random"] };
const windowFilter = { kinds: [9, 40002, 39005], "#h": ["general"] };

test("metadata and unrelated global consumers cannot announce message readiness", () => {
  assert.equal(ready([{ kinds: [39000] }], "general", undefined, false), false);
  assert.equal(ready([{ kinds: [9] }], "general", undefined, false), false);
  assert.equal(
    ready([{ kinds: [9], "#h": ["other"] }], "general", undefined, false),
    false,
  );
});

test("inactive channels accept their actual bundled message consumer", () => {
  assert.equal(ready([background], "random", undefined, false), true);
  assert.equal(ready([background], "other", undefined, false), false);
});

test("active timeline readiness requires the window consumer instead of the unread feed", () => {
  assert.equal(ready([background], "general", undefined, true), false);
  assert.equal(ready([windowFilter], "general", undefined, true), true);
  assert.equal(
    ready(
      [{ ...windowFilter, "#h": ["general", "other"] }],
      "general",
      undefined,
      true,
    ),
    false,
  );
});

test("separate REQ filters cannot cross-product a channel with another filter's kind", () => {
  assert.equal(
    ready(
      [
        { kinds: [9], "#h": ["other"] },
        { kinds: [39000], "#h": ["general"] },
      ],
      "general",
      undefined,
      false,
    ),
    false,
  );
});

test("thread, recipient and author subsets cannot promise arbitrary message delivery", () => {
  for (const subset of [
    { "#e": ["thread"] },
    { "#p": ["person"] },
    { authors: ["person"] },
  ]) {
    assert.equal(
      ready([{ ...windowFilter, ...subset }], "general", undefined, true),
      false,
    );
  }
});

test("explicit non-message kinds retain their own channel consumer readiness", () => {
  const huddle = { kinds: [48103], "#h": ["general"] };
  assert.equal(ready([huddle], "general", 48103, true), true);
  assert.equal(ready([huddle], "general", 48100, true), false);
  assert.equal(ready([huddle], "general", undefined, true), false);
});

test("an explicit non-message kind can name an author-scoped consumer", () => {
  assert.equal(
    ready(
      [{ kinds: [44226], "#h": ["general"], authors: ["founder"] }],
      "general",
      44226,
      true,
    ),
    true,
  );
});

test("explicit read-state readiness uses its identity-scoped global consumer", () => {
  const reads = [{ kinds: [30078], authors: ["viewer"] }];
  assert.equal(ready(reads, "general", 30078, true), true);
  assert.equal(ready(reads, "general", undefined, true), false);
  assert.equal(ready(reads, "general", 44226, true), false);
  assert.equal(ready([{ kinds: [39000] }], "general", 30078, true), false);
});

test("huddle readiness waits for the lifecycle consumer, not the broad activity feed", () => {
  assert.equal(
    ready(
      [{ kinds: [9, 48100, 48103], "#h": ["general"] }],
      "general",
      48100,
      true,
    ),
    false,
  );
  assert.equal(
    ready(
      [{ kinds: [48100, 48101, 48102, 48103], "#h": ["general"] }],
      "general",
      48100,
      true,
    ),
    true,
  );
});
