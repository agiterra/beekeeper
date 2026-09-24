import assert from "node:assert/strict";
import { test } from "node:test";

import { autorunGrantState, standingGrantNote } from "./autorunState.ts";

const PUBKEY = "3d".repeat(32);

test("autorunGrantState reads no grants as none", () => {
  assert.deepEqual(autorunGrantState(undefined), { kind: "none" });
  assert.deepEqual(autorunGrantState([]), { kind: "none" });
});

test("autorunGrantState prefers an active grant over a stale one", () => {
  const state = autorunGrantState([
    { revokedAt: null, matchesCurrent: false, grantedBy: "stale-key" },
    { revokedAt: null, matchesCurrent: true, grantedBy: PUBKEY },
  ]);
  assert.deepEqual(state, { kind: "active", grantedBy: PUBKEY });
});

test("autorunGrantState reports stale when only an old-hash grant stands", () => {
  const state = autorunGrantState([
    { revokedAt: null, matchesCurrent: false, grantedBy: PUBKEY },
  ]);
  assert.deepEqual(state, { kind: "stale" });
});

test("autorunGrantState ignores a revoked grant", () => {
  const state = autorunGrantState([
    {
      revokedAt: "2026-09-24T00:00:00Z",
      matchesCurrent: true,
      grantedBy: PUBKEY,
    },
  ]);
  assert.deepEqual(state, { kind: "none" });
});

test("standingGrantNote calls out a grant with zero runs — the honest Actions-tab label", () => {
  const active = { kind: "active", grantedBy: PUBKEY };
  assert.equal(standingGrantNote(active, 0), "standing grant · no run yet");
});

test("standingGrantNote says nothing once any run exists, or without an active grant", () => {
  const active = { kind: "active", grantedBy: PUBKEY };
  assert.equal(standingGrantNote(active, 1), null);
  assert.equal(standingGrantNote({ kind: "stale" }, 0), null);
  assert.equal(standingGrantNote({ kind: "none" }, 0), null);
});
