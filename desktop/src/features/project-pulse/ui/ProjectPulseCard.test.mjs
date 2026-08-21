import assert from "node:assert/strict";
import { test } from "node:test";

import { projectPulseCardReachabilityAbsence } from "@/features/project-pulse/ui/ProjectPulseCard";

test("a refreshing Pulse card never turns its last completed read into a current absence", () => {
  const copy = projectPulseCardReachabilityAbsence(true);
  assert.doesNotMatch(copy, /No sessions are currently verified live/);
  assert.match(copy, /last completed read.*refreshing/i);
});

test("a settled Pulse card may report the exact verified-live absence", () => {
  assert.equal(
    projectPulseCardReachabilityAbsence(false),
    "No sessions are currently verified live.",
  );
});
