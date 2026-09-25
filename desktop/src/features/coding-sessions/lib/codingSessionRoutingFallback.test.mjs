import assert from "node:assert/strict";
import { test } from "node:test";

import {
  bareModelId,
  fallbackCatalogTarget,
  fallbackHardBlock,
} from "./codingSessionRoutingFallback.ts";

const EMPTY_REGISTRY = { version: 1, tiers: {}, classes: {}, targets: [] };

test("bareModelId strips a trailing [effort] variant, and nothing else", () => {
  assert.equal(bareModelId("gpt-5.6-sol[high]"), "gpt-5.6-sol");
  assert.equal(bareModelId("claude-nova-9"), "claude-nova-9");
  assert.equal(bareModelId(""), "");
});

test("fallbackHardBlock is null for an ordinary class with no requires", () => {
  assert.equal(fallbackHardBlock({ minimums: {} }, undefined), null);
});

test("fallbackHardBlock names a class's own multimodal requirement", () => {
  const why = fallbackHardBlock(
    { minimums: {}, requires: { multimodal: true } },
    undefined,
  );
  assert.match(why ?? "", /multimodal/);
});

test("fallbackHardBlock names a task's own tool requirement", () => {
  const why = fallbackHardBlock({ minimums: {} }, { tools: ["search"] });
  assert.match(why ?? "", /search/);
});

test("fallbackHardBlock names an intolerable failure mode requirement", () => {
  const why = fallbackHardBlock(
    { minimums: {} },
    { incompatibleFailureModes: ["hallucination"] },
  );
  assert.match(why ?? "", /failure mode/);
});

test("fallbackCatalogTarget picks the catalog's first unregistered, unblocked offer", () => {
  const excluded = [];
  const decision = fallbackCatalogTarget({
    registry: EMPTY_REGISTRY,
    catalog: [
      { providerInstanceRef: "claude-primary", model: "claude-nova-9" },
      { providerInstanceRef: "claude-primary", model: "claude-nova-10" },
    ],
    classGate: { minimums: {} },
    requirements: undefined,
    effort: "medium",
    excluded,
  });
  assert.equal(decision.ok, true);
  assert.deepEqual(decision.ok ? decision.target : null, {
    provider: "claude-primary",
    model: "claude-nova-9",
    effort: "medium",
  });
  assert.equal(excluded.length, 0);
});

test("fallbackCatalogTarget never reconsiders a catalog entry a registry row already covers", () => {
  const decision = fallbackCatalogTarget({
    registry: {
      ...EMPTY_REGISTRY,
      targets: [
        {
          provider: "claude-primary",
          model: "claude-nova-9",
          scores: {},
          facts: { knownFailureModes: [] },
          status: {},
          rating: {
            status: "operational_opinion",
            confidence: "low",
            author: "test",
            date: "2026-09-25",
          },
        },
      ],
    },
    catalog: [
      { providerInstanceRef: "claude-primary", model: "claude-nova-9" },
    ],
    classGate: { minimums: {} },
    requirements: undefined,
    effort: "medium",
    excluded: [],
  });
  assert.equal(decision.ok, false);
  assert.equal(decision.ok ? null : decision.why, null);
});

test("fallbackCatalogTarget refuses, naming the block, when every offer is unregistered and blocked", () => {
  const excluded = [];
  const decision = fallbackCatalogTarget({
    registry: EMPTY_REGISTRY,
    catalog: [
      { providerInstanceRef: "claude-primary", model: "claude-nova-9" },
    ],
    classGate: { minimums: {}, requires: { multimodal: true } },
    requirements: undefined,
    effort: "medium",
    excluded,
  });
  assert.equal(decision.ok, false);
  assert.match(decision.ok ? "" : (decision.why ?? ""), /multimodal/);
  assert.equal(excluded.length, 1);
  assert.match(excluded[0].why, /multimodal/);
});

test("fallbackCatalogTarget checks minContextWindow per catalog entry, no registry row needed", () => {
  const tooSmall = fallbackCatalogTarget({
    registry: EMPTY_REGISTRY,
    catalog: [
      {
        providerInstanceRef: "claude-primary",
        model: "claude-nova-9",
        contextWindow: 8000,
      },
    ],
    classGate: { minimums: {} },
    requirements: { minContextWindow: 32000 },
    effort: "medium",
    excluded: [],
  });
  assert.equal(tooSmall.ok, false);

  const bigEnough = fallbackCatalogTarget({
    registry: EMPTY_REGISTRY,
    catalog: [
      {
        providerInstanceRef: "claude-primary",
        model: "claude-nova-9",
        contextWindow: 200000,
      },
    ],
    classGate: { minimums: {} },
    requirements: { minContextWindow: 32000 },
    effort: "medium",
    excluded: [],
  });
  assert.equal(bigEnough.ok, true);
});
