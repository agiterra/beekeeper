/**
 * The one hire-routing contract, pinned to the shared fixtures.
 *
 * On 2026-08-30 a routed hire was accepted by the relay and answered by
 * nothing for fifteen minutes: the CLI emitted the routing RECORD and this
 * host's parser accepted only a REQUEST, so every routed hire classified
 * `malformed` and was dropped without a 44220, a console line or a pixel
 * (ledger draft 97). These tests hold the halves together — the same two JSON
 * files buzz-core's validator and the CLI's emitter are pinned to.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  isCodingSessionHireRoutingRequest,
  readCodingSessionHireRoutingRequest,
  resolveCodingSessionHireRouting,
} from "./codingSessionHireRouting.ts";
import { isStrictCodingSessionRoutingRecord } from "./codingSessionRoutingRecord.ts";
import { hasStrictRoutingRecord } from "@/shared/coordination/sessionCoordinationStrictJson.ts";

const HIRE_FIXTURE = JSON.parse(
  readFileSync(
    new URL(
      "../../../../../testdata/routing/hire-request-fixture.json",
      import.meta.url,
    ),
    "utf8",
  ),
);
const RECORD_FIXTURE = JSON.parse(
  readFileSync(
    new URL(
      "../../../../../testdata/routing/create-record-fixture.json",
      import.meta.url,
    ),
    "utf8",
  ),
);
const REGISTRY_TEXT = readFileSync(
  new URL("../../../../../team/model-registry.yaml", import.meta.url),
  "utf8",
);
const READABLE = {
  kind: "readable",
  text: REGISTRY_TEXT,
  label: "team/model-registry.yaml",
};
const CLAUDE_CATALOG = [
  "claude-fable-5[1m]",
  "default",
  "haiku",
  "opus[1m]",
  "sonnet",
];

// The fixture's override names `gpt-5.6-terra[medium]`, a codex target, so
// the override cases route inside the codex runtime. The identity decides the
// runtime (item 88(i)); a router free to cross vendors would produce a
// decision the host then had to quietly ignore.
const CODEX_CATALOG = [
  "gpt-5.4-mini[low]",
  "gpt-5.6-luna[low]",
  "gpt-5.6-luna[medium]",
  "gpt-5.6-sol[high]",
  "gpt-5.6-terra[medium]",
];
const OVERRIDE_CASE = "standard builder with an override and its because";
const OVERRIDE_MODEL = "gpt-5.6-terra[medium]";

function routingOf(caseName) {
  const entry = HIRE_FIXTURE.requests.find((row) => row.name === caseName);
  assert.ok(entry, `no fixture request named ${caseName}`);
  return entry.routing;
}

function resolve(overrides = {}) {
  return resolveCodingSessionHireRouting({
    request: routingOf("fast builder, no override"),
    requestedModel: null,
    registry: READABLE,
    providerInstanceRef: "claude-primary",
    offeredModels: CLAUDE_CATALOG,
    catalogRevision: 7,
    ...overrides,
  });
}

test("every request in the shared fixture is accepted by this host's parser", () => {
  assert.equal(HIRE_FIXTURE.requests.length, 3);
  for (const entry of HIRE_FIXTURE.requests) {
    const read = readCodingSessionHireRoutingRequest(entry.routing);
    assert.equal(read.ok, true, `${entry.name}: ${read.ok ? "" : read.why}`);
    assert.equal(isCodingSessionHireRoutingRequest(entry.routing), true);
  }
});

test("the request carries no tier and no risk score — both are the host's", () => {
  for (const entry of HIRE_FIXTURE.requests) {
    assert.equal(Object.hasOwn(entry.routing, "tier"), false, entry.name);
    assert.equal(Object.hasOwn(entry.routing.risk, "score"), false, entry.name);
  }
  for (const [bad, key] of [
    [
      { ...routingOf("fast builder, no override"), tier: "fast" },
      "routing.tier",
    ],
    [
      {
        class: "builder",
        risk: { impact: 2, uncertainty: 1, irreversibility: 2, score: 4 },
      },
      "routing.risk.score",
    ],
  ]) {
    const read = readCodingSessionHireRoutingRequest(bad);
    assert.equal(read.ok, false, `accepted ${JSON.stringify(bad)}`);
    assert.equal(read.key, key);
  }
});

test("the RECORD is refused as a request, naming the first key that does not belong", () => {
  // This is the exact payload the CLI published on 2026-08-30. It must be
  // refused *by name*, not dropped.
  const record = RECORD_FIXTURE.records[0].routing;
  const read = readCodingSessionHireRoutingRequest(record);
  assert.equal(read.ok, false);
  assert.equal(read.key, "routing.tier");
  assert.match(read.why, /the host derives/);
});

test("a malformed request names the failing key, always", () => {
  for (const [bad, key] of [
    [null, "routing"],
    [
      { risk: { impact: 1, uncertainty: 1, irreversibility: 1 } },
      "routing.class",
    ],
    [
      {
        class: "Builder",
        risk: { impact: 1, uncertainty: 1, irreversibility: 1 },
      },
      "routing.class",
    ],
    [{ class: "builder" }, "routing.risk"],
    [
      {
        class: "builder",
        risk: { impact: 6, uncertainty: 1, irreversibility: 1 },
      },
      "routing.risk.impact",
    ],
    [
      {
        class: "builder",
        risk: { impact: 1, uncertainty: 1, irreversibility: 1 },
        profile: { nonsense: 4 },
      },
      "routing.profile.nonsense",
    ],
    [
      {
        class: "builder",
        risk: { impact: 1, uncertainty: 1, irreversibility: 1 },
        reviewFlags: ["nope"],
      },
      "routing.reviewFlags[0]",
    ],
    [
      {
        class: "builder",
        risk: { impact: 1, uncertainty: 1, irreversibility: 1 },
        challengerSample: "yes",
      },
      "routing.challengerSample",
    ],
    [
      {
        class: "builder",
        risk: { impact: 1, uncertainty: 1, irreversibility: 1 },
        override: { because: "why" },
      },
      "routing.override.model",
    ],
    [
      {
        class: "builder",
        risk: { impact: 1, uncertainty: 1, irreversibility: 1 },
        override: { model: "sonnet", because: "  " },
      },
      "routing.override.because",
    ],
    [
      {
        class: "builder",
        risk: { impact: 1, uncertainty: 1, irreversibility: 1 },
        proposed: {
          chosen: { provider: "p", model: "m", effort: "ultra" },
          reason: "r",
          registryVersion: 1,
          catalogRevision: 7,
        },
      },
      "routing.proposed.chosen.effort",
    ],
  ]) {
    const read = readCodingSessionHireRoutingRequest(bad);
    assert.equal(read.ok, false, `accepted ${JSON.stringify(bad)}`);
    assert.equal(read.key, key, `wrong key for ${JSON.stringify(bad)}`);
    assert.ok(read.why.trim().length > 0, "a refusal with no sentence");
  }
});

test("a top-level model with no override is HIRE_MALFORMED, not HIRE_NO_ROUTE", () => {
  const outcome = resolve({ requestedModel: "opus[1m]" });
  assert.equal(outcome.kind, "refused");
  assert.equal(outcome.code, "HIRE_MALFORMED");
  assert.match(outcome.reason, /model without override\.because/);
});

test("an override with a because is honoured and recorded", () => {
  const request = routingOf(OVERRIDE_CASE);
  const outcome = resolve({
    request,
    requestedModel: OVERRIDE_MODEL,
    providerInstanceRef: "codex-primary",
    offeredModels: CODEX_CATALOG,
  });
  assert.equal(outcome.kind, "routed");
  assert.deepEqual(outcome.record.chosen, {
    provider: "codex-primary",
    model: OVERRIDE_MODEL,
    effort: "medium",
  });
  // `effort: null` is the canonical "take the tier's" answer buzz-core writes;
  // the host records the override it was given without inventing an effort.
  assert.equal(outcome.record.override.model, OVERRIDE_MODEL);
  assert.equal(outcome.record.override.because, request.override.because);
});

test("a top-level model that contradicts the override is malformed", () => {
  const outcome = resolve({
    request: routingOf(OVERRIDE_CASE),
    requestedModel: "gpt-5.6-sol[high]",
    providerInstanceRef: "codex-primary",
    offeredModels: CODEX_CATALOG,
  });
  assert.equal(outcome.kind, "refused");
  assert.equal(outcome.code, "HIRE_MALFORMED");
  assert.match(outcome.reason, /gpt-5\.6-sol\[high\]/);
  assert.match(outcome.reason, /gpt-5\.6-terra\[medium\]/);
});

test("every rejected shape in the shared fixture is refused, by the named key", () => {
  // The fixture's rejected cases are the exact payloads the drop of
  // 2026-08-30 09:52 put on the wire. Each names the key that does not
  // belong, and this host must name the same one.
  assert.equal(HIRE_FIXTURE.rejected.length, 2);
  for (const entry of HIRE_FIXTURE.rejected) {
    const read = readCodingSessionHireRoutingRequest(entry.routing);
    assert.equal(read.ok, false, `${entry.name} was accepted`);
    assert.ok(
      read.key.split(".").includes(entry.offendingKey),
      `${entry.name}: expected the refusal to name ${entry.offendingKey}, got ${read.key}`,
    );
  }
});

test("the record says so when the host's choice differs from what was proposed", () => {
  // The fixture's fast builder proposes codex-primary/gpt-5.6-luna; this host
  // is routing inside claude-primary's catalog, so it cannot honour it — and
  // a host that quietly substituted would be routing behind the requester.
  const outcome = resolve();
  assert.equal(outcome.kind, "routed");
  assert.notEqual(outcome.record.chosen.provider, "codex-primary");
  assert.ok(
    typeof outcome.record.proposedDisagreement === "string" &&
      outcome.record.proposedDisagreement.length > 0,
    "a disagreed-with proposal produced no sentence",
  );
  assert.match(
    outcome.record.proposedDisagreement,
    /codex-primary\/gpt-5\.6-luna/,
  );
  assert.match(
    outcome.record.proposedDisagreement,
    new RegExp(outcome.record.chosen.model.replace(/[[\]]/g, "\\$&")),
  );
});

test("a proposal the host agrees with adds no disagreement sentence", () => {
  const agreed = resolve({
    request: {
      ...routingOf("fast builder, no override"),
      proposed: {
        chosen: { provider: "claude-primary", model: "haiku", effort: "low" },
        reason: "the requester's own route",
        registryVersion: 1,
        catalogRevision: 7,
      },
    },
  });
  assert.equal(agreed.kind, "routed");
  if (
    agreed.record.chosen.provider === "claude-primary" &&
    agreed.record.chosen.model === "haiku" &&
    agreed.record.chosen.effort === "low"
  ) {
    assert.equal(Object.hasOwn(agreed.record, "proposedDisagreement"), false);
  }
});

test("every record in the shared fixture is accepted by the strict observer", () => {
  assert.equal(RECORD_FIXTURE.records.length, 3);
  for (const entry of RECORD_FIXTURE.records) {
    assert.equal(
      isStrictCodingSessionRoutingRecord(entry.routing),
      true,
      `${entry.name} was refused by the strict observer`,
    );
  }
});

test("profile: null is the shape the CLI emits, and it is accepted", () => {
  const [first] = RECORD_FIXTURE.records;
  assert.equal(first.routing.profile, null);
  assert.equal(isStrictCodingSessionRoutingRecord(first.routing), true);
});

test("every record in the shared fixture clears the coordination strict gate", () => {
  // Two record checks ship in this app — the feature's own validator and the
  // strict-JSON gate every incoming 44221/44223 passes through. A record the
  // second refuses never reaches the first, so both are pinned to the file.
  for (const entry of RECORD_FIXTURE.records) {
    assert.equal(
      hasStrictRoutingRecord(entry.routing),
      true,
      `${entry.name} was refused by the coordination gate`,
    );
  }
});
