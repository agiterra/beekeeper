import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionDeviceOpenCount,
  DEVICE_NOT_OFFERED_REASON,
  deriveCodingSessionDeviceView,
  foldCodingSessionDevice,
} from "./codingSessionDevice.ts";
import {
  availability,
  CHANNEL,
  command,
  eventId,
  OTHER,
  PROVIDER,
  record,
  SEAT,
  SLOT,
  snapshot,
  state,
  TARGET_KEY,
} from "./surfaceFixtures.testFixtures.mjs";

const NOW = 1_791_374_600_000;
const MACHINE = "Brian's Mac";

function fold(events) {
  return foldCodingSessionDevice(events, {
    channelId: CHANNEL,
    authorityFor: (key) => (key === TARGET_KEY ? PROVIDER : null),
  });
}

function view(events, observer = null, extra = {}) {
  return deriveCodingSessionDeviceView({
    fold: fold(events),
    targetKey: TARGET_KEY,
    machine: MACHINE,
    observer,
    now: NOW,
    nameOf: (pubkey) => (pubkey === SEAT ? "Reviewer" : null),
    ...extra,
  });
}

const live = (frameAt, cadenceMs = 3000) => ({
  status: "live",
  frameAt,
  cadenceMs,
  actor: null,
});

test("no availability record: not offered by the machine, with the exact sentence, never 'no devices'", () => {
  const result = view([]);
  assert.equal(result.status, "not-offered");
  assert.equal(result.statusText, `Not offered by ${MACHINE}`);
  assert.equal(
    result.detail,
    "This machine's provider does not offer devices.",
  );
  assert.equal(result.detail, DEVICE_NOT_OFFERED_REASON);
  for (const text of [result.statusText, result.detail, result.header]) {
    assert.doesNotMatch(text, /no devices/i);
  }
  // A command nobody answers on such a machine is still "not offered".
  const unanswered = view([
    command("open", "cmd-1", { created_at: NOW / 1000 - 600 }),
  ]);
  assert.equal(unanswered.status, "not-offered");
});

test("iOS unavailable: the provider's reason verbatim", () => {
  const result = view([
    availability({ available: false, reason: "Xcode not found" }),
  ]);
  assert.equal(result.status, "unavailable");
  assert.equal(result.statusText, `Not offered by ${MACHINE}: Xcode not found`);
});

test("agent-device not installed: the note, and snapshots still work", () => {
  const result = view([
    availability(
      { available: true },
      { installed: false, reason: "agent-device 0.21 not on PATH" },
    ),
  ]);
  assert.equal(
    result.agentDeviceNote,
    "agent-device unavailable: agent-device 0.21 not on PATH. Snapshots still work.",
  );
  assert.equal(view([availability()]).agentDeviceNote, null);
});

test("available, no slot: No device open, with the hint", () => {
  const result = view([availability()]);
  assert.equal(result.status, "no-device");
  assert.equal(result.statusText, "No device open");
  assert.equal(result.detail, "The agent opens one with bee device open.");
  assert.equal(result.header, `Device · on ${MACHINE}`);
});

test("an open slot: header, Live · every 3 s, then Stalled with the age, never Live", () => {
  const events = [availability(), state("open", { cmd: "cmd-1" })];
  const result = view(events, live(NOW - 2_000));
  assert.equal(result.header, `iPhone 17 · iOS 27.0 · on ${MACHINE}`);
  assert.equal(result.status, "live");
  assert.equal(result.statusText, "Live · every 3 s");
  assert.deepEqual(result.watch, { slot: SLOT, producerPubkey: PROVIDER });
  const stalled = view(events, {
    status: "stalled",
    frameAt: NOW - 40_000,
    cadenceMs: 3000,
    actor: null,
  });
  assert.equal(stalled.status, "stalled");
  assert.equal(stalled.statusText, "Stalled · last frame 40 s ago");
  assert.doesNotMatch(stalled.statusText, /Live/);
  assert.equal(codingSessionDeviceOpenCount(fold(events)), 1);
});

