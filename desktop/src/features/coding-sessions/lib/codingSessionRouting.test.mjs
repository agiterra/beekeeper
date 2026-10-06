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

// ONE registry and ONE catalog, shared with the canonical Rust router.
//
// `team/model-registry.yaml` is the registry the product itself reads — not a
// copy pinned for tests. A test fixture that had drifted from it would let
// this suite go green on rules the shipped router does not follow, which is
// the specific failure the cross-implementation contract exists to prevent.
// The expected decisions live *inside* the catalog fixture, and
// `every_recorded_decision_in_the_fixture_still_holds`
// (crates/beekeeper-core/src/coding_session_routing.rs:2872) asserts the same six
// against the Rust router, so the two cannot silently disagree.
const registryText = readFileSync(
  new URL("../../../../../team/model-registry.yaml", import.meta.url),
  "utf8",
);
const liveCatalog = JSON.parse(
  readFileSync(
    new URL(
      "../../../../../testdata/routing/live-catalog-665076ce.json",
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

test("a record without extra trait minimums writes profile null", () => {
  const decision = route();
  assert.equal(decision.ok, true);
  assert.equal(Object.hasOwn(decision.record, "profile"), true);
  assert.equal(decision.record.profile, null);

  const profiled = route({ profile: { verification: 4.7 } });
  assert.equal(profiled.ok, true);
  assert.deepEqual(profiled.record.profile, { verification: 4.7 });
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

test("every recorded decision in the shared fixture still holds", () => {
  const cases = liveCatalog.expectedDecisions;
  assert.equal(cases.length, 6, "the fixture records six decisions");
  for (const expected of cases) {
    const label = `${expected.class} ${JSON.stringify(expected.risk)}`;
    const decision = routeCodingSession({
      registry: registry(),
      catalog: CATALOG,
      catalogRevision: null,
      className: expected.class,
      risk: expected.risk,
      sampleChallenger: expected.challengerSample === true,
      peer: expected.counterpartProvider
        ? { className: "builder", provider: expected.counterpartProvider }
        : null,
    });
    assert.equal(decision.ok, true, `${label} should route`);
    assert.equal(decision.record.tier, expected.tier, `${label} tier`);
    assert.equal(
      `${decision.record.chosen.provider}/${decision.record.chosen.model}`,
      expected.chosen,
      label,
    );
    assert.equal(
      decision.record.runnerUp === null
        ? null
        : `${decision.record.runnerUp.provider}/${decision.record.runnerUp.model}`,
      expected.runnerUp,
      `${label} runner-up`,
    );
    assert.equal(
      decision.record.chosen.effort,
      expected.effort,
      `${label} effort`,
    );
    assert.equal(
      decision.record.reviewRequired,
      expected.reviewRequired,
      `${label} reviewRequired`,
    );
    assert.deepEqual(
      decision.record.reviewReasons,
      expected.reviewReasons,
      `${label} reviewReasons`,
    );
    assert.equal(
      decision.record.challengerSample,
      expected.challengerSample,
      `${label} challengerSample`,
    );
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

test("a required tool a target does not have excludes it, it does not pass it", () => {
  // The shipped registry records `tools: [search]` for every row but Spark,
  // and says in `factsProvenance` that those values are lane-drafted. Spark's
  // list is empty, so the researcher gate removes it by name rather than
  // letting a 5.0 velocity argue its way past a missing capability.
  const decision = route({
    className: "researcher",
    risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
  });
  assert.equal(decision.ok, true);
  const spark = decision.excluded.find(
    (entry) => entry.model === "gpt-5.3-codex-spark",
  );
  assert.ok(spark);
  assert.match(spark.why, /search/);
});

test("a registry with no tools recorded routes nothing that needs one", () => {
  // The other direction, and the honest one: strip the lane-drafted facts and
  // the researcher class has nothing it can prove is eligible. An unrecorded
  // capability is a refusal, never an assumption.
  // Only the per-row facts are stripped; the class's own `requires` stays.
  const stripped = parseModelRegistry(
    registryText.replaceAll("\n      tools: [search]", "\n      tools: []"),
  );
  assert.equal(stripped.ok, true, stripped.ok ? "" : stripped.why);
  const decision = routeCodingSession({
    registry: stripped.registry,
    catalog: CATALOG,
    catalogRevision: null,
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
  assert.ok(deep.record.reviewReasons.includes("risk 125 >= 40"));

  const irreversible = route({
    risk: { impact: 1, uncertainty: 1, irreversibility: 4 },
  });
  assert.equal(irreversible.record.tier, "fast");
  assert.equal(irreversible.record.reviewRequired, true);
  assert.ok(
    irreversible.record.reviewReasons.includes("irreversibility 4 >= 4"),
  );

  const quiet = route({
    risk: { impact: 2, uncertainty: 2, irreversibility: 2 },
  });
  assert.equal(quiet.record.reviewRequired, false);
  assert.deepEqual(quiet.record.reviewReasons, []);

  const boundary = route({
    risk: { impact: 2, uncertainty: 2, irreversibility: 2 },
    review: { securityBoundary: true, leadRequests: true },
  });
  assert.equal(boundary.record.reviewRequired, true);
  assert.deepEqual(boundary.record.reviewReasons, [
    "securityBoundary",
    "leadRequests",
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

// Ledger 267 (control run 8): the lead's routed verifier hire was refused
// `HIRE_NO_ROUTE` three times although an override named an eligible target,
// because the empty-registry-pool refusal ran before the override was even
// considered. These three tests pin the ruling: override first; then, absent
// an override, a catalog offer no registry row covers is a fallback for an
// ordinary session; a genuine hard requirement still refuses it, named.

const UNREGISTERED_CATALOG = [
  // `team/model-registry.yaml` has no row for either provider/model pair —
  // the run-8 shape, where the live catalog outruns the seeded registry.
  { providerInstanceRef: "claude-primary", model: "claude-nova-9" },
];

test("an empty registry pool still honours a valid override (run 8's shape)", () => {
  const decision = routeCodingSession({
    registry: registry(),
    catalog: UNREGISTERED_CATALOG,
    catalogRevision: null,
    className: "builder",
    risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
    override: {
      model: "claude-nova-9",
      because: "run 8: the registry has no row for it yet",
    },
  });
  assert.equal(decision.ok, true);
  assert.deepEqual(decision.record.chosen, {
    provider: "claude-primary",
    model: "claude-nova-9",
    effort: "medium",
  });
  assert.match(decision.record.reason, /override/);
  // No registry-ranked candidate existed to name as a runner-up.
  assert.equal(decision.record.runnerUp, null);
});

test("an empty pool with no hard requirement falls back to the catalog's own offer", () => {
  const decision = routeCodingSession({
    registry: registry(),
    catalog: UNREGISTERED_CATALOG,
    catalogRevision: null,
    // `builder` carries no `requires` at all — an ordinary, neutral class.
    className: "builder",
    risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
  });
  assert.equal(decision.ok, true);
  assert.deepEqual(decision.record.chosen, {
    provider: "claude-primary",
    model: "claude-nova-9",
    effort: "medium",
  });
  assert.match(decision.record.reason, /catalog fallback/);
  assert.equal(decision.record.override, null);
});

test("a hard policy still refuses every unregistered catalog offer, named", () => {
  const decision = routeCodingSession({
    registry: registry(),
    catalog: UNREGISTERED_CATALOG,
    catalogRevision: null,
    // `ui_designer` hard-requires multimodal (spec §4) — an unregistered
    // catalog offer carries no recorded modality to clear that with.
    className: "ui_designer",
    risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
  });
  assert.equal(decision.ok, false);
  assert.equal(decision.code, "HIRE_NO_ROUTE");
  assert.match(decision.reason, /multimodal/);
});

test("a non-empty ranked pool is unaffected by the fallback path", () => {
  // Same shape as the pre-existing override test, but pinned here to name the
  // control-run-8 fix directly: an override with a real ranked pool behind it
  // still records that pool's own choice as the runner-up.
  const decision = route({
    override: { model: "gpt-5.6-sol[xhigh]", because: "Brian asked for it" },
  });
  assert.equal(decision.ok, true);
  assert.ok(decision.record.runnerUp);
  assert.notEqual(decision.record.runnerUp.model, null);
  assert.match(decision.record.reason, /the router would have chosen/);
});
