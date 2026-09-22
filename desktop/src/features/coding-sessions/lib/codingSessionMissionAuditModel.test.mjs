import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_AUDIT_LIMITS,
  deriveCodingSessionMissionAudit,
  roomDownloadCommand,
} from "./codingSessionMissionAuditModel.ts";

const START = Date.parse("2026-09-01T21:34:22.000Z");

function at(offsetSeconds) {
  return new Date(START + offsetSeconds * 1_000).toISOString();
}

/** `result: null` means the call is still open — no answer ever arrived. */
function tool(id, turnId, offsetSeconds, args, result = "") {
  return {
    id,
    type: "tool",
    renderClass: "generic",
    descriptor: { renderClass: "generic", label: "Ran tool", preview: null },
    title: id,
    toolName: id,
    buzzToolName: null,
    status: result === null ? "executing" : "completed",
    args,
    result: result ?? "",
    isError: false,
    timestamp: at(offsetSeconds),
    startedAt: at(offsetSeconds),
    completedAt: at(offsetSeconds),
    turnId,
  };
}

function turnResult(id, turnId, offsetSeconds, { durationMs, usage, costUsd }) {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title: "Turn result",
    text: "done",
    outcome: "success",
    durationMs,
    costUsd: costUsd ?? null,
    timestamp: at(offsetSeconds),
    turnId,
    ...(usage ? { usage } : {}),
  };
}

function contextWindow(id, turnId, offsetSeconds, used, size) {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title: "Context Window Updated",
    text: `used: ${used}\nsize: ${size}`,
    timestamp: at(offsetSeconds),
    turnId,
  };
}

/**
 * The 2026-09-01 TeamRolesV1 run, in miniature: Keystone's first turn (546 s,
 * 64 tool calls, 31,994 out, 5,108,379 cache reads) and Bob's lane-A turn
 * (822 s, 88 calls, 56,580 out, 11,957,407 cache reads).
 */
function liveRunSeats() {
  return [
    {
      executionKey: "execution:keystone",
      seat: "Keystone · Lead",
      transcript: [
        contextWindow("k-ctx", "k1", 0, 216_000, 1_000_000),
        tool(
          "k-read",
          "k1",
          1,
          { path: "plans/SESSION_STATE.md" },
          "x".repeat(8),
        ),
        turnResult("k-result", "k1", 546, {
          durationMs: 546_000,
          usage: {
            toolCalls: 64,
            inputTokens: 4_200,
            outputTokens: 31_994,
            cacheReadTokens: 5_108_379,
            cacheWriteTokens: 135_012,
          },
        }),
      ],
    },
    {
      executionKey: "execution:bob",
      seat: "Bob · Builder",
      transcript: [
        tool("b-read", "b1", 300, { path: "crates/buzz-cli/src/lib.rs" }, "y"),
        turnResult("b-result", "b1", 1_122, {
          durationMs: 822_000,
          usage: {
            toolCalls: 88,
            inputTokens: 7_100,
            outputTokens: 56_580,
            cacheReadTokens: 11_957_407,
            cacheWriteTokens: 199_772,
            contextWindow: 1_000_000,
          },
          costUsd: 4.25,
        }),
      ],
    },
  ];
}

test("A3.5: per-turn rows carry the live run's own numbers", () => {
  const audit = deriveCodingSessionMissionAudit(liveRunSeats());
  assert.equal(audit.turns.length, 2);
  const [keystone, bob] = audit.turns;
  assert.equal(keystone.seat, "Keystone · Lead");
  assert.equal(keystone.durationMs, 546_000);
  assert.equal(keystone.toolCalls, 64);
  assert.equal(keystone.outputTokens, 31_994);
  assert.equal(keystone.cacheReadTokens, 5_108_379);
  assert.equal(keystone.cacheWriteTokens, 135_012);
  // No `usage.contextWindow` on this turn: the seat's own occupancy item is.
  assert.equal(keystone.contextWindow, 1_000_000);
  assert.equal(bob.toolCalls, 88);
  assert.equal(bob.outputTokens, 56_580);
  assert.equal(bob.cacheReadTokens, 11_957_407);
  assert.equal(bob.costUsd, 4.25);
});