test("no stream: the newest snapshot, by the person who asked", () => {
  const takenAt = new Date(2026, 9, 7, 14, 2).getTime();
  const result = view(
    [availability(), state("open"), snapshot({ requestedBy: SEAT, takenAt })],
    { status: "not-streaming", frameAt: null, cadenceMs: null, actor: null },
  );
  assert.equal(result.status, "snapshot");
  assert.equal(result.statusText, "Snapshot · 14:02 by Reviewer");
  assert.equal(result.snapshots.length, 1);
});

test("booting, failed and closed read as such; an unanswered command reads No answer with its age", () => {
  assert.equal(view([availability(), state("booting")]).statusText, "Booting…");
  assert.equal(
    view([availability(), state("failed", {}, { reason: "Simulator lost" })])
      .statusText,
    "Failed: Simulator lost",
  );
  const closed = view([availability(), state("closed")]);
  assert.equal(closed.statusText, "Closed");
  assert.equal(
    codingSessionDeviceOpenCount(fold([availability(), state("closed")])),
    0,
  );
  const waiting = view([
    availability(),
    command("open", "cmd-9", { created_at: NOW / 1000 - 120 }),
  ]);
  assert.equal(waiting.status, "waiting");
  assert.equal(waiting.statusText, `No answer from ${MACHINE} · 2 min`);
  // Answered by a terminal record: no longer waiting.
  const answered = view([
    availability(),
    command("open", "cmd-9", { created_at: NOW / 1000 - 120 }),
    state("open", { cmd: "cmd-9", created_at: NOW / 1000 - 100 }),
  ]);
  assert.equal(answered.status, "connecting");
  // A refusal answers it too.
  const refused = view([
    availability(),
    command("open", "cmd-9", { created_at: NOW / 1000 - 120 }),
    record(
      "refused",
      {
        code: "no_standing",
        reason: "Only a seat of this session may drive the device",
      },
      { cmd: "cmd-9" },
    ),
  ]);
  assert.equal(refused.status, "no-device");
});

test("records from a signer that does not run the generation are dropped", () => {
  const forged = [
    availability({ available: true }, undefined, { pubkey: OTHER }),
    state("open", { pubkey: OTHER }),
  ];
  assert.equal(view(forged).status, "not-offered");
  assert.equal(codingSessionDeviceOpenCount(fold(forged)), 0);
  // A snapshot signed by anyone but the slot's authority is not shown.
  const result = view([
    availability(),
    state("open"),
    snapshot({ signer: OTHER }),
  ]);
  assert.equal(result.snapshots.length, 0);
});

test("strict tag order: a reordered or extra tag drops the record", () => {
  const reordered = availability();
  [reordered.tags[2], reordered.tags[3]] = [
    reordered.tags[3],
    reordered.tags[2],
  ];
  assert.equal(view([reordered]).status, "not-offered");
  const extra = availability();
  extra.tags.push(["x", "y"]);
  assert.equal(view([extra]).status, "not-offered");
  const unknownKey = record("availability", {
    platforms: { ios: { available: true }, android: { available: true } },
    agentDevice: { installed: true },
    udid: "nope",
  });
  assert.equal(view([unknownKey]).status, "not-offered");
  // A host-local value anywhere in the content drops the record (§ 2).
  const pathy = availability({
    available: false,
    reason: "see /Users/brian/Library",
  });
  assert.equal(view([pathy]).status, "not-offered");
});

test("the newest state wins per slot; ties go to the lower id", () => {
  const older = state("open", { created_at: 100, id: eventId(9001) });
  const newer = state("closed", { created_at: 200, id: eventId(9002) });
  assert.equal(fold([newer, older]).slots.get(SLOT).state, "closed");
  const tieA = state("open", { created_at: 300, id: eventId(1) });
  const tieB = state("failed", { created_at: 300, id: eventId(2) });
  assert.equal(fold([tieB, tieA]).slots.get(SLOT).state, "open");
});
