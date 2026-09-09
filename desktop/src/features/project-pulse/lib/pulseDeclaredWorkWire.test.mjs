import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  decodePulseDeclaredWork,
  PULSE_DECLARED_WORK_SCHEMA,
} from "@/features/project-pulse/lib/pulseDeclaredWorkWire";

const here = dirname(fileURLToPath(import.meta.url));

/**
 * The frozen contract, written by `crates/buzz-core/src/pulse_declared_work_tests.rs`
 * from the real projection over real signed events. Read from disk rather than
 * hand-built: a decoder proved only against a fixture the test also wrote
 * proves the test agrees with itself, not that TypeScript agrees with Rust.
 */
function fixture() {
  return JSON.parse(
    readFileSync(resolve(here, "pulseDeclaredWork.fixture.json"), "utf8"),
  );
}

/** The decode failure, or null when the payload was accepted. */
function rejection(mutate) {
  const payload = fixture();
  mutate(payload);
  try {
    decodePulseDeclaredWork(payload);
    return null;
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  }
}

test("the frozen fixture decodes and keeps every top-level key", () => {
  const decoded = decodePulseDeclaredWork(fixture());
  assert.equal(decoded.schema, PULSE_DECLARED_WORK_SCHEMA);
  assert.deepEqual(Object.keys(decoded).sort(), [
    "errors",
    "schema",
    "sessions",
    "viewerPubkey",
  ]);
  assert.equal(decoded.sessions.length, 2);
  assert.equal(decoded.errors.length, 1);
});

test("the open session carries a report and a changes-requested disposition", () => {
  const [open] = decodePulseDeclaredWork(fixture()).sessions;
  assert.equal(open.lifecycle, "open");
  assert.equal(open.terminal, null);
  assert.equal(open.unreadable, null);
  assert.equal(open.excludedCount, 0);
  assert.equal(open.assignments.length, 1);
  const [assignment] = open.assignments;
  // A report is evidence of a report: the word is `reported`, never `settled`.
  assert.equal(assignment.status, "reported");
  assert.equal(assignment.settlement.settled, false);
  assert.equal(assignment.reports.length, 1);
  assert.equal(assignment.reports[0].testCount, 2);
  assert.equal(assignment.dispositions.length, 1);
  assert.equal(assignment.dispositions[0].decision, "changes-requested");
  // The assigner stays inspectable, and is not the responsible participant.
  assert.notEqual(assignment.assignerPubkey, assignment.assigneeActor);
  assert.equal(assignment.assigneeRole, "builder");
  assert.equal(assignment.fileOwnership.length, 2);
});

test("the closed session is settled and carries its terminal", () => {
  const [, closed] = decodePulseDeclaredWork(fixture()).sessions;
  assert.equal(closed.lifecycle, "closed");
  assert.equal(closed.name, null);
  assert.equal(closed.terminal.type, "mission.completed");
  assert.match(closed.terminal.eventId, /^[0-9a-f]{64}$/);
  const [assignment] = closed.assignments;
  assert.equal(assignment.status, "settled");
  assert.equal(assignment.settlement.settled, true);
  for (const key of [
    "governedReportEventId",
    "dispositionEventId",
    "acknowledgementEventId",
  ]) {
    assert.match(assignment.settlement[key], /^[0-9a-f]{64}$/, key);
  }
});

test("one extra top-level key is refused by name", () => {
  const message = rejection((payload) => {
    payload.declaredSurprise = 1;
  });
  assert.match(String(message), /declaredSurprise/);
});

test("an extra key on an assignment is refused by name", () => {
  const message = rejection((payload) => {
    payload.sessions[0].assignments[0].extra = true;
  });
  assert.match(String(message), /extra/);
});

test("a missing top-level key is refused by name", () => {
  const message = rejection((payload) => {
    delete payload.errors;
  });
  assert.match(String(message), /errors/);
});

test("a different response schema is refused rather than upgraded", () => {
  const message = rejection((payload) => {
    payload.schema = "buzz-pulse-declared-work/v2";
  });
  assert.match(String(message), /buzz-pulse-declared-work\/v2/);
});

test("an unknown disposition token is refused rather than rendered with a shrug", () => {
  const message = rejection((payload) => {
    payload.sessions[0].assignments[0].dispositions[0].decision = "vibing";
  });
  assert.match(String(message), /vibing/);
  assert.match(String(message), /decision/);
});

