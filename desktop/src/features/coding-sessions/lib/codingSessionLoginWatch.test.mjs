import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_LOGIN_WATCH_INTERVAL_MS,
  CODING_SESSION_LOGIN_WATCH_TIMEOUT_MS,
  codingSessionLoginWatchVerdict,
} from "./codingSessionLoginWatch.ts";

const T0 = 1_700_000_000_000;

test("the watch continues while the runtime stays signed out", () => {
  assert.equal(
    codingSessionLoginWatchVerdict({
      runtime: "claude",
      startedAt: T0,
      now: T0 + CODING_SESSION_LOGIN_WATCH_INTERVAL_MS,
      runtimes: [{ runtime: "claude", authState: "needs_auth" }],
    }),
    "continue",
  );
});

test("the watch ends when the launched runtime reports ready", () => {
  assert.equal(
    codingSessionLoginWatchVerdict({
      runtime: "claude",
      startedAt: T0,
      now: T0 + 10_000,
      runtimes: [
        { runtime: "codex", authState: "needs_auth" },
        { runtime: "claude", authState: "ready" },
      ],
    }),
    "ready",
  );
});

test("some other runtime being ready does not end the watch", () => {
  assert.equal(
    codingSessionLoginWatchVerdict({
      runtime: "codex",
      startedAt: T0,
      now: T0 + 10_000,
      runtimes: [
        { runtime: "claude", authState: "ready" },
        { runtime: "codex", authState: "needs_auth" },
      ],
    }),
    "continue",
  );
});

test("an abandoned login expires instead of polling forever", () => {
  assert.equal(
    codingSessionLoginWatchVerdict({
      runtime: "claude",
      startedAt: T0,
      now: T0 + CODING_SESSION_LOGIN_WATCH_TIMEOUT_MS,
      runtimes: [{ runtime: "claude", authState: "needs_auth" }],
    }),
    "expired",
  );
});

test("a login completing on the final tick still counts as ready", () => {
  assert.equal(
    codingSessionLoginWatchVerdict({
      runtime: "claude",
      startedAt: T0,
      now: T0 + CODING_SESSION_LOGIN_WATCH_TIMEOUT_MS + 1,
      runtimes: [{ runtime: "claude", authState: "ready" }],
    }),
    "ready",
  );
});
