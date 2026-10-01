import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_FULL_ACCESS_BADGE,
  CODING_SESSION_FULL_ACCESS_LABEL,
  codingSessionFullAccessDetail,
  codingSessionFullAccessFailure,
} from "./codingSessionFullAccess.ts";

const idle = { pending: null, error: null };

test("labels name the grant plainly", () => {
  assert.equal(
    CODING_SESSION_FULL_ACCESS_LABEL,
    "Full access to this computer",
  );
  assert.equal(CODING_SESSION_FULL_ACCESS_BADGE, "Full access");
});

test("off explains what turning it on does, including the restart", () => {
  assert.equal(
    codingSessionFullAccessDetail({ ...idle, granted: false }),
    "Let this agent install tools and work outside the project. Restarts the agent.",
  );
});

test("on says the agent is outside the sandbox and how to undo it", () => {
  assert.equal(
    codingSessionFullAccessDetail({ ...idle, granted: true }),
    "On — this agent runs outside the sandbox. Turn off to restart it sandboxed.",
  );
});

test("a change in flight names the direction, not the old state", () => {
  assert.match(
    codingSessionFullAccessDetail({
      granted: false,
      pending: true,
      error: null,
    }),
    /^Turning on — restarting/,
  );
  assert.match(
    codingSessionFullAccessDetail({
      granted: true,
      pending: false,
      error: null,
    }),
    /^Turning off — restarting/,
  );
});

test("a failed change shows the cause verbatim", () => {
  const cause =
    "a turn is open on this execution; wait for it to end or interrupt it, then restart";
  assert.equal(
    codingSessionFullAccessDetail({
      granted: false,
      pending: null,
      error: cause,
    }),
    `Not changed: ${cause}`,
  );
});

test("a failure whose revert succeeded is just the cause", () => {
  assert.equal(
    codingSessionFullAccessFailure({
      granted: true,
      cause: "SESSION_BUSY text",
      revertError: null,
    }),
    "SESSION_BUSY text",
  );
});

test("a failure whose revert also failed says what will happen at next start", () => {
  const on = codingSessionFullAccessFailure({
    granted: true,
    cause: "refused.",
    revertError: "disk full",
  });
  assert.ok(on.includes("disk full"));
  assert.ok(on.includes("full access stays granted"));
  assert.ok(on.includes("next time this agent starts"));
  const off = codingSessionFullAccessFailure({
    granted: false,
    cause: "refused.",
    revertError: " ",
  });
  assert.ok(off.includes("stays withdrawn"));
  assert.ok(off.includes("no reason given"));
});

test("an empty cause still reads as a failure", () => {
  assert.equal(
    codingSessionFullAccessFailure({
      granted: true,
      cause: " ",
      revertError: null,
    }),
    "the change did not complete",
  );
});
