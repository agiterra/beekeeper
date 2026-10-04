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
  codingSessionEarlierUnsandboxedPeriod,
  codingSessionSandboxChip,
  codingSessionSandboxFromTranscript,
  codingSessionSandboxStateFromBoundaryText,
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

// SV-17: the sandbox as a composer chip, read from the transcript's rows.

function lifecycle(id, title, text) {
  return { id, type: "lifecycle", title, text };
}

test("each boundary text classifies to one sandbox state", () => {
  assert.equal(
    codingSessionSandboxStateFromBoundaryText(
      codingSessionBoundaryText(ENFORCED, "macos-seatbelt"),
    ),
    "sandboxed",
  );
  assert.equal(
    codingSessionSandboxStateFromBoundaryText(
      codingSessionBoundaryText(NOT_ENFORCED, "no-backend-for-platform"),
    ),
    "not-sandboxed",
  );
  for (const status of [ENFORCED, NOT_ENFORCED]) {
    assert.equal(
      codingSessionSandboxStateFromBoundaryText(
        codingSessionBoundaryText(status, "full-access"),
      ),
      "full-access",
    );
  }
  // Text this build did not write is never read as protection.
  assert.equal(
    codingSessionSandboxStateFromBoundaryText("Enforcement pending"),
    "unreported",
  );
});

test("the newest boundary row describes the running agent, with its isolation rows", () => {
  const items = [
    lifecycle(
      "a",
      CODING_SESSION_BOUNDARY_TITLE,
      codingSessionBoundaryText(NOT_ENFORCED, "full-access"),
    ),
    lifecycle(
      "b",
      CODING_SESSION_BOUNDARY_TITLE,
      codingSessionBoundaryText(ENFORCED, "macos-seatbelt"),
    ),
    lifecycle(
      "c",
      CODING_SESSION_ISOLATION_TITLE,
      CODING_SESSION_ISOLATION_STATUSES.get("operator_git_withheld"),
    ),
    { id: "d", type: "message", text: "hello" },
  ];
  const report = codingSessionSandboxFromTranscript(items);
  assert.equal(report.state, "sandboxed");
  assert.match(report.boundaryText, /^Enforced/);
  assert.deepEqual(report.isolation, [
    CODING_SESSION_ISOLATION_STATUSES.get("operator_git_withheld"),
  ]);
});

test("no boundary row reads as unreported, never as sandboxed", () => {
  const report = codingSessionSandboxFromTranscript([
    { id: "x", type: "message", text: "hi" },
  ]);
  assert.deepEqual(report, {
    state: "unreported",
    boundaryText: null,
    isolation: [],
  });
  const chip = codingSessionSandboxChip(report, null);
  assert.equal(chip.label, "Sandbox unreported");
  assert.equal(chip.tone, "muted");
});

test("full access always wears the warning, from the transcript or the local grant", () => {
  const full = { state: "full-access", boundaryText: "x", isolation: [] };
  const sandboxed = { state: "sandboxed", boundaryText: "x", isolation: [] };
  const unreported = { state: "unreported", boundaryText: null, isolation: [] };
  const grant = { granted: true, pending: null, error: null };
  const noGrant = { granted: false, pending: null, error: null };

  for (const [report, local] of [
    [full, null],
    [full, noGrant],
    [sandboxed, grant],
    [unreported, grant],
  ]) {
    const chip = codingSessionSandboxChip(report, local);
    assert.equal(chip.label, "Full access");
    assert.equal(chip.tone, "warning");
  }
  // The running agent is sandboxed but the grant waits for its next start:
  // the dropdown says which is which.
  assert.match(
    codingSessionSandboxChip(sandboxed, grant).summary,
    /still sandboxed; the grant applies the next time it starts/,
  );

  const safe = codingSessionSandboxChip(sandboxed, noGrant);
  assert.equal(safe.label, "Sandboxed");
  assert.equal(safe.tone, "safe");
});

