/**
 * What a pending row is allowed to claim.
 *
 * These captions are the only thing standing between a person and a wrong
 * story about their own message: a turn the provider has parked behind an
 * hour of work must not read the same as one nobody ever picked up, and a
 * steer that was quietly downgraded to a boundary delivery must say so — the
 * whole point of asking to steer was to reach the turn that is running now.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { PENDING_CODING_SESSION_TURN_STALL_MS } from "../lib/codingSessionPendingTurns.ts";
import {
  describeCodingSessionTurnDegrade,
  describePendingCodingSessionTurn,
} from "./CodingSessionPendingTurns.tsx";

const STALLED = PENDING_CODING_SESSION_TURN_STALL_MS + 1;

test("an ordinary in-flight turn says nothing at all", () => {
  assert.equal(describePendingCodingSessionTurn("sending", 0), null);
  assert.equal(describePendingCodingSessionTurn("waiting", 0), null);
  // Asked for 2026-08-25: no spinner, no waiting-on-provider copy. A normal
  // in-flight turn is just the person's message; the session's own status
  // announces work beginning.
  const source = readFileSync(
    new URL("./CodingSessionPendingTurns.tsx", import.meta.url),
    "utf8",
  );
  assert.doesNotMatch(source, /LoaderCircle|animate-spin/);
  assert.doesNotMatch(source, /waiting for the provider/i);
});

test("nothing claims the model is thinking", () => {
  for (const state of [
    "sending",
    "queued",
    "waiting",
    "stalled",
    "degraded",
    "injected",
    "unknown",
  ]) {
    for (const age of [1_000, STALLED]) {
      const caption = describePendingCodingSessionTurn(state, age);
      if (caption === null) continue;
      assert.doesNotMatch(caption, /thinking|working|generating/i, caption);
    }
  }
});

test("a turn nobody answered says so, and only that", () => {
  assert.equal(
    describePendingCodingSessionTurn("stalled", STALLED),
    "Not picked up yet",
  );
});

test("a queued turn ages out loud instead of expiring", () => {
  assert.equal(
    describePendingCodingSessionTurn("queued", 1_000),
    "Queued by the provider; it cannot be recalled",
  );
  assert.equal(
    describePendingCodingSessionTurn("queued", 4 * 60_000),
    "Queued by the provider, not started yet — 4m; it cannot be recalled",
  );
});

test("a held row says out loud that it cannot be taken back", () => {
  // The client-side queue this slice retired had a Cancel button beside the
  // words "Queued by the provider". They now mean something else: the command
  // is signed, published, and the provider's. Saying so only in a `title`
  // leaves every touch user, and most screen-reader users, reading the old
  // sentence with none of the new meaning.
  for (const state of ["queued", "degraded"]) {
    for (const age of [1_000, 4 * 60_000]) {
      assert.match(
        describePendingCodingSessionTurn(state, age),
        /cannot be recalled/,
        `${state} at ${age}ms`,
      );
    }
  }
  const source = readFileSync(
    new URL("./CodingSessionPendingTurns.tsx", import.meta.url),
    "utf8",
  );
  assert.doesNotMatch(
    source,
    /title=\{[\s\S]*?cannot be recalled/,
    "the sentence belongs on screen, not only in a tooltip",
  );
});

test("a degraded steer names the downgrade rather than reading as a queue", () => {
  const unsupported = {
    code: "STEER_UNSUPPORTED",
    message: "this runtime offers no mid-turn steering",
  };
  assert.equal(
    describePendingCodingSessionTurn("degraded", 1_000, undefined, unsupported),
    "Not steered — this execution's runtime does not offer mid-turn steering; queued for the next turn boundary; it cannot be recalled",
  );
  assert.equal(
    describePendingCodingSessionTurn(
      "degraded",
      90_000,
      undefined,
      unsupported,
    ),
    "Not steered — this execution's runtime does not offer mid-turn steering; queued for the next turn boundary; not started yet — 1m; it cannot be recalled",
  );
});

// The two defects this replaced, pinned so neither can come back.
//
// A degraded turn has *not* been delivered — `turn_degraded` is published
// beside the `turn_queued` that follows it, and the turn is sitting in the
// provider's mailbox. And the reason is not always a missing capability: a
// turn that ended before the input reached it says nothing whatever about
// whether the runtime can steer.
test("a degrade never claims delivery, and never invents the reason", () => {
  const cases = [
    [
      { code: "STEER_TURN_ENDED", message: "the turn ended" },
      "Not steered — the turn ended before this reached it; queued for the next turn boundary; it cannot be recalled",
    ],
    [
      { code: "STEER_REJECTED", message: "the runtime said no" },
      "Not steered — the runtime refused the mid-turn delivery; queued for the next turn boundary; it cannot be recalled",
    ],
    [
      { code: "STEER_ATTACHMENTS_UNSUPPORTED", message: "images" },
      "Not steered — images cannot ride a mid-turn steer; queued for the next turn boundary; it cannot be recalled",
    ],
    [
      { code: "IMAGE_UNSUPPORTED", message: "no images here" },
      "Images were dropped — this execution's runtime does not accept them; queued for the next turn boundary; it cannot be recalled",
    ],
  ];
  for (const [degraded, expected] of cases) {
    const caption = describePendingCodingSessionTurn(
      "degraded",
      1_000,
      undefined,
      degraded,
    );
    assert.equal(caption, expected);
    assert.doesNotMatch(
      caption,
      /Delivered at/,
      `${degraded.code}: the turn is queued, not delivered`,
    );
  }
  // Only the code that actually means it may say the runtime cannot steer.
  for (const [degraded] of cases) {
    assert.doesNotMatch(
      describePendingCodingSessionTurn("degraded", 1_000, undefined, degraded),
      /does not offer mid-turn steering/,
      `${degraded.code} is not evidence about the runtime's capabilities`,
    );
  }
});

// A provider newer than this build can name a downgrade it has never heard
// of. Repeating what the provider said is the only honest option; inventing a
// reason is what the whole caption exists to stop.
test("an unrecognized degrade repeats the provider's own words", () => {
  assert.equal(
    describeCodingSessionTurnDegrade({
      code: "STEER_SOMETHING_NEW",
      message: "the adapter withdrew mid-write",
    }),
    "Not delivered as asked — the adapter withdrew mid-write; queued for the next turn boundary",
  );
  // No message to repeat: the code is better than a guess.
  assert.equal(
    describeCodingSessionTurnDegrade({
      code: "STEER_SOMETHING_NEW",
      message: "  ",
    }),
    "Not delivered as asked — STEER_SOMETHING_NEW; queued for the next turn boundary",
  );
  // Nothing at all — a row degraded by a receipt this client could not read.
  assert.equal(
    describeCodingSessionTurnDegrade(undefined),
    "Not delivered as asked; queued for the next turn boundary",
  );
});

test("an injected steer says it went into the running turn, and nothing about waiting", () => {
  for (const age of [1_000, 4 * 60_000]) {
    assert.equal(
      describePendingCodingSessionTurn("injected", age),
      "Injected into the running turn",
    );
  }
});

test("a delivery-unknown row leads with the provider's own words and offers dismissal", () => {
  assert.equal(
    describePendingCodingSessionTurn("unknown", 1_000, {
      code: "STEER_ACK_LOST",
      message: "the prompt ended before the acknowledgement arrived",
    }),
    "Delivery unknown — the prompt ended before the acknowledgement arrived",
  );
  // A blank message falls back to the code, and no detail at all to the one
  // honest sentence — never to "refused" or "dropped".
  assert.equal(
    describePendingCodingSessionTurn("unknown", 1_000, {
      code: "STEER_ACK_TIMEOUT",
      message: "  ",
    }),
    "Delivery unknown — STEER_ACK_TIMEOUT",
  );
  assert.match(
    describePendingCodingSessionTurn("unknown", 4 * 60_000),
    /^Delivery unknown — /,
  );
  const source = readFileSync(
    new URL("./CodingSessionPendingTurns.tsx", import.meta.url),
    "utf8",
  );
  assert.match(source, /coding-session-pending-turn-dismiss/);
  assert.match(source, /forgetPendingCodingSessionTurn\(/);
});
