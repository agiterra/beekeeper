import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import {
  codingSessionRiskScore,
  codingSessionRiskTier,
  describeCodingSessionRouting,
  isStrictCodingSessionRoutingRecord,
  parseModelRegistry,
  ROUTING_EFFORT_FOR_TIER,
  routeCodingSession,
} from "./codingSessionRouting.ts";

const registryText = readFileSync(
  new URL(
    "../../../../../testdata/routing/registry-fixture.yaml",
    import.meta.url,
  ),
  "utf8",
);
const liveCatalog = JSON.parse(
  readFileSync(
    new URL(
      "../../../../../testdata/rubric/live-catalog-665076ce.json",
      import.meta.url,
    ),
    "utf8",
  ),
);
const expectedDecisions = JSON.parse(
  readFileSync(
    new URL(
      "../../../../../testdata/routing/expected-decisions.json",
      import.meta.url,
    ),
    "utf8",
  ),
);

function registry() {
  const parsed = parseModelRegistry(registryText);
  assert.equal(parsed.ok, true, parsed.ok ? "" : parsed.why);
  return parsed.registry;
}

const CATALOG = liveCatalog.offered;

function route(overrides = {}) {
  return routeCodingSession({
    registry: registry(),
    catalog: CATALOG,
    catalogRevision: null,
    className: "builder",
    risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
    ...overrides,
  });
}

test("risk is impact × uncertainty × irreversibility, and the tier is the band", () => {
  assert.equal(
    codingSessionRiskScore({ impact: 3, uncertainty: 3, irreversibility: 2 }),
    18,
  );
  const model = registry();
  assert.equal(codingSessionRiskTier(model, 1), "fast");
  assert.equal(codingSessionRiskTier(model, 8), "fast");
  assert.equal(codingSessionRiskTier(model, 9), "standard");
  assert.equal(codingSessionRiskTier(model, 39), "standard");
  assert.equal(codingSessionRiskTier(model, 40), "deep");
  assert.equal(codingSessionRiskTier(model, 125), "deep");
});

test("the router buys exactly three efforts and never xhigh, max or ultra", () => {
  assert.deepEqual(ROUTING_EFFORT_FOR_TIER, {
    fast: "low",
    standard: "medium",
    deep: "high",
  });
  for (const tier of ["fast", "standard", "deep"]) {
    const risk =
      tier === "fast"
        ? { impact: 1, uncertainty: 1, irreversibility: 1 }
        : tier === "standard"
          ? { impact: 3, uncertainty: 3, irreversibility: 2 }
          : { impact: 5, uncertainty: 5, irreversibility: 5 };
    const decision = route({ risk });
    assert.equal(decision.ok, true);
    assert.ok(
      ["low", "medium", "high"].includes(decision.record.chosen.effort),
      `${tier} bought ${decision.record.chosen.effort}`,
    );
    assert.ok(!/\[(xhigh|max|ultra)\]$/.test(decision.record.chosen.model));
  }
});

test("every recorded decision in the pinned fixture still holds", () => {
  for (const expected of expectedDecisions.decisions) {
    const decision = routeCodingSession({
      registry: registry(),
      catalog: CATALOG,
      catalogRevision: null,
      className: expected.input.class,
      risk: expected.input.risk,
      ...(expected.input.sampleChallenger ? { sampleChallenger: true } : {}),
      ...(expected.input.peer ? { peer: expected.input.peer } : {}),
      ...(expected.input.requirements
        ? { requirements: expected.input.requirements }
        : {}),
    });
    if (expected.outcome === "no-route") {
      assert.equal(decision.ok, false, `${expected.name} should not route`);
      assert.equal(decision.code, "HIRE_NO_ROUTE");
      assert.match(decision.reason, new RegExp(expected.reasonMatch));
      continue;
    }
    assert.equal(decision.ok, true, `${expected.name} should route`);
    assert.deepEqual(
      decision.record.chosen,
      expected.chosen,
      `${expected.name} chose ${JSON.stringify(decision.record.chosen)}`,
    );
    assert.deepEqual(
      decision.record.runnerUp,
      expected.runnerUp,
      `${expected.name} runner-up ${JSON.stringify(decision.record.runnerUp)}`,
    );
    assert.equal(decision.record.tier, expected.tier);
    assert.equal(decision.record.challengerSample, expected.challengerSample);
  }
});