test("A3.5: totals are per seat and per session, and cost stays absent when unreported", () => {
  const audit = deriveCodingSessionMissionAudit(liveRunSeats());
  assert.deepEqual(
    audit.totalsBySeat.map((entry) => entry.seat),
    ["Keystone · Lead", "Bob · Builder"],
  );
  assert.equal(audit.totalsBySeat[0].totals.outputTokens, 31_994);
  // Keystone's turn carried no cost: `null`, never `0`.
  assert.equal(audit.totalsBySeat[0].totals.costUsd, null);
  assert.equal(audit.sessionTotals.outputTokens, 88_574);
  assert.equal(audit.sessionTotals.cacheReadTokens, 17_065_786);
  assert.equal(audit.sessionTotals.toolCalls, 152);
  assert.equal(audit.sessionTotals.toolCallsReported, true);
  assert.equal(audit.sessionTotals.costUsd, 4.25);
  assert.equal(audit.seatCount, 2);
  assert.equal(audit.reportedSeatCount, 2);
});

test("A3.5: a driver that reported nothing yields null, never zero", () => {
  const audit = deriveCodingSessionMissionAudit([
    {
      executionKey: "execution:silent",
      seat: "Quiet · Builder",
      transcript: [
        tool("s-1", "t1", 0, { command: "cargo test" }, "ok"),
        turnResult("s-result", "t1", 10, { durationMs: 10_000 }),
      ],
    },
  ]);
  const [turn] = audit.turns;
  // A1 rule 1: the driver reported no count, so this is the published one.
  assert.equal(turn.toolCalls, 1);
  assert.equal(turn.toolCallsReported, false);
  assert.equal(turn.inputTokens, null);
  assert.equal(turn.outputTokens, null);
  assert.equal(turn.cacheReadTokens, null);
  assert.equal(turn.cacheWriteTokens, null);
  assert.equal(turn.contextWindow, null);
  assert.equal(turn.costUsd, null);
  assert.equal(audit.sessionTotals.outputTokens, null);
  assert.equal(audit.reportedSeatCount, 0);
});

test("A3.5: handed twice names what one seat asked for more than once", () => {
  const audit = deriveCodingSessionMissionAudit([
    {
      executionKey: "execution:keystone",
      seat: "Keystone · Lead",
      transcript: [
        tool("a", "t1", 0, { path: "SKILL.md" }, "a".repeat(4_000)),
        tool("b", "t1", 1, { path: "SKILL.md" }, "a".repeat(4_000)),
        tool("c", "t1", 2, { path: "read-once.md" }, "z"),
      ],
    },
  ]);
  assert.deepEqual(audit.handedTwice, [
    {
      seat: "Keystone · Lead",
      what: "path",
      key: "SKILL.md",
      count: 2,
      resultsSeen: 2,
      bytes: 8_000,
      bytesClipped: false,
    },
  ]);
});

test("A3.5: downloads the room counts the unbounded relay reads per subcommand", () => {
  const audit = deriveCodingSessionMissionAudit([
    {
      executionKey: "execution:bob",
      seat: "Bob · Builder",
      transcript: [
        tool(
          "a",
          "t1",
          0,
          { command: "bee sessions operation get --id 1" },
          "1",
        ),
        tool(
          "b",
          "t1",
          1,
          { command: "bee sessions operation get --id 2" },
          "2",
        ),
        tool("c", "t1", 2, { command: "bee sessions status" }, "3"),
        tool("d", "t1", 3, { command: "cargo build" }, "4"),
      ],
    },
  ]);
  // A1's own name for the read: `sessions <verb>`, four verbs, no object.
  assert.deepEqual(audit.roomDownloads, [
    { seat: "Bob · Builder", command: "sessions operation", count: 2 },
    { seat: "Bob · Builder", command: "sessions status", count: 1 },
  ]);
});

test("A3.5: a retry loop is the LONGEST run of identical command AND result", () => {
  const audit = deriveCodingSessionMissionAudit([
    {
      executionKey: "execution:keystone",
      seat: "Keystone · Lead",
      transcript: [
        // A run of three — the shape of the seat-repair loop.
        tool("a", "t1", 0, { command: "bee sessions seat-repair" }, "exit 5"),
        tool("b", "t1", 1, { command: "bee sessions seat-repair" }, "exit 5"),
        tool("c", "t1", 2, { command: "bee sessions seat-repair" }, "exit 5"),
        tool("d", "t1", 3, { command: "cargo test" }, "run 1"),
        // A second, shorter run of the same command: the count stays the
        // longest run, it is not summed to five.
        tool("e", "t1", 4, { command: "bee sessions seat-repair" }, "exit 5"),
        tool("f", "t1", 5, { command: "bee sessions seat-repair" }, "exit 5"),
      ],
    },
  ]);
  assert.deepEqual(audit.retryLoops, [
    {
      seat: "Keystone · Lead",
      command: "bee sessions seat-repair",
      count: 3,
      identicalResults: true,
    },
  ]);
});

