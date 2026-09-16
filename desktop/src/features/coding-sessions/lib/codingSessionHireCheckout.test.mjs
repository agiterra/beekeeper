/**
 * The rule that decides which repository a hired seat works in.
 *
 * Live evidence for every case here: on 2026-09-16 two Tank Loop seats were
 * cut from `/Users/brian/Projects/beekeeper/beekeeper` because the host asked
 * `byChannel[channel] ?? mru[0]` and the project's own recorded checkout was
 * never consulted (ledger 135(a)).
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  HIRE_CHECKOUT_NOT_RECORDED,
  codingSessionHireCheckoutLine,
  resolveCodingSessionHireCheckout,
} from "./codingSessionHireCheckout.ts";

const CHANNEL = "6620be79-1b2c-4f8a-9c3d-2e4f6a8b0c1d";
const PROJECT = `30621:${"3d3b7169".padEnd(64, "0")}:tank-loop`;
const TANK_LOOP = "/Users/brian/Projects/tankloop";
const BEEKEEPER = "/Users/brian/Projects/beekeeper/beekeeper";

test("the project's own recorded checkout answers first", () => {
  const resolved = resolveCodingSessionHireCheckout({
    projectRef: PROJECT,
    channelId: CHANNEL,
    byProject: { [PROJECT]: { path: TANK_LOOP } },
    byChannel: { [CHANNEL]: { path: BEEKEEPER } },
  });
  assert.equal(resolved.kind, "resolved");
  assert.equal(resolved.path, TANK_LOOP);
  assert.equal(resolved.source, "project");
  // The folder that used to win is named, not silently dropped.
  assert.match(resolved.passedOver, /Beekeeper|beekeeper/);
  assert.match(resolved.passedOver, /was not used/);
});

test("a channel folder inside the project's checkout is nothing to report", () => {
  const resolved = resolveCodingSessionHireCheckout({
    projectRef: PROJECT,
    channelId: CHANNEL,
    byProject: { [PROJECT]: { path: TANK_LOOP } },
    byChannel: { [CHANNEL]: { path: `${TANK_LOOP}/crates` } },
  });
  assert.equal(resolved.path, TANK_LOOP);
  assert.equal(resolved.passedOver, null);
});

test("a trailing separator does not make the same folder a different one", () => {
  const resolved = resolveCodingSessionHireCheckout({
    projectRef: PROJECT,
    channelId: CHANNEL,
    byProject: { [PROJECT]: { path: `${TANK_LOOP}/` } },
    byChannel: { [CHANNEL]: { path: TANK_LOOP } },
  });
  assert.equal(resolved.passedOver, null);
});

test("a projectless session still uses the folder it remembers", () => {
  const resolved = resolveCodingSessionHireCheckout({
    projectRef: null,
    channelId: CHANNEL,
    byProject: {},
    byChannel: { [CHANNEL]: { path: BEEKEEPER } },
  });
  assert.equal(resolved.kind, "resolved");
  assert.equal(resolved.path, BEEKEEPER);
  assert.equal(resolved.source, "channel");
});

test("a project with no recorded checkout is refused, and names the control", () => {
  const resolved = resolveCodingSessionHireCheckout({
    projectRef: PROJECT,
    projectLabel: "Tank Loop",
    channelId: CHANNEL,
    byProject: {},
    byChannel: { [CHANNEL]: { path: BEEKEEPER } },
  });
  assert.equal(resolved.kind, "unrecorded");
  assert.equal(resolved.code, HIRE_CHECKOUT_NOT_RECORDED);
  assert.match(
    resolved.reason,
    /Set the repository folder for Tank Loop in Project settings → This computer → Repository folder/,
  );
  // The channel's folder is named as passed over, never taken.
  assert.match(resolved.reason, /was not used/);
});

test("with no label the refusal still names the coordinate, never nothing", () => {
  const resolved = resolveCodingSessionHireCheckout({
    projectRef: PROJECT,
    channelId: CHANNEL,
    byProject: {},
    byChannel: {},
  });
  assert.equal(resolved.kind, "unrecorded");
  assert.ok(resolved.reason.includes(`project ${PROJECT}`));
});

test("a projectless session with nothing recorded is refused, not seated", () => {
  const resolved = resolveCodingSessionHireCheckout({
    projectRef: null,
    channelId: CHANNEL,
    byProject: {},
    byChannel: {},
  });
  assert.equal(resolved.kind, "unrecorded");
  assert.match(resolved.reason, /Folder control/);
});

// The whole point: `mru` is not even a parameter, so the fallback that cut
// the wrong repository cannot be reached from here.
test("the resolver has no most-recently-used input to fall back to", () => {
  const resolved = resolveCodingSessionHireCheckout({
    projectRef: PROJECT,
    channelId: CHANNEL,
    byProject: {},
    byChannel: {},
    // Ignored: not part of the contract.
    mru: [{ path: BEEKEEPER }],
  });
  assert.equal(resolved.kind, "unrecorded");
});

test("a blank recorded path is no path at all", () => {
  const resolved = resolveCodingSessionHireCheckout({
    projectRef: null,
    channelId: CHANNEL,
    byProject: {},
    byChannel: { [CHANNEL]: { path: "   " } },
  });
  assert.equal(resolved.kind, "unrecorded");
});

test("the umbrella's line says where the tree came from and which record said so", () => {
  assert.equal(
    codingSessionHireCheckoutLine({
      role: "builder",
      path: TANK_LOOP,
      source: "project",
      passedOver: null,
    }),
    `Hired a builder — worktree cut from ${TANK_LOOP} (project checkout)`,
  );
  assert.match(
    codingSessionHireCheckoutLine({
      role: "verifier",
      path: BEEKEEPER,
      source: "channel",
      passedOver:
        "the folder this channel last used, /elsewhere, is not inside /x and was not used",
    }),
    /\(this session's remembered folder\); the folder this channel last used/,
  );
});
