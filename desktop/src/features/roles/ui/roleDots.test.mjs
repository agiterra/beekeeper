import assert from "node:assert/strict";
import test from "node:test";

import { rolesPageSummary } from "../lib/rolesPageSummary.ts";
import { roleActivity } from "./roleDots.tsx";

function summarize(seats) {
  return rolesPageSummary(
    { roles: [{ agents: [], seats }], byProject: [], unplaced: [] },
    { reported: [] },
  );
}

test("finished executions left in an open session do not imply open role activity", () => {
  const seats = ["completed", "stopped", "failed"].map((status) => ({
    key: status,
    status,
  }));
  assert.equal(summarize(seats).openSessions, 0);
  assert.equal(roleActivity(seats), "none");
});

test("activity and counts retain nonterminal records without inventing running", () => {
  for (const status of [
    "idle",
    "starting",
    "waiting_for_input",
    "disconnected",
    "unknown",
  ]) {
    const seats = [
      { key: "finished", status: "completed" },
      { key: "open", status },
    ];
    assert.equal(summarize(seats).openSessions, 1);
    assert.equal(roleActivity(seats), "idle");
  }
});

test("a running execution determines role activity even beside finished executions", () => {
  const seats = [
    { key: "finished", status: "stopped" },
    { key: "open", status: "running" },
  ];
  assert.equal(summarize(seats).openSessions, 1);
  assert.equal(roleActivity(seats), "running");
  assert.equal(roleActivity([]), "none");
});