test("cost and speed choose only among targets that already cleared the gates", () => {
  // Haiku is the cheapest and fastest thing this catalog offers, and it is
  // never a builder: coding 3.7 misses the class minimum of 4.2. A weighted
  // product would have let velocity 5.0 and cost 4.6 buy that back.
  const decision = route({
    risk: { impact: 1, uncertainty: 1, irreversibility: 1 },
  });
  assert.equal(decision.ok, true);
  assert.notEqual(decision.record.chosen.model, "haiku");
  const excluded = decision.excluded.find(
    (entry) => entry.provider === "claude-primary" && entry.model === "haiku",
  );
  assert.ok(excluded, "haiku should be recorded as excluded");
  assert.match(excluded.why, /coding/);
});

test("a class minimum a target has no score for excludes it rather than passing it", () => {
  const model = registry();
  const spark = model.targets.find(
    (target) => target.model === "gpt-5.3-codex-spark",
  );
  assert.equal(spark.scores.costEfficiency, null);
  // Spark's cost prior is null, so its expected cost cannot be estimated. It
  // is still eligible for runner work — an unknown price is not a
  // disqualification — but it never outranks a target whose cost is known.
  const decision = route({
    className: "runner",
    risk: { impact: 1, uncertainty: 1, irreversibility: 1 },
  });
  assert.equal(decision.ok, true);
  assert.notEqual(decision.record.chosen.model, "gpt-5.3-codex-spark");
});

test("Spark is refused work outside its hard constraints", () => {
  const decision = route({
    className: "runner",
    risk: { impact: 1, uncertainty: 1, irreversibility: 1 },
    requirements: { ambiguity: 4, scope: "bounded" },
  });
  assert.equal(decision.ok, true);
  const excluded = decision.excluded.find(
    (entry) => entry.model === "gpt-5.3-codex-spark",
  );
  assert.ok(excluded);
  assert.match(excluded.why, /ambiguity/);
});

test("a class requiring modality drops a target that is not multimodal", () => {
  const decision = route({
    className: "ui_designer",
    risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
  });
  assert.equal(decision.ok, true);
  const spark = decision.excluded.find(
    (entry) => entry.model === "gpt-5.3-codex-spark",
  );
  assert.ok(spark);
  assert.match(spark.why, /multimodal/);
});

test("a required tool nobody has recorded excludes the target, it does not pass it", () => {
  const decision = route({
    className: "researcher",
    risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
  });
  assert.equal(decision.ok, false);
  assert.equal(decision.code, "HIRE_NO_ROUTE");
  assert.match(decision.reason, /search/);
});

test("a verifier avoids the builder's vendor when an eligible one exists", () => {
  const decision = route({
    className: "verifier",
    risk: { impact: 4, uncertainty: 3, irreversibility: 3 },
    peer: { className: "builder", provider: "claude-primary" },
  });
  assert.equal(decision.ok, true);
  assert.equal(decision.record.chosen.provider, "codex-primary");
  const opus = decision.excluded.find((entry) => entry.model === "opus[1m]");
  assert.ok(opus);
  assert.match(opus.why, /cross-provider/);
});

test("a target the live catalog does not offer is dormant, not chosen", () => {
  const decision = routeCodingSession({
    registry: registry(),
    // Only Claude is live on this host today.
    catalog: CATALOG.filter(
      (pair) => pair.providerInstanceRef === "claude-primary",
    ),
    catalogRevision: 7,
    className: "builder",
    risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
  });
  assert.equal(decision.ok, true);
  assert.equal(decision.record.chosen.provider, "claude-primary");
  assert.equal(decision.record.catalogRevision, 7);
  const luna = decision.excluded.find(
    (entry) => entry.model === "gpt-5.6-luna",
  );
  assert.ok(luna);
  assert.match(luna.why, /not offered/);
});

test("no eligible target is HIRE_NO_ROUTE, never a quietly weakened requirement", () => {
  const decision = routeCodingSession({
    registry: registry(),
    catalog: [{ providerInstanceRef: "claude-primary", model: "haiku" }],
    catalogRevision: null,
    className: "lead",
    risk: { impact: 5, uncertainty: 5, irreversibility: 5 },
  });
  assert.equal(decision.ok, false);
  assert.equal(decision.code, "HIRE_NO_ROUTE");
  assert.match(decision.reason, /lead/);
});

