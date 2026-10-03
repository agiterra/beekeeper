import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_BOUNDARY_STATUSES,
  CODING_SESSION_BOUNDARY_TITLE,
  CODING_SESSION_FULL_ACCESS_BOUNDARY_TEXT,
  CODING_SESSION_FULL_ACCESS_REASON,
  CODING_SESSION_ISOLATION_STATUSES,
  CODING_SESSION_ISOLATION_TITLE,
  codingSessionBoundaryText,
} from "./codingSessionBoundaryStatus.ts";
import { buildBaseTranscriptItem } from "./codingSessionTranscriptItems.ts";

const ENFORCED = "execution_boundary_enforced";
const NOT_ENFORCED = "execution_boundary_not_enforced";

const IDENTITY = {
  id: "item-1",
  sessionId: "session-1",
  targetKey: "target-1",
  channelId: "channel-1",
  timestamp: "2026-10-01T00:00:00.000Z",
};

test("the reason slug matches the provider's FULL_ACCESS_REASON", () => {
  // crates/buzz-session-provider/src/full_access.rs
  assert.equal(CODING_SESSION_FULL_ACCESS_REASON, "full-access");
});

test("a full-access generation says the sandbox is off and why", () => {
  const text = codingSessionBoundaryText(NOT_ENFORCED, "full-access");
  assert.equal(text, CODING_SESSION_FULL_ACCESS_BOUNDARY_TEXT);
  assert.ok(text.startsWith("Sandbox off — "));
  assert.ok(text.includes("full access to the computer it runs on"));
  assert.ok(text.includes("other projects' files"));
});

test("full access never reads as protected, whichever status carries it", () => {
  const enforcedText = CODING_SESSION_BOUNDARY_STATUSES.get(ENFORCED);
  for (const status of [ENFORCED, NOT_ENFORCED]) {
    const text = codingSessionBoundaryText(status, "full-access");
    assert.equal(text, CODING_SESSION_FULL_ACCESS_BOUNDARY_TEXT, status);
    assert.ok(!text.startsWith("Enforced"), status);
    assert.ok(!text.includes(enforcedText), status);
    assert.ok(!text.includes("inside this project's boundary"), status);
  }
});

test("full access does not claim the viewer granted it", () => {
  // The grant is made on the provider's computer; a teammate elsewhere did not.
  assert.ok(!CODING_SESSION_FULL_ACCESS_BOUNDARY_TEXT.includes("by you"));
});

test("a full-access status item renders through the transcript as the boundary row", () => {
  const item = buildBaseTranscriptItem(
    { kind: "status", status: NOT_ENFORCED, reason: "full-access" },
    IDENTITY,
  );
  assert.equal(item.title, CODING_SESSION_BOUNDARY_TITLE);
  assert.equal(item.text, CODING_SESSION_FULL_ACCESS_BOUNDARY_TEXT);
});

test("other reasons are unchanged by the full-access case", () => {
  assert.equal(
    codingSessionBoundaryText(ENFORCED, "macos-seatbelt"),
    `${CODING_SESSION_BOUNDARY_STATUSES.get(ENFORCED)} (macOS Seatbelt)`,
  );
  assert.ok(
    codingSessionBoundaryText(NOT_ENFORCED, "full-access-plus").endsWith(
      "(full-access-plus)",
    ),
  );
  assert.equal(
    codingSessionBoundaryText("session_fresh", "full-access"),
    undefined,
  );
});

test("the provider's isolation statuses render as session isolation rows", () => {
  // crates/buzz-session-provider/src/session_isolation.rs
  const expected = [
    [
      "operator_git_withheld",
      "provider-setting",
      "Git credentials from this computer were withheld from this session",
    ],
    [
      "network_egress_proxy_only",
      "loopback-proxy",
      "This session can reach the network only through the provider's egress proxy",
    ],
  ];
  assert.equal(CODING_SESSION_ISOLATION_STATUSES.size, expected.length);
  for (const [status, reason, text] of expected) {
    const item = buildBaseTranscriptItem(
      { kind: "status", status, reason },
      IDENTITY,
    );
    assert.equal(item.title, CODING_SESSION_ISOLATION_TITLE);
    assert.equal(item.text, text);
    assert.equal(codingSessionBoundaryText(status, reason), undefined);
  }
});
