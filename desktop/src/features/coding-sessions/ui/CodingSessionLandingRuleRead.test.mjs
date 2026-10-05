/**
 * DB5 / SV-22: a refusing newest verdict reaches Landing's badge — and so the
 * header's right-panel dot — through Landing's own `readExtension`, with the
 * panel closed.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionLandingRefusingVerdict,
  readCodingSessionLandingExtension,
} from "./CodingSessionLandingRuleRead.ts";
import { codingSessionSurfaceLanding } from "./surfaces/CodingSessionSurfaceLanding.tsx";
import { codingSessionLandingBadgeFromCtx } from "../lib/codingSessionSurfaceBadgeModelCtx.ts";
import { strongestCodingSessionSurfaceBadgeTone } from "../lib/codingSessionSurfaceBadgeModel.ts";

const SEAT = "11".repeat(32);

function rule(decision) {
  return {
    state: "read",
    land: { state: "ready", headSha: null },
    newestVerdict: {
      eventId: "v".repeat(64),
      authorPubkey: SEAT,
      decision,
      reportEventId: "r".repeat(64),
      headSha: null,
    },
  };
}

const resolveWho = (pubkey) => (pubkey === SEAT ? "Bob" : pubkey.slice(0, 8));

function ctxWith(landing) {
  return {
    observations: { state: "not-read", reason: "no genesis" },
    extensions: { landing },
    activeSurfaceId: null,
    currentUserPubkey: null,
    founderPubkey: null,
    resolveActorName: () => null,
  };
}

test("Landing registers a readExtension (the B3 contract)", () => {
  assert.equal(typeof codingSessionSurfaceLanding.readExtension, "function");
});

test("only a refusing newest verdict is shared, labelled with its word and signer", () => {
  assert.deepEqual(
    codingSessionLandingRefusingVerdict(rule("reject"), resolveWho),
    { label: "reject by Bob" },
  );
  assert.deepEqual(
    codingSessionLandingRefusingVerdict(rule("refuted"), resolveWho),
    { label: "refuted by Bob" },
  );
  assert.equal(
    codingSessionLandingRefusingVerdict(rule("approve"), resolveWho),
    null,
  );
  assert.equal(
    codingSessionLandingRefusingVerdict({ state: "asking" }, resolveWho),
    null,
  );
});

test("a 'reject' verdict gives Landing an attention badge with the panel closed", () => {
  const extension = {
    refusingVerdict: codingSessionLandingRefusingVerdict(
      rule("reject"),
      resolveWho,
    ),
    rule: rule("reject"),
    repository: null,
    repoRef: null,
    hasGenesis: true,
  };
  assert.ok(readCodingSessionLandingExtension(extension));
  const badge = codingSessionLandingBadgeFromCtx(ctxWith(extension), []);
  assert.equal(badge?.tone, "attention");
  assert.equal(strongestCodingSessionSurfaceBadgeTone([badge]), "attention");
  assert.ok(badge.facts.some((fact) => fact.includes("reject by Bob")));

  const approved = codingSessionLandingBadgeFromCtx(
    ctxWith({ ...extension, refusingVerdict: null, rule: rule("approve") }),
    [],
  );
  assert.equal(approved, null);
});
