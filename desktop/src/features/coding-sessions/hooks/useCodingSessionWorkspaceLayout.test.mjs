import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_AWAY_FROM_END_PX,
  CODING_SESSION_JUMP_PILL_GAP_PX,
  codingSessionAwayFromEnd,
  codingSessionJumpPillPosition,
} from "./useCodingSessionWorkspaceLayout.ts";

test("the jump-to-latest pill rides the measured dock at every height (SV-14)", () => {
  for (const dockHeight of [96, 180, 312.4, 520]) {
    const position = codingSessionJumpPillPosition({
      dockHeight,
      hasDock: true,
    });
    const bottom = Number.parseFloat(position.style.bottom);
    assert.ok(
      bottom >= dockHeight + CODING_SESSION_JUMP_PILL_GAP_PX - 0.5,
      `${dockHeight}: pill at ${bottom}px would sit on the composer`,
    );
    // No fixed class competes with the measured offset.
    assert.doesNotMatch(position.className, /\bbottom-/);
  }
});

test("before the first measurement the old fixed offset holds", () => {
  assert.deepEqual(
    codingSessionJumpPillPosition({ dockHeight: null, hasDock: true }),
    { className: "bottom-32", style: undefined },
  );
});

test("with no composer the pill clears the sandbox footer, not a ghost dock", () => {
  // The footer is one row: a 1.5rem chip plus 0.5rem padding top and bottom
  // (2.5rem). bottom-16 (4rem) leaves the pill clear of it.
  assert.deepEqual(
    codingSessionJumpPillPosition({ dockHeight: 240, hasDock: false }),
    { className: "bottom-16", style: undefined },
  );
});

test("the pill shows only once the reader has left the end by a few rows", () => {
  const at = (scrollTop) =>
    codingSessionAwayFromEnd({
      scrollHeight: 2000,
      clientHeight: 600,
      scrollTop,
    });
  assert.equal(at(1400), false, "at the end");
  assert.equal(
    at(1400 - CODING_SESSION_AWAY_FROM_END_PX),
    false,
    "at the threshold",
  );
  assert.equal(at(1400 - CODING_SESSION_AWAY_FROM_END_PX - 1), true, "past it");
  assert.equal(at(0), true, "at the top");
});