test("a change in flight is a warning both ways, never Sandboxed", () => {
  const sandboxed = { state: "sandboxed", boundaryText: "x", isolation: [] };
  const full = { state: "full-access", boundaryText: "x", isolation: [] };
  const on = codingSessionSandboxChip(sandboxed, {
    granted: false,
    pending: true,
    error: null,
  });
  assert.equal(on.tone, "warning");
  const off = codingSessionSandboxChip(full, {
    granted: true,
    pending: false,
    error: null,
  });
  assert.equal(off.tone, "warning");
  assert.match(off.summary, /still runs with full access/);
});

test("an unenforced boundary for any other reason is a warning too", () => {
  const chip = codingSessionSandboxChip(
    { state: "not-sandboxed", boundaryText: "x", isolation: [] },
    null,
  );
  assert.equal(chip.label, "Not sandboxed");
  assert.equal(chip.tone, "warning");
});

test("an earlier full-access period outlives the restart into a sandbox (SV-17)", () => {
  const items = [
    {
      ...lifecycle(
        "a",
        CODING_SESSION_BOUNDARY_TITLE,
        codingSessionBoundaryText(NOT_ENFORCED, "full-access"),
      ),
      timestamp: "2026-10-04T09:00:00.000Z",
    },
    { id: "m", type: "message", text: "work" },
    lifecycle(
      "b",
      CODING_SESSION_BOUNDARY_TITLE,
      codingSessionBoundaryText(ENFORCED, "macos-seatbelt"),
    ),
    lifecycle(
      "c",
      CODING_SESSION_ISOLATION_TITLE,
      CODING_SESSION_ISOLATION_STATUSES.get("operator_git_withheld"),
    ),
  ];
  const report = codingSessionSandboxFromTranscript(items);
  assert.equal(report.state, "sandboxed");
  assert.equal(report.isolation.length, 1);
  assert.deepEqual(
    report.earlier.map((period) => [period.id, period.state, period.timestamp]),
    [["a", "full-access", "2026-10-04T09:00:00.000Z"]],
  );
  assert.equal(codingSessionEarlierUnsandboxedPeriod(report)?.id, "a");

  const chip = codingSessionSandboxChip(report, null);
  assert.equal(chip.label, "Sandboxed · was full access");
  assert.equal(chip.tone, "warning");
  assert.match(chip.summary, /Earlier in this session it ran with full access/);
});

test("an earlier unenforced period marks the chip too; a sandboxed history does not", () => {
  const notEnforced = lifecycle(
    "a",
    CODING_SESSION_BOUNDARY_TITLE,
    codingSessionBoundaryText(NOT_ENFORCED, "no-backend-for-platform"),
  );
  const enforced = (id) =>
    lifecycle(
      id,
      CODING_SESSION_BOUNDARY_TITLE,
      codingSessionBoundaryText(ENFORCED, "macos-seatbelt"),
    );
  const exposed = codingSessionSandboxChip(
    codingSessionSandboxFromTranscript([notEnforced, enforced("b")]),
    null,
  );
  assert.equal(exposed.label, "Sandboxed · was not sandboxed");
  assert.equal(exposed.tone, "warning");

  const clean = codingSessionSandboxChip(
    codingSessionSandboxFromTranscript([enforced("a"), enforced("b")]),
    null,
  );
  assert.equal(clean.label, "Sandboxed");
  assert.equal(clean.tone, "safe");
});

test("an unrecognised current report after full access is never muted", () => {
  const report = codingSessionSandboxFromTranscript([
    lifecycle(
      "a",
      CODING_SESSION_BOUNDARY_TITLE,
      codingSessionBoundaryText(NOT_ENFORCED, "full-access"),
    ),
    lifecycle("b", CODING_SESSION_BOUNDARY_TITLE, "something new"),
  ]);
  assert.equal(report.state, "unreported");
  const chip = codingSessionSandboxChip(report, null);
  assert.equal(chip.tone, "warning");
  assert.equal(chip.label, "Sandbox unreported · was full access");
});
