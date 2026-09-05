/**
 * The Agents directory's own sentences, asserted as values.
 *
 * The Add-agent affordance is here because replacing the old agents section
 * with the directory took its "Add" button with it, and a screen that lists
 * agents but offers no way to make one is a dead end. The button opens the
 * dialog that already existed; only the entry point is new.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  AGENT_DIRECTORY_ADD_LABEL,
  AGENT_DIRECTORY_ADD_TESTID,
  AGENT_DIRECTORY_EMPTY,
  AGENT_FILTERS_TESTID,
  OFFERS_ELIGIBILITY_FOOTNOTE,
  currentSeatText,
  launchesAsText,
} from "@/features/agents/ui/agentDirectoryCopy";

test("the directory's creation affordance is one button, named plainly", () => {
  assert.equal(AGENT_DIRECTORY_ADD_LABEL, "Add agent");
  assert.equal(AGENT_DIRECTORY_ADD_TESTID, "agent-directory-add");
});

test("the add label claims nothing about membership", () => {
  for (const word of ["Eligible", "Offered", "team", "crew", "available"]) {
    assert.equal(AGENT_DIRECTORY_ADD_LABEL.includes(word), false);
  }
});

test("the empty state still has a way out of itself", () => {
  // The toolbar is not rendered with zero rows, so the empty sentence and the
  // Add button are shown together; asserting the copy here keeps the pair
  // named in one place.
  assert.equal(AGENT_DIRECTORY_EMPTY, "No agents on this computer yet.");
  assert.equal(AGENT_FILTERS_TESTID, "agent-filters");
});

test("the surface's two evidenced facts are unchanged by this addition", () => {
  assert.equal(
    OFFERS_ELIGIBILITY_FOOTNOTE,
    "Offers and eligibility are not recorded yet.",
  );
  assert.equal(currentSeatText(null), "not seated");
  assert.equal(launchesAsText(null), "launches as — not set");
});
