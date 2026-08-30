import assert from "node:assert/strict";
import { test } from "node:test";

import {
  MAX_PULSE_CODE_AREAS,
  MAX_PULSE_COST_SEATS,
  MAX_PULSE_CODE_AREA_BYTES,
  MAX_PULSE_TEXT_BYTES,
  decodePulseEntry,
  normalizePulseProjectCoordinate,
  pulseEntryProjectCoordinate,
  pulseEntrySessionRef,
  validatePulseCodeArea,
  validatePulseEntryEnvelope,
} from "@/features/project-pulse/lib/pulseEntry";
import { normalizeProjectRef } from "@/features/projects-container/lib/projectContainerModel";
import { KIND_PULSE_ENTRY } from "@/shared/constants/kinds";

const OWNER = "a1".repeat(32);
const PROJECT = `30621:${OWNER}:pulse-demo`;
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

function content(overrides = {}) {
  return JSON.stringify({
    schema: "buzz-pulse-entry/v1",
    type: "plan",
    text: "Refactoring session creation.",
    codeAreas: [],
    branch: null,
    supersedes: null,
    ...overrides,
  });
}

function event(overrides = {}) {
  return {
    id: "a".repeat(64),
    pubkey: "b".repeat(64),
    created_at: 1_785_512_037,
    kind: KIND_PULSE_ENTRY,
    tags: [
      ["a", PROJECT],
      ["pu-v", "pu1-1"],
      ["pu-type", "plan"],
    ],
    content: content(),
    ...overrides,
  };
}

test("the local coordinate normalizer agrees with the shared one", () => {
  for (const candidate of [
    PROJECT,
    `30621:${OWNER.toUpperCase()}:pulse-demo`,
    "30617:abc:repo",
    "not-a-coordinate",
  ]) {
    assert.equal(
      normalizePulseProjectCoordinate(candidate),
      normalizeProjectRef(candidate),
      candidate,
    );
  }
});

test("the kind constant matches the shared registry", () => {
  assert.equal(KIND_PULSE_ENTRY, 44240);
});

test("every valid entry type decodes", () => {
  for (const type of ["plan", "milestone", "note", "handoff", "blocker"]) {
    const decoded = decodePulseEntry(content({ type }));
    assert.equal(decoded.ok, true, type);
  }
});

test("unknown content fields and wrong schemas are rejected", () => {
  assert.equal(decodePulseEntry(content({ extra: 1 })).ok, false);
  assert.equal(decodePulseEntry(content({ schema: "other/v1" })).ok, false);
  assert.equal(decodePulseEntry("not json").ok, false);
});

test("text must be prose and within its cap", () => {
  assert.equal(decodePulseEntry(content({ text: "   " })).ok, false);
  assert.equal(
    decodePulseEntry(content({ text: "x".repeat(MAX_PULSE_TEXT_BYTES) })).ok,
    true,
  );
  assert.equal(
    decodePulseEntry(content({ text: "x".repeat(MAX_PULSE_TEXT_BYTES + 1) }))
      .ok,
    false,
  );
});

test("code areas are repository-relative, deduplicated, and capped", () => {
  for (const bad of [
    "",
    "/abs/path",
    "~/home",
    "C:\\src",
    "a\\b",
    "a//b",
    "../escape",
    "dir/",
    "with\u0007bell",
  ]) {
    assert.notEqual(validatePulseCodeArea(bad), null, bad);
  }
  assert.equal(validatePulseCodeArea("crates/buzz-acp/src/pool.rs"), null);
  assert.equal(validatePulseCodeArea("./crates/pool.rs"), null);
  assert.equal(
    validatePulseCodeArea("x".repeat(MAX_PULSE_CODE_AREA_BYTES)),
    null,
  );
  assert.notEqual(
    validatePulseCodeArea("x".repeat(MAX_PULSE_CODE_AREA_BYTES + 1)),
    null,
  );

  const stripped = decodePulseEntry(content({ codeAreas: ["./a.rs"] }));
  assert.deepEqual(stripped.entry.codeAreas, ["a.rs"]);
  assert.equal(
    decodePulseEntry(content({ codeAreas: ["a.rs", "./a.rs"] })).ok,
    false,
    "a duplicate after stripping is a rejection, never a silent merge",
  );
  const areas = Array.from(
    { length: MAX_PULSE_CODE_AREAS + 1 },
    (_, index) => `a${index}.rs`,
  );
  assert.equal(decodePulseEntry(content({ codeAreas: areas })).ok, false);
  assert.equal(
    decodePulseEntry(
      content({ codeAreas: areas.slice(0, MAX_PULSE_CODE_AREAS) }),
    ).ok,
    true,
  );
});

