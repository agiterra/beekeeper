import assert from "node:assert/strict";
import { test } from "node:test";

import {
  cadenceTagMs,
  collaboratorInputCoalesceMs,
  shellBroadcastCadenceLabel,
} from "@/features/builtin-shell/observe/shellBroadcastCadence";
import { WRITE_RESERVE } from "@/shared/api/relaySendBudget";

const frame = (tags) => ({
  id: "f".repeat(64),
  pubkey: "a".repeat(64),
  created_at: 1,
  kind: 24311,
  tags,
  content: "",
  sig: "0".repeat(128),
});

test("the status line states the quota honestly at baseline and while backing off", () => {
  const quota = "≤40/min — the relay's per-key quota";
  assert.equal(shellBroadcastCadenceLabel(null), `≤1 frame/s, ${quota}`);
  assert.equal(shellBroadcastCadenceLabel(1_000), `≤1 frame/s, ${quota}`);
  assert.equal(
    shellBroadcastCadenceLabel(2_000),
    `≤1 frame/2 s (backing off), ${quota}`,
  );
  assert.equal(
    shellBroadcastCadenceLabel(1_500, 40),
    `≤1 frame/1.5 s (backing off), ${quota}`,
  );
});

test("the cadence tag is read from a frame, and only when it is a positive integer", () => {
  assert.equal(cadenceTagMs(frame([["cadence", "1000"]])), 1_000);
  assert.equal(
    cadenceTagMs(
      frame([
        ["d", "s1"],
        ["cadence", "4000"],
      ]),
    ),
    4_000,
  );
  assert.equal(cadenceTagMs(frame([["cadence", "soon"]])), null);
  assert.equal(cadenceTagMs(frame([["cadence", "0"]])), null);
  assert.equal(cadenceTagMs(frame([])), null);
});

test("collaborator keystrokes coalesce for 80 ms once the write lane is below two reserves", () => {
  assert.equal(collaboratorInputCoalesceMs(WRITE_RESERVE * 2), 30);
  assert.equal(collaboratorInputCoalesceMs(WRITE_RESERVE * 2 - 1), 80);
  assert.equal(collaboratorInputCoalesceMs(0), 80);
});
