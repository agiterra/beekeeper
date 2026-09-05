import assert from "node:assert/strict";
import { test } from "node:test";

import {
  CONTRIBUTORS_EMPTY,
  CONTRIBUTORS_FOOTNOTE,
  CONTRIBUTORS_LANDINGS_NOTE,
  CONTRIBUTORS_LOADING,
  CONTRIBUTORS_PROJECT_MISSING,
  CONTRIBUTORS_SUBTITLE,
  CONTRIBUTORS_TITLE,
  CONTRIBUTOR_ROLES_EMPTY,
  PAST_CONTRIBUTOR,
  contributorLastSeenText,
  contributorNameText,
  contributorRolesText,
  contributorSeatsText,
  contributorStateAttr,
  contributorsNoticeSentence,
} from "@/features/contributors/ui/contributorsCopy";
import { truncatePubkey } from "@/shared/lib/pubkey";

test("every §C string is the exact copy the design specifies", () => {
  assert.equal(CONTRIBUTORS_TITLE, "Contributors");
  assert.equal(
    CONTRIBUTORS_SUBTITLE,
    "Seat history for this project — who has held a seat here, and when it was last observed. This is not a list of who is available.",
  );
  assert.equal(
    CONTRIBUTORS_PROJECT_MISSING,
    "This project is not readable here.",
  );
  assert.equal(CONTRIBUTORS_LOADING, "Reading seat history…");
  assert.equal(CONTRIBUTORS_EMPTY, "No agent has held a seat on this project.");
  assert.equal(CONTRIBUTOR_ROLES_EMPTY, "no role");
  assert.equal(PAST_CONTRIBUTOR, "Past contributor");
  assert.equal(
    CONTRIBUTORS_LANDINGS_NOTE,
    "Landings are not counted here yet: they are recorded per session channel, and this list does not subscribe to them.",
  );
  assert.equal(
    CONTRIBUTORS_FOOTNOTE,
    "Offers and eligibility are not recorded yet.",
  );
});

test("contributorsNoticeSentence joins message and detail with an em dash, or omits it", () => {
  assert.equal(
    contributorsNoticeSentence("Seats are partial", "channel unreadable"),
    "Seats are partial — channel unreadable",
  );
  assert.equal(
    contributorsNoticeSentence("Seats are partial", null),
    "Seats are partial",
  );
});

test("contributorSeatsText pluralizes", () => {
  assert.equal(contributorSeatsText(1), "1 seat");
  assert.equal(contributorSeatsText(2), "2 seats");
  assert.equal(contributorSeatsText(0), "0 seats");
});

test("contributorRolesText joins with a middle dot, or discloses no role", () => {
  assert.equal(contributorRolesText(["lead", "verifier"]), "lead · verifier");
  assert.equal(contributorRolesText([]), "no role");
});

test("contributorLastSeenText renders only the two evidenced states", () => {
  assert.equal(
    contributorLastSeenText({
      isSeatedNow: true,
      lastStatus: "running",
      lastAgeSeconds: 300,
    }),
    "Seated · running · 5m",
  );
  assert.equal(
    contributorLastSeenText({
      isSeatedNow: false,
      lastStatus: "completed",
      lastAgeSeconds: 90,
    }),
    "Past contributor",
  );
});

test("contributorStateAttr is seated or past, nothing else", () => {
  assert.equal(contributorStateAttr({ isSeatedNow: true }), "seated");
  assert.equal(contributorStateAttr({ isSeatedNow: false }), "past");
});

test("contributorNameText falls back to the canonical short pubkey", () => {
  assert.equal(
    contributorNameText({ name: "Ada", agentPubkey: "1".repeat(64) }),
    "Ada",
  );
  // §C says "the pubkey's first 8 chars"; the repo's pubkey-truncation guard
  // says every display truncation goes through `truncatePubkey`, because a
  // bare prefix is forgeable by grinding and a reader who has learned one
  // shape must not meet a second. The guard wins, and this is its shape:
  // eight, an ellipsis, then the last four.
  assert.equal(
    contributorNameText({ name: null, agentPubkey: "abcdef01".repeat(8) }),
    truncatePubkey("abcdef01".repeat(8)),
  );
  assert.equal(
    contributorNameText({ name: null, agentPubkey: "abcdef01".repeat(8) }),
    "abcdef01…ef01",
  );
});
