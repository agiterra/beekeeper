/**
 * What this host does with a lead's routing request — including the two
 * refusals it owes rather than guessing its way past.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  CODING_SESSION_ROUTING_REGISTRY_UNREADABLE,
  isCodingSessionHireRoutingRequest,
  resolveCodingSessionHireRouting,
} from "./codingSessionHireRouting.ts";
import { describeUnreadableModelRegistry } from "./codingSessionRegistryAccess.ts";

const REGISTRY_TEXT = readFileSync(
  new URL("../../../../../team/model-registry.yaml", import.meta.url),
  "utf8",
);
const CLAUDE_CATALOG = [
  "claude-fable-5[1m]",
  "default",
  "haiku",
  "opus[1m]",
  "sonnet",
];
const READABLE = {
  kind: "readable",
  text: REGISTRY_TEXT,
  label: "team/model-registry.yaml",
};
const REQUEST = {
  class: "builder",
  risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
};

function resolve(overrides = {}) {
  return resolveCodingSessionHireRouting({
    request: REQUEST,
    requestedModel: null,
    registry: READABLE,
    providerInstanceRef: "claude-primary",
    offeredModels: CLAUDE_CATALOG,
    catalogRevision: 4,
    ...overrides,
  });
}

test("a hire that asks for nothing is routed by nothing", () => {
  assert.deepEqual(
    resolve({ request: undefined, registry: { kind: "unreadable", why: "x" } }),
    { kind: "not-requested" },
  );
});

test("a routed hire is answered with the whole decision", () => {
  const outcome = resolve();
  assert.equal(outcome.kind, "routed");
  assert.deepEqual(outcome.record.chosen, {
    provider: "claude-primary",
    model: "sonnet",
    effort: "medium",
  });
  assert.equal(outcome.record.class, "builder");
  assert.equal(outcome.record.tier, "standard");
  assert.deepEqual(outcome.record.risk, {
    impact: 3,
    uncertainty: 3,
    irreversibility: 2,
    score: 18,
  });
  assert.equal(outcome.record.registryVersion, 1);
  assert.equal(outcome.record.catalogRevision, 4);
  assert.equal(outcome.record.override, null);
});

test("the router chooses within the seat's own runtime, never across it", () => {
  // The identity decides the runtime (item 88(i)); the catalog handed in is
  // that runtime's alone, so no decision can name a vendor the seat cannot run.
  const outcome = resolve();
  assert.equal(outcome.record.chosen.provider, "claude-primary");
  assert.notEqual(outcome.record.runnerUp?.provider, "codex-primary");
});

test("a registry this host cannot read refuses the hire, it does not route it", () => {
  const outcome = resolve({
    registry: describeUnreadableModelRegistry("/Users/x/checkout"),
  });
  assert.equal(outcome.kind, "refused");
  assert.equal(outcome.code, "HIRE_NO_ROUTE");
  assert.ok(
    outcome.reason.startsWith(CODING_SESSION_ROUTING_REGISTRY_UNREADABLE),
  );
  assert.match(
    outcome.reason,
    /\/Users\/x\/checkout\/team\/model-registry\.yaml/,
  );
});

test("a registry that will not parse is the same refusal, with the reason", () => {
  const outcome = resolve({
    registry: { kind: "readable", text: "version: 9\n", label: "bad" },
  });
  assert.equal(outcome.kind, "refused");
  assert.match(outcome.reason, /version 9/);
});

test("a runtime with no catalog read is refused, never routed against nothing", () => {
  const outcome = resolve({ offeredModels: [] });
  assert.equal(outcome.kind, "refused");
  assert.match(outcome.reason, /no model catalog for claude-primary/);
});

test("a routed hire naming a model is an override, and an override needs a because", () => {
  const bare = resolve({ requestedModel: "opus[1m]" });
  assert.equal(bare.kind, "refused");
  assert.match(bare.reason, /routing\.override\.because/);

  const given = resolve({
    requestedModel: "opus[1m]",
    request: {
      ...REQUEST,
      override: { because: "Brian asked for the big one" },
    },
  });
  assert.equal(given.kind, "routed");
  assert.deepEqual(given.record.chosen, {
    provider: "claude-primary",
    model: "opus[1m]",
    effort: "medium",
  });
  assert.deepEqual(given.record.override, {
    model: "opus[1m]",
    because: "Brian asked for the big one",
  });
  // The router's own answer survives as the runner-up, so the record reads as
  // an override rather than as the router's opinion.
  assert.deepEqual(given.record.runnerUp, {
    provider: "claude-primary",
    model: "sonnet",
    effort: "medium",
  });
});

test("an override naming an id the catalog does not publish is refused", () => {
  const outcome = resolve({
    requestedModel: "claude-sonnet-5",
    request: { ...REQUEST, override: { because: "muscle memory" } },
  });
  assert.equal(outcome.kind, "refused");
  assert.match(outcome.reason, /claude-sonnet-5/);
  assert.match(outcome.reason, /nothing is translated/);
});

test("a reason with no model is refused rather than read as a route", () => {
  const outcome = resolve({
    request: { ...REQUEST, override: { because: "because I said so" } },
  });
  assert.equal(outcome.kind, "refused");
  assert.match(outcome.reason, /names no model/);
});

test("the wire check accepts a request and refuses anything that names a target", () => {
  assert.equal(isCodingSessionHireRoutingRequest(REQUEST), true);
  assert.equal(
    isCodingSessionHireRoutingRequest({
      ...REQUEST,
      tier: "deep",
      profile: { verification: 4.7 },
      override: { model: "sonnet", effort: "high", because: "why" },
    }),
    true,
  );
  for (const bad of [
    null,
    {},
    { class: "Builder", risk: REQUEST.risk },
    {
      class: "builder",
      risk: { impact: 6, uncertainty: 1, irreversibility: 1 },
    },
    { ...REQUEST, profile: { nonsense: 4 } },
    { ...REQUEST, profile: { reasoning: 9 } },
    { ...REQUEST, chosen: { provider: "p", model: "m", effort: "low" } },
    { ...REQUEST, override: { model: "sonnet", because: " " } },
    {
      ...REQUEST,
      override: { model: "sonnet", effort: "xhigh", because: "why" },
    },
  ]) {
    assert.equal(
      isCodingSessionHireRoutingRequest(bad),
      false,
      `accepted ${JSON.stringify(bad)}`,
    );
  }
});
