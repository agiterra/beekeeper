import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  decodePulseMissionRows,
  PULSE_MISSION_ROWS_SCHEMA,
} from "@/features/project-pulse/lib/pulseMissionWire";

const here = dirname(fileURLToPath(import.meta.url));

/**
 * The frozen contract. Read from disk rather than hand-built: a decoder proved
 * only against a fixture the test also wrote proves the test agrees with
 * itself, not that TypeScript agrees with Rust.
 */
function fixture() {
  return JSON.parse(
    readFileSync(resolve(here, "pulseMissionResponse.fixture.json"), "utf8"),
  );
}

/** The decode failure, or null when the payload was accepted. */
function rejection(mutate) {
  const payload = fixture();
  mutate(payload);
  try {
    decodePulseMissionRows(payload);
    return null;
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  }
}

test("the frozen fixture decodes and keeps every top-level key", () => {
  const decoded = decodePulseMissionRows(fixture());
  assert.equal(decoded.missionsSchema, PULSE_MISSION_ROWS_SCHEMA);
  assert.deepEqual(Object.keys(decoded).sort(), [
    "missionErrors",
    "missionScope",
    "missions",
    "missionsSchema",
    "openRulings",
    "overlaps",
    "rulingsWaitingOnViewer",
    "viewerPubkey",
  ]);
  assert.equal(decoded.missions.length, 3);
  assert.equal(decoded.missions[0].seats.length, 2);
  assert.equal(decoded.missions[0].moved.length, 2);
  assert.equal(decoded.overlaps.length, 1);
  assert.equal(decoded.missionErrors.length, 2);
  assert.equal(decoded.rulingsWaitingOnViewer.length, 1);
});

test("one extra top-level key is refused by name", () => {
  const message = rejection((payload) => {
    payload.missionSurprise = 1;
  });
  assert.match(String(message), /missionSurprise/);
});

test("one missing top-level key is refused by name", () => {
  const message = rejection((payload) => {
    delete payload.overlaps;
  });
  assert.match(String(message), /overlaps/);
});

test("an extra key on a mission row is refused by name", () => {
  const message = rejection((payload) => {
    payload.missions[0].extra = true;
  });
  assert.match(String(message), /mission/);
});

test("a missing key on a seat is refused by name", () => {
  const message = rejection((payload) => {
    delete payload.missions[0].seats[0].role;
  });
  assert.match(String(message), /seat/);
});

test("a required string that arrives null is refused, not coerced", () => {
  assert.match(
    String(rejection((p) => (p.missionScope = null))),
    /missionScope/,
  );
  assert.match(
    String(rejection((p) => (p.missions[0].channelId = null))),
    /channelId/,
  );
  assert.match(
    String(rejection((p) => (p.missions[0].lines[0].text = null))),
    /text/,
  );
  assert.match(
    String(rejection((p) => (p.missions[0].seats[0].pubkey = null))),
    /pubkey/,
  );
  assert.match(
    String(rejection((p) => (p.overlaps[0].pathsTruncated = null))),
    /pathsTruncated/,
  );
});

test("an unknown mission state is refused rather than passed through", () => {
  const message = rejection((payload) => {
    payload.missions[0].state = "sprinting";
  });
  assert.match(String(message), /sprinting/);
});

test("an unknown moved kind is refused rather than passed through", () => {
  const message = rejection((payload) => {
    payload.missions[0].moved[0].kind = "rebase";
  });
  assert.match(String(message), /rebase/);
});

test("an unknown line id is refused in every line family", () => {
  assert.match(
    String(rejection((p) => (p.missions[0].lines[0].id = "vibes"))),
    /vibes/,
  );
  assert.match(
    String(rejection((p) => (p.missions[0].seats[0].lines[0].id = "vibes"))),
    /vibes/,
  );
  assert.match(
    String(rejection((p) => (p.missions[0].moved[0].lines[0].id = "gate"))),
    /gate/,
  );
  assert.match(
    String(rejection((p) => (p.missions[0].timing[0].id = "moved"))),
    /moved/,
  );
  assert.match(
    String(rejection((p) => (p.overlaps[0].lines[0].id = "timing"))),
    /timing/,
  );
});