test("A3.5: two in a row is repetition, not a loop, and changing results never are", () => {
  const audit = deriveCodingSessionMissionAudit([
    {
      executionKey: "execution:keystone",
      seat: "Keystone · Lead",
      transcript: [
        tool("a", "t1", 0, { command: "bee sessions seat-repair" }, "exit 5"),
        tool("b", "t1", 1, { command: "bee sessions seat-repair" }, "exit 5"),
        tool("c", "t1", 2, { command: "cargo test" }, "run 1"),
        tool("d", "t1", 3, { command: "cargo test" }, "run 2"),
        tool("e", "t1", 4, { command: "cargo test" }, "run 3"),
      ],
    },
  ]);
  assert.deepEqual(audit.retryLoops, []);
});

test("A3.5: clipped result bytes are a floor, and say so", () => {
  const audit = deriveCodingSessionMissionAudit([
    {
      executionKey: "execution:bob",
      seat: "Bob · Builder",
      transcript: [
        tool(
          "a",
          "t1",
          0,
          { path: "big.log" },
          `${"x".repeat(64)}…[elided 4096 bytes]`,
        ),
        tool("b", "t1", 1, { path: "big.log" }, "y".repeat(64)),
      ],
    },
  ]);
  assert.equal(audit.handedTwice.length, 1);
  assert.equal(audit.handedTwice[0].bytesClipped, true);
});

test("A3.5: one call can be handed twice under several keys", () => {
  const audit = deriveCodingSessionMissionAudit([
    {
      executionKey: "execution:bob",
      seat: "Bob · Builder",
      transcript: [
        {
          ...tool("a", "t1", 0, { command: "sed -i s/x/y/ f.ts" }, "ok"),
          editPaths: ["f.ts"],
        },
        {
          ...tool("b", "t1", 1, { command: "sed -i s/x/y/ f.ts" }, "ok"),
          editPaths: ["f.ts"],
        },
      ],
    },
  ]);
  assert.deepEqual(
    audit.handedTwice.map((row) => [row.what, row.key, row.count]),
    [
      ["path", "f.ts", 2],
      ["command", "sed -i s/x/y/ f.ts", 2],
    ],
  );
});

test("A3.5: every list is bounded and says what it left out", () => {
  const transcript = [];
  for (
    let index = 0;
    index < CODING_SESSION_AUDIT_LIMITS.rows + 3;
    index += 1
  ) {
    transcript.push(
      tool(`a-${index}`, `t${index}`, index, {
        path: `file-${index}.ts`,
      }),
    );
    transcript.push(
      tool(`b-${index}`, `t${index}`, index, {
        path: `file-${index}.ts`,
      }),
    );
  }
  const audit = deriveCodingSessionMissionAudit([
    { executionKey: "execution:x", seat: "X · Builder", transcript },
  ]);
  assert.equal(audit.handedTwice.length, CODING_SESSION_AUDIT_LIMITS.rows);
  const notice = audit.truncations.find(
    (entry) => entry.section === "handed-twice",
  );
  assert.equal(
    notice.notice,
    `Showing 50 of 53 repeated reads; 3 are not listed.`,
  );
});

test("A3.5: roomDownloadCommand names the subcommand and ignores everything else", () => {
  assert.equal(
    roomDownloadCommand("bee sessions operation get --id abc"),
    "sessions operation",
  );
  assert.equal(
    roomDownloadCommand("./target/release/bee sessions inbox"),
    "sessions inbox",
  );
  assert.equal(
    roomDownloadCommand("bee --format compact sessions status"),
    "sessions status",
  );
  assert.equal(roomDownloadCommand("bee messages list"), null);
  assert.equal(roomDownloadCommand("bee sessions audit"), null);
  assert.equal(roomDownloadCommand("cargo test -p buzz-cli"), null);
});

test("A3.5/F4: a cost the driver reported is reported, with no price-list gate", () => {
  // The gate this replaces looked for `usage.pricingIdentity`, which the wire
  // does not carry (`TurnUsageReport` is `deny_unknown_fields` over six token
  // fields) — so a real `cost_usd` printed as `not reported`.
  const audit = deriveCodingSessionMissionAudit([
    {
      executionKey: "execution:bob",
      seat: "Bob · Builder",
      transcript: [
        tool("a", "t1", 0, { command: "cargo test" }, "ok"),
        turnResult("r", "t1", 10, { durationMs: 10_000, costUsd: 4.21 }),
      ],
    },
  ]);
  assert.equal(audit.turns[0].costUsd, 4.21);
  assert.equal(audit.sessionTotals.costUsd, 4.21);
});