test("an unknown status token is refused", () => {
  const message = rejection((payload) => {
    payload.sessions[0].assignments[0].status = "done";
  });
  assert.match(String(message), /status/);
});

test("an unknown lifecycle token is refused", () => {
  const message = rejection((payload) => {
    payload.sessions[0].lifecycle = "paused";
  });
  assert.match(String(message), /lifecycle/);
});

test("an unknown terminal token is refused", () => {
  const message = rejection((payload) => {
    payload.sessions[1].terminal.type = "mission.abandoned";
  });
  assert.match(String(message), /mission.abandoned/);
});

test("a non-hex genesis ref is refused at the field", () => {
  const message = rejection((payload) => {
    payload.sessions[0].genesisRef = "0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0";
  });
  assert.match(String(message), /genesisRef/);
});

test("the two umbrellas carry different session and genesis ids", () => {
  // The details block shows both (§6): the umbrella's coordination key and
  // the signed record its 44244 set names are different facts, and a reader
  // checking that records belong to the row needs the second one.
  const [open, closed] = decodePulseDeclaredWork(fixture()).sessions;
  assert.notEqual(open.genesisRef, open.sessionRef);
  assert.notEqual(open.genesisRef, closed.genesisRef);
  assert.match(open.genesisRef, /^[0-9a-f]{64}$/);
  assert.match(closed.genesisRef, /^[0-9a-f]{64}$/);
});

test("a non-hex source event id is refused at the field", () => {
  const message = rejection((payload) => {
    payload.sessions[0].assignments[0].sourceEventId = "not-an-event-id";
  });
  assert.match(String(message), /sourceEventId/);
});

test("a widened actor pubkey is refused rather than becoming the new normal", () => {
  const message = rejection((payload) => {
    const assignment = payload.sessions[0].assignments[0];
    assignment.assigneeActor = `${assignment.assigneeActor}ff`;
  });
  assert.match(String(message), /assigneeActor/);
});

test("a fractional timestamp is refused", () => {
  const message = rejection((payload) => {
    payload.sessions[0].assignments[0].createdAt = 1756790160.5;
  });
  assert.match(String(message), /createdAt/);
});

test("an excluded count that is not a count is refused", () => {
  const message = rejection((payload) => {
    payload.sessions[0].excludedCount = -1;
  });
  assert.match(String(message), /excludedCount/);
});

test("an unreadable session decodes, with its sentence and no assignments", () => {
  const payload = fixture();
  payload.sessions[0].unreadable =
    "duplicate supplied team transaction 4c5d6e7f";
  payload.sessions[0].assignments = [];
  const decoded = decodePulseDeclaredWork(payload);
  assert.equal(
    decoded.sessions[0].unreadable,
    "duplicate supplied team transaction 4c5d6e7f",
  );
  assert.deepEqual(decoded.sessions[0].assignments, []);
});

test("a decoded response is frozen so no renderer can edit the wire", () => {
  const decoded = decodePulseDeclaredWork(fixture());
  assert.equal(Object.isFrozen(decoded), true);
  assert.equal(Object.isFrozen(decoded.sessions), true);
  assert.equal(Object.isFrozen(decoded.sessions[0].assignments[0]), true);
  assert.equal(
    Object.isFrozen(decoded.sessions[0].assignments[0].settlement),
    true,
  );
});

test("every id in the frozen contract is canonical", () => {
  const payload = fixture();
  assert.match(payload.viewerPubkey, /^[0-9a-f]{64}$/);
  for (const session of payload.sessions) {
    assert.match(
      session.channelId,
      /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/,
    );
    assert.match(
      session.sessionRef,
      /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/,
    );
    assert.match(session.genesisRef, /^[0-9a-f]{64}$/);
    assert.match(session.founderPubkey, /^[0-9a-f]{64}$/);
    for (const assignment of session.assignments) {
      assert.match(assignment.sourceEventId, /^[0-9a-f]{64}$/);
      assert.match(assignment.assignerPubkey, /^[0-9a-f]{64}$/);
      assert.match(assignment.assigneeActor, /^[0-9a-f]{64}$/);
      for (const report of assignment.reports) {
        assert.match(report.eventId, /^[0-9a-f]{64}$/);
        assert.match(report.authorPubkey, /^[0-9a-f]{64}$/);
      }
      for (const disposition of assignment.dispositions) {
        assert.match(disposition.eventId, /^[0-9a-f]{64}$/);
        assert.match(disposition.reportRef, /^[0-9a-f]{64}$/);
      }
    }
  }
});
