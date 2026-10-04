import assert from "node:assert/strict";
import test from "node:test";

import {
  deriveCodingSessionTranscriptModel,
  isCodingSessionContinuityLoss,
  leavesCodingSessionTranscript,
  stabilizeCodingSessionTranscriptModel,
} from "./codingSessionTranscriptModel.ts";
import { buildBaseTranscriptItem } from "./codingSessionTranscriptItems.ts";

// SV-16 (decision D3): continuity and boundary rows describe the session, not
// a turn. Routine ones leave the reading order; all of them stay in the model
// for Details and the composer's sandbox chip. Built through the real item
// builder, so the prose matched here is the prose the builder mints.

let seq = 0;
function status(status, reason, turnId) {
  seq += 1;
  const item = { kind: "status", status };
  if (reason !== undefined) item.reason = reason;
  return buildBaseTranscriptItem(item, {
    id: `status-${seq}`,
    sessionId: "session-1",
    targetKey: "target-1",
    channelId: "channel-1",
    timestamp: "2026-10-04T00:00:00.000Z",
    ...(turnId ? { turnId } : {}),
  });
}

function prompt(id, turnId) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role: "user",
    title: "Brian",
    text: "Go",
    timestamp: "2026-10-04T00:00:01.000Z",
    turnId,
  };
}

function shownIds(model) {
  return model.blocks.flatMap((block) =>
    block.kind === "standalone"
      ? [block.entry.kind === "item" ? block.entry.item.id : block.entry.id]
      : block.entries.map((entry) =>
          entry.kind === "item" ? entry.item.id : entry.id,
        ),
  );
}

test("routine continuity and every boundary row leave the transcript but stay in sessionFacts", () => {
  const fresh = status("session_fresh", "no_prior_execution");
  const rehydrated = status("session_rehydrated");
  const resumed = status("session_resumed");
  const loaded = status("session_loaded");
  const enforced = status("execution_boundary_enforced", "macos-seatbelt");
  const fullAccess = status("execution_boundary_not_enforced", "full-access");
  const isolation = status("operator_git_withheld");
  const inTurn = status(
    "execution_boundary_enforced",
    "macos-seatbelt",
    "turn-1",
  );
  const items = [
    fresh,
    rehydrated,
    resumed,
    loaded,
    enforced,
    fullAccess,
    isolation,
    prompt("prompt", "turn-1"),
    inTurn,
  ];
  for (const item of items.filter((item) => item.type === "lifecycle")) {
    assert.equal(leavesCodingSessionTranscript(item), true, item.text);
  }
  const model = deriveCodingSessionTranscriptModel(items, { isWorking: false });
  assert.deepEqual(shownIds(model), ["prompt"]);
  assert.deepEqual(
    model.sessionFacts.map((item) => item.id),
    [
      fresh,
      rehydrated,
      resumed,
      loaded,
      enforced,
      fullAccess,
      isolation,
      inTurn,
    ].map((item) => item.id),
  );
  // Nothing was moved into the diagnostics rail either: the facts left the
  // transcript whole.
  assert.equal(model.diagnostics.length, 0);
});

test("a continuity loss stays in the reading order and in sessionFacts", () => {
  const losses = [
    status("session_restarted_without_context", "relay_unavailable"),
    status("session_restarted_without_context"),
    status("session_fresh", "relay_query_failed"),
    status("session_fresh", "some_future_reason"),
  ];
  for (const item of losses) {
    assert.equal(isCodingSessionContinuityLoss(item), true, item.text);
    assert.equal(leavesCodingSessionTranscript(item), false, item.text);
  }
  const model = deriveCodingSessionTranscriptModel(losses, {
    isWorking: false,
  });
  assert.deepEqual(
    shownIds(model),
    losses.map((item) => item.id),
  );
  assert.equal(model.sessionFacts.length, losses.length);
});

test("an unknown status slug is untouched by the session-fact rule", () => {
  const other = status("something_new");
  assert.equal(other.title, "Status");
  assert.equal(leavesCodingSessionTranscript(other), false);
  const model = deriveCodingSessionTranscriptModel([other], {
    isWorking: false,
  });
  assert.equal(model.sessionFacts.length, 0);
});

test("stabilizing keeps sessionFacts and reuses an unchanged list", () => {
  const enforced = status("execution_boundary_enforced", "macos-seatbelt");
  const items = [enforced, prompt("prompt", "turn-1")];
  const previous = deriveCodingSessionTranscriptModel(items, {
    isWorking: false,
  });
  const same = stabilizeCodingSessionTranscriptModel(
    previous,
    deriveCodingSessionTranscriptModel(items, { isWorking: false }),
  );
  assert.equal(same, previous);

  const fresh = status("session_fresh", "no_prior_execution");
  const next = stabilizeCodingSessionTranscriptModel(
    previous,
    deriveCodingSessionTranscriptModel([...items, fresh], {
      isWorking: false,
    }),
  );
  assert.deepEqual(
    next.sessionFacts.map((item) => item.id),
    [enforced.id, fresh.id],
  );
});