test("the tag grammar is a closed key set with no ordering rule", () => {
  const reordered = event({
    tags: [
      ["pu-type", "plan"],
      ["pu-v", "pu1-1"],
      ["a", PROJECT],
    ],
  });
  assert.equal(validatePulseEntryEnvelope(reordered).ok, true);

  assert.equal(
    validatePulseEntryEnvelope(
      event({ tags: [...event().tags, ["nope", "x"]] }),
    ).ok,
    false,
  );
  assert.equal(
    validatePulseEntryEnvelope(
      event({ tags: [...event().tags, ["pu-type", "note"]] }),
    ).ok,
    false,
    "a duplicate singleton tag is a rejection",
  );
  assert.equal(
    validatePulseEntryEnvelope(event({ tags: [["a", PROJECT, "relay"]] })).ok,
    false,
    "every tag is exactly two fields",
  );
});

test("an h tag must be a lowercase canonical channel UUID", () => {
  const CHANNEL = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
  assert.equal(
    validatePulseEntryEnvelope(
      event({ tags: [...event().tags, ["h", CHANNEL]] }),
    ).ok,
    true,
  );
  // Spellings Rust's `Uuid::parse_str` also accepts; both languages must
  // reject them, or the same signed event folds one way in `bee pulse
  // digest` and another here.
  for (const nonCanonical of [
    CHANNEL.toUpperCase(),
    CHANNEL.replaceAll("-", ""),
    `{${CHANNEL}}`,
  ]) {
    const result = validatePulseEntryEnvelope(
      event({ tags: [...event().tags, ["h", nonCanonical]] }),
    );
    assert.equal(result.ok, false, nonCanonical);
    assert.match(result.error, /lowercase canonical channel UUID/);
  }
});

test("a non-canonical coordinate is rejected", () => {
  const upper = event({
    tags: [
      ["a", `30621:${OWNER.toUpperCase()}:pulse-demo`],
      ["pu-v", "pu1-1"],
      ["pu-type", "plan"],
    ],
  });
  const result = validatePulseEntryEnvelope(upper);
  assert.equal(result.ok, false);
  assert.match(result.error, /canonical/);
});

test("tag and content type/branch must agree", () => {
  assert.equal(
    validatePulseEntryEnvelope(
      event({
        tags: [
          ["a", PROJECT],
          ["pu-v", "pu1-1"],
          ["pu-type", "note"],
        ],
      }),
    ).ok,
    false,
  );
  assert.equal(
    validatePulseEntryEnvelope(
      event({
        tags: [
          ["a", PROJECT],
          ["pu-v", "pu1-1"],
          ["pu-type", "plan"],
          ["branch", "wip/one"],
        ],
        content: content({ branch: "wip/two" }),
      }),
    ).ok,
    false,
  );
});

test("supersedes is syntax-only, and never the event's own id", () => {
  const own = "a".repeat(64);
  assert.equal(
    validatePulseEntryEnvelope(event({ content: content({ supersedes: own }) }))
      .ok,
    false,
  );
  assert.equal(
    validatePulseEntryEnvelope(
      event({ content: content({ supersedes: "f".repeat(64) }) }),
    ).ok,
    true,
    "an unknown but well-formed id passes; the fold resolves it, not the decoder",
  );
  assert.equal(decodePulseEntry(content({ supersedes: "abc" })).ok, false);
});