test("A3.5/F2: totals count how many of their turns reported usage", () => {
  const audit = deriveCodingSessionMissionAudit([
    {
      executionKey: "execution:bob",
      seat: "Bob · Builder",
      transcript: [
        turnResult("r1", "t1", 10, {
          durationMs: 1_000,
          usage: { outputTokens: 1_000 },
        }),
        turnResult("r2", "t2", 20, { durationMs: 1_000 }),
        turnResult("r3", "t3", 30, { durationMs: 1_000 }),
      ],
    },
  ]);
  // The seat "reported" under the old seat-granular rule; two of its three
  // turns did not, and the Σ is a third of the work.
  assert.equal(audit.reportedSeatCount, 1);
  assert.equal(audit.sessionTotals.turns, 3);
  assert.equal(audit.sessionTotals.reportedTurns, 1);
  assert.equal(audit.totalsBySeat[0].totals.reportedTurns, 1);
  assert.equal(audit.sessionTotals.outputTokens, 1_000);
});

test("Amendment 00:20: an unanswered repeat reports no bytes and says how many answers it saw", () => {
  const audit = deriveCodingSessionMissionAudit([
    {
      executionKey: "execution:bob",
      seat: "Bob · Builder",
      transcript: [
        tool("a", "t1", 0, { path: "open.ts" }, null),
        tool("b", "t1", 1, { path: "open.ts" }, null),
        tool("c", "t1", 2, { path: "half.ts" }, null),
        tool("d", "t1", 3, { path: "half.ts" }, "x".repeat(10)),
      ],
    },
  ]);
  const byKey = new Map(audit.handedTwice.map((row) => [row.key, row]));
  assert.equal(byKey.get("open.ts").resultsSeen, 0);
  assert.equal(byKey.get("open.ts").bytes, null);
  assert.equal(byKey.get("half.ts").resultsSeen, 1);
  assert.equal(byKey.get("half.ts").bytes, 10);
});

test("Amendment 00:20: a run whose results never arrived reports agreement as unknown", () => {
  const audit = deriveCodingSessionMissionAudit([
    {
      executionKey: "execution:keystone",
      seat: "Keystone · Lead",
      transcript: [
        tool("a", "t1", 0, { command: "bee sessions seat-repair" }, null),
        tool("b", "t1", 1, { command: "bee sessions seat-repair" }, null),
        tool("c", "t1", 2, { command: "bee sessions seat-repair" }, null),
      ],
    },
  ]);
  assert.deepEqual(audit.retryLoops, [
    {
      seat: "Keystone · Lead",
      command: "bee sessions seat-repair",
      count: 3,
      // Never `true`: unknown is not "they agreed".
      identicalResults: null,
    },
  ]);
});

test("Amendment 00:20: a cut-short transcript makes its counted tool calls a floor", () => {
  const seat = (transcriptTruncated) => ({
    executionKey: "execution:bob",
    seat: "Bob · Builder",
    transcriptTruncated,
    transcript: [
      tool("a", "t1", 0, { command: "cargo test" }, "ok"),
      turnResult("r", "t1", 10, { durationMs: 1_000 }),
    ],
  });
  assert.equal(
    deriveCodingSessionMissionAudit([seat(false)]).turns[0].toolCallsTruncated,
    false,
  );
  const cut = deriveCodingSessionMissionAudit([seat(true)]);
  assert.equal(cut.turns[0].toolCallsTruncated, true);
  assert.equal(cut.sessionTotals.toolCallsTruncated, true);
});

test("Amendment 00:20: the driver's own count is never a floor, cut short or not", () => {
  const audit = deriveCodingSessionMissionAudit([
    {
      executionKey: "execution:bob",
      seat: "Bob · Builder",
      transcriptTruncated: true,
      transcript: [
        tool("a", "t1", 0, { command: "cargo test" }, "ok"),
        turnResult("r", "t1", 10, {
          durationMs: 1_000,
          usage: { toolCalls: 88 },
        }),
      ],
    },
  ]);
  assert.equal(audit.turns[0].toolCalls, 88);
  assert.equal(audit.turns[0].toolCallsReported, true);
  assert.equal(audit.turns[0].toolCallsTruncated, false);
});
