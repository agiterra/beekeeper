import assert from "node:assert/strict";
import { test } from "node:test";

import {
  APP_SOFTWARE_URL,
  relayBuildDrift,
  relayBuildDriftDetail,
  relayBuildDriftNotice,
} from "./relayBuildDrift.ts";

const APP_SHA = "6a683c9e3f1a4b2c8d9e0f1a2b3c4d5e6f7a8b9c";
const RELAY_SHA = "42dd921d831c483e6e16111491b39947b4cf1f86";

const drift = ({
  appCommit = APP_SHA,
  appCount = 3279,
  appDirty = null,
  relayCommit = RELAY_SHA,
  relayCount = 3291,
  software = APP_SOFTWARE_URL,
} = {}) =>
  relayBuildDrift({
    app: { commit: appCommit, commitCount: appCount, sourceDirty: appDirty },
    relay: { commit: relayCommit, commitCount: relayCount, software },
  });

test("behind reports the delta and names its method", () => {
  assert.deepEqual(drift(), {
    state: "behind",
    method: "ordinal",
    commits: 12,
  });
});

test("ahead is reported but never notified — that is a local dev build", () => {
  const d = drift({ appCount: 3300 });
  assert.deepEqual(d, { state: "ahead", method: "ordinal", commits: 9 });
  assert.equal(relayBuildDriftNotice(d, true), null);
});

test("equal commits win over mismatched counts, and render no number", () => {
  const d = drift({ appCommit: RELAY_SHA, appCount: 1, relayCount: 9999 });
  assert.deepEqual(d, { state: "same", method: "commit" });
  assert.equal(relayBuildDriftNotice(d, true), null);
  assert.ok(
    !/\d/.test(relayBuildDriftDetail(d)),
    "a contradicted count must not be shown",
  );
});

test("equal counts with different commits is divergence, not agreement", () => {
  assert.deepEqual(drift({ appCount: 3291 }), {
    state: "unknown",
    method: "none",
    reason: "divergent-equal-ordinal",
  });
});

test("a dirty app build refuses to compare at all", () => {
  assert.deepEqual(drift({ appDirty: true }), {
    state: "unknown",
    method: "none",
    reason: "app-source-dirty",
  });
});

test("a null count is unknown, never coerced to zero", () => {
  // The bug this guards: `3291 - null` is 3291 in JS, so an unguarded
  // subtraction would confidently announce the relay's whole ordinal.
  assert.deepEqual(drift({ appCount: null }), {
    state: "unknown",
    method: "none",
    reason: "app-count-unknown",
  });
  assert.deepEqual(drift({ relayCount: null }), {
    state: "unknown",
    method: "none",
    reason: "relay-count-unknown",
  });
});

test("a zero or fractional count is refused like a missing one", () => {
  for (const bad of [0, -1, 1.5, Number.NaN]) {
    assert.equal(
      drift({ relayCount: bad }).state,
      "unknown",
      `relay count ${bad}`,
    );
    assert.equal(drift({ appCount: bad }).state, "unknown", `app count ${bad}`);
  }
});

test("a missing commit on either side blocks the comparison", () => {
  assert.equal(drift({ appCommit: null }).reason, "app-commit-unknown");
  assert.equal(drift({ relayCommit: null }).reason, "relay-commit-unknown");
});

test("a different source repository is not comparable", () => {
  assert.deepEqual(drift({ software: "https://github.com/block/buzz" }), {
    state: "unknown",
    method: "none",
    reason: "different-software",
  });
});

test("an undisclosed software URL is not treated as evidence of difference", () => {
  assert.equal(drift({ software: null }).state, "behind");
});

test("a trailing slash or different case is the same repository", () => {
  assert.equal(drift({ software: `${APP_SOFTWARE_URL}/` }).state, "behind");
  assert.equal(
    drift({ software: APP_SOFTWARE_URL.toUpperCase() }).state,
    "behind",
  );
});

test("the notice carries the method into the sentence the user reads", () => {
  const notice = relayBuildDriftNotice(drift(), true);
  assert.match(notice.description, /by commit count/);
  assert.match(notice.description, /12 commits/);
});

test("one commit behind is singular", () => {
  const notice = relayBuildDriftNotice(drift({ appCount: 3290 }), true);
  assert.match(notice.description, /1 commit ahead/);
  assert.doesNotMatch(notice.description, /1 commits/);
});

test("behind with no update available says so rather than implying an action", () => {
  const notice = relayBuildDriftNotice(drift(), false);
  assert.match(notice.description, /No app update is available yet/);
});

test("only behind ever produces a notice", () => {
  const states = [
    drift({ appCommit: RELAY_SHA }),
    drift({ appCount: 3300 }),
    drift({ appDirty: true }),
    drift({ relayCount: null }),
    drift({ software: "https://example.invalid/other" }),
  ];
  for (const d of states) {
    assert.equal(
      relayBuildDriftNotice(d, true),
      null,
      `${d.state}/${d.reason ?? ""}`,
    );
  }
});

test("every state has a settings sentence, and none is a bare yes/no", () => {
  const reasons = [
    "app-commit-unknown",
    "app-count-unknown",
    "relay-commit-unknown",
    "relay-count-unknown",
    "app-source-dirty",
    "divergent-equal-ordinal",
    "different-software",
  ];
  const all = [
    { state: "same", method: "commit" },
    { state: "behind", method: "ordinal", commits: 12 },
    { state: "ahead", method: "ordinal", commits: 3 },
    ...reasons.map((reason) => ({ state: "unknown", method: "none", reason })),
  ];
  for (const d of all) {
    const sentence = relayBuildDriftDetail(d);
    assert.ok(
      sentence.length > 20,
      `${d.state}/${d.reason ?? ""} needs a real sentence`,
    );
    assert.ok(
      sentence.trim().endsWith("."),
      `${d.state}/${d.reason ?? ""} must be a sentence`,
    );
  }
});

test("the behind sentence discloses that ancestry was not checked", () => {
  const sentence = relayBuildDriftDetail(drift());
  assert.match(sentence, /ancestor/);
  assert.match(sentence, /cannot check ancestry/);
});