test("an override is validated against the catalog exactly and needs a because", () => {
  const missingBecause = route({
    override: { model: "gpt-5.6-sol[high]", because: "   " },
  });
  assert.equal(missingBecause.ok, false);
  assert.match(missingBecause.reason, /because/);

  const notOffered = route({
    override: { model: "claude-sonnet-5", because: "Brian asked for it" },
  });
  assert.equal(notOffered.ok, false);
  assert.match(notOffered.reason, /claude-sonnet-5/);

  const honoured = route({
    override: { model: "gpt-5.6-sol[xhigh]", because: "Brian asked for it" },
  });
  assert.equal(honoured.ok, true);
  assert.deepEqual(honoured.record.chosen, {
    provider: "codex-primary",
    model: "gpt-5.6-sol[xhigh]",
    effort: "medium",
  });
  assert.deepEqual(honoured.record.override, {
    model: "gpt-5.6-sol[xhigh]",
    because: "Brian asked for it",
  });
  // The router's own choice is still recorded, so the override is readable as
  // an override rather than as the router's opinion.
  assert.ok(honoured.record.runnerUp);
  assert.match(honoured.record.reason, /override/);
});

test("review is triggered by the spec's list, not by the deep tier", () => {
  const deep = route({
    risk: { impact: 5, uncertainty: 5, irreversibility: 5 },
  });
  assert.equal(deep.record.reviewRequired, true);
  assert.ok(deep.record.reviewReasons.includes("risk>=40"));

  const irreversible = route({
    risk: { impact: 1, uncertainty: 1, irreversibility: 4 },
  });
  assert.equal(irreversible.record.tier, "fast");
  assert.equal(irreversible.record.reviewRequired, true);
  assert.ok(irreversible.record.reviewReasons.includes("irreversibility>=4"));

  const quiet = route({
    risk: { impact: 2, uncertainty: 2, irreversibility: 2 },
  });
  assert.equal(quiet.record.reviewRequired, false);
  assert.deepEqual(quiet.record.reviewReasons, []);

  const boundary = route({
    risk: { impact: 2, uncertainty: 2, irreversibility: 2 },
    review: { securityBoundary: true, leadRequested: true },
  });
  assert.equal(boundary.record.reviewRequired, true);
  assert.deepEqual(boundary.record.reviewReasons, [
    "security-auth-data-boundary",
    "lead-requested",
  ]);
});

test("sampling a challenger is marked on the record", () => {
  const incumbent = route({});
  assert.equal(incumbent.record.challengerSample, false);
  const sampled = route({ sampleChallenger: true });
  assert.equal(sampled.record.challengerSample, true);
  assert.notDeepEqual(sampled.record.chosen, incumbent.record.chosen);
});

test("the record round-trips through the strict wire check", () => {
  const decision = route({});
  assert.equal(isStrictCodingSessionRoutingRecord(decision.record), true);
  assert.equal(
    isStrictCodingSessionRoutingRecord(
      JSON.parse(JSON.stringify(decision.record)),
    ),
    true,
  );
  assert.equal(
    isStrictCodingSessionRoutingRecord({
      ...decision.record,
      chosen: { ...decision.record.chosen, effort: "xhigh" },
    }),
    false,
    "the wire never carries an effort the router may not buy",
  );
  assert.equal(
    isStrictCodingSessionRoutingRecord({ ...decision.record, extra: 1 }),
    false,
  );
});

test("the seat's one-line disclosure names the class, tier, model and reason", () => {
  const decision = route({});
  const line = describeCodingSessionRouting(decision.record);
  assert.match(
    line,
    /^routed: builder\/standard → claude-primary\/sonnet \(medium\) — /,
  );
  assert.equal(line.includes("\n"), false, "one line, never a panel");
});

test("a registry that cannot be parsed says so rather than routing on a guess", () => {
  assert.equal(parseModelRegistry("").ok, false);
  assert.equal(parseModelRegistry("version: 2\ntargets: []\n").ok, false);
  assert.match(parseModelRegistry("version: 2\ntargets: []\n").why, /version/);
  assert.equal(parseModelRegistry(": : :").ok, false);
});