test("the coordinate and session ref read back off the event", () => {
  assert.equal(pulseEntryProjectCoordinate(event()), PROJECT);
  assert.equal(pulseEntrySessionRef(event()), null);
  assert.equal(
    pulseEntrySessionRef(
      event({ tags: [...event().tags, ["pu-session", SESSION_REF]] }),
    ),
    SESSION_REF,
  );
  assert.equal(
    pulseEntryProjectCoordinate(event({ tags: [] })),
    null,
    "no coordinate closes the gate, it does not open it",
  );
});

const BUILDER =
  "cc00000000000000000000000000000000000000000000000000000000000022";
const REFUTER =
  "dd00000000000000000000000000000000000000000000000000000000000033";

const costContent = (cost) => content({ cost });

test("a costless entry decodes to a null cost", () => {
  const decoded = decodePulseEntry(content());
  assert.equal(decoded.ok, true);
  assert.equal(decoded.entry.cost, null);
});

test("a cost decodes with its seats and its total", () => {
  const decoded = decodePulseEntry(
    costContent({
      seats: [
        {
          actor: BUILDER,
          role: "builder",
          model: "opus-5[1m]",
          inputTokens: 10,
          outputTokens: 5,
          cacheReadTokens: 100,
          cacheWriteTokens: 20,
          toolCalls: 9,
          turns: 3,
        },
      ],
      totalTokens: 135,
    }),
  );
  assert.equal(decoded.ok, true);
  assert.equal(decoded.entry.cost.seats.length, 1);
  assert.equal(decoded.entry.cost.seats[0].role, "builder");
  assert.equal(decoded.entry.cost.seats[0].turns, 3);
  assert.equal(decoded.entry.cost.totalTokens, 135);
});

test("an empty cost, an empty seat, and an unknown cost key are rejected", () => {
  assert.match(
    decodePulseEntry(costContent({})).error,
    /must report something/,
  );
  assert.match(
    decodePulseEntry(costContent({ seats: [{}] })).error,
    /seat must report something/,
  );
  assert.equal(decodePulseEntry(costContent({ costUsd: 1.5 })).ok, false);
  assert.equal(
    decodePulseEntry(costContent({ seats: [{ actor: BUILDER, spend: 1 }] })).ok,
    false,
  );
});

test("a total that does not equal the seats it lists is rejected", () => {
  const decoded = decodePulseEntry(
    costContent({
      seats: [{ actor: BUILDER, inputTokens: 10, outputTokens: 5 }],
      totalTokens: 900,
    }),
  );
  assert.equal(decoded.ok, false);
  assert.match(decoded.error, /totalTokens 900 does not equal/);
});

test("a repeated seat and a malformed actor are rejected", () => {
  assert.match(
    decodePulseEntry(
      costContent({
        seats: [
          { actor: BUILDER, turns: 1 },
          { actor: BUILDER, turns: 1 },
        ],
      }),
    ).error,
    /repeats cost seat/,
  );
  assert.match(
    decodePulseEntry(
      costContent({ seats: [{ actor: BUILDER.toUpperCase(), turns: 1 }] }),
    ).error,
    /64-character lowercase hex/,
  );
});

test("too many seats and blank labels are rejected", () => {
  const seats = Array.from({ length: MAX_PULSE_COST_SEATS + 1 }, (_, i) => ({
    role: `r${i}`,
    turns: 1,
  }));
  assert.match(decodePulseEntry(costContent({ seats })).error, /more than/);
  assert.match(
    decodePulseEntry(costContent({ seats: [{ role: "  ", turns: 1 }] })).error,
    /role/,
  );
  assert.match(
    decodePulseEntry(costContent({ seats: [{ model: "", turns: 1 }] })).error,
    /model/,
  );
});

test("a cost survives the signed envelope validator", () => {
  const decoded = validatePulseEntryEnvelope(
    event({
      content: costContent({
        seats: [
          { actor: BUILDER, turns: 2 },
          { actor: REFUTER, turns: 1 },
        ],
      }),
    }),
  );
  assert.equal(decoded.ok, true);
  assert.equal(decoded.entry.cost.seats.length, 2);
});