test("a seat line id is not accepted on a mission line and vice versa", () => {
  // The families are closed independently: `gate` is a seat line, never a
  // mission line, so a producer that flattened them would be caught here
  // rather than rendering a gate row where the mission's own state belongs.
  assert.match(
    String(rejection((p) => (p.missions[0].lines[1].id = "gate"))),
    /gate/,
  );
  assert.match(
    String(rejection((p) => (p.missions[0].seats[0].lines[0].id = "policy"))),
    /policy/,
  );
});

test("the wrong schema string is refused", () => {
  const message = rejection((payload) => {
    payload.missionsSchema = "buzz-pulse-mission-rows/v2";
  });
  assert.match(String(message), /missionsSchema/);
});

test("a viewer pubkey may be null but never a non-hex string", () => {
  const payload = fixture();
  payload.viewerPubkey = null;
  assert.equal(decodePulseMissionRows(payload).viewerPubkey, null);
  assert.match(
    String(rejection((p) => (p.viewerPubkey = "me"))),
    /viewerPubkey/,
  );
});

test("a ruling may be held on the founder or on a pubkey, and nothing else", () => {
  const payload = fixture();
  payload.openRulings[0].heldOn = "founder";
  assert.equal(
    decodePulseMissionRows(payload).openRulings[0].heldOn,
    "founder",
  );
  assert.match(
    String(rejection((p) => (p.openRulings[0].heldOn = "the boss"))),
    /heldOn/,
  );
});

test("an age or observation time must be a whole number of seconds or null", () => {
  assert.match(
    String(rejection((p) => (p.missions[0].latestObservationAt = 1.5))),
    /latestObservationAt/,
  );
  assert.match(
    String(rejection((p) => (p.missions[0].moved[0].ageSeconds = "6m"))),
    /ageSeconds/,
  );
  const payload = fixture();
  payload.missions[0].moved[0].ageSeconds = null;
  assert.equal(
    decodePulseMissionRows(payload).missions[0].moved[0].ageSeconds,
    null,
  );
});

test("a decoded response is frozen so no renderer can edit the wire", () => {
  const decoded = decodePulseMissionRows(fixture());
  assert.equal(Object.isFrozen(decoded), true);
  assert.equal(Object.isFrozen(decoded.missions), true);
  assert.equal(Object.isFrozen(decoded.missions[0].lines[0]), true);
});

test("every actor pubkey in the frozen contract is canonical 64-hex", () => {
  // Sub-lane T found this contract carrying two 66-character actor values and
  // pinned the defect rather than absorbing it; the lane owner corrected the
  // generator. This is the pin the other way round: a producer that widens an
  // actor pubkey again fails here, at the field, instead of silently teaching
  // this decoder a wider shape.
  const payload = fixture();
  const actors = [
    payload.viewerPubkey,
    payload.missions[0].channelId,
    payload.missions[0].seats[0].pubkey,
    payload.missions[0].seats[1].pubkey,
    payload.missions[0].moved[0].authorPubkey,
    payload.missions[0].moved[1].authorPubkey,
    payload.openRulings[0].askedBy,
    payload.openRulings[1].heldOn,
    payload.overlaps[0].seats[0].authorPubkey,
    payload.overlaps[0].seats[1].authorPubkey,
  ];
  for (const actor of actors) {
    assert.equal(actor.length, 64, actor);
    assert.match(actor, /^[0-9a-f]{64}$/);
  }
  assert.match(
    String(
      rejection(
        (p) =>
          (p.missions[0].seats[0].pubkey = `${p.missions[0].seats[0].pubkey}ff`),
      ),
    ),
    /pubkey/,
  );
});
