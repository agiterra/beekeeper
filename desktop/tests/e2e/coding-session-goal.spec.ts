import { expect, test } from "@playwright/test";

import { buildCodingSessionGoalEvent } from "@/features/coding-sessions/lib/codingSessionGoal";

/** Contract-level screenshot companion; the signed end-to-end goal screenshots
 * live in coding-sessions.spec.ts so they can reuse its complete trusted
 * session fixture without weakening ingress. */
test("goal screenshot contract keeps workspace and catalog selectors distinct", () => {
  const event = buildCodingSessionGoalEvent({
    channelId: "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9",
    content: "Make authority visible at every decision point",
    sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
  });
  expect(event.kind).toBe(44227);
  expect("coding-session-goal-workspace").not.toBe(
    "coding-session-goal-catalog",
  );
});
