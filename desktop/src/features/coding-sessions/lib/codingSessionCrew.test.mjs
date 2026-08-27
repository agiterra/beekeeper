import assert from "node:assert/strict";
import test from "node:test";

import {
  checkCodingSessionCrewFamilies,
  checkCodingSessionCrewSeatModels,
  codingSessionCrewFirstTurnText,
  codingSessionCrewRosterText,
  deriveCodingSessionModelVendor,
  parseCodingSessionCrew,
  resolveCodingSessionSeatVendor,
} from "./codingSessionCrew.ts";

test("the derivation table maps exactly the ids that name a vendor", () => {
  assert.equal(deriveCodingSessionModelVendor("claude-opus-5"), "anthropic");
  assert.equal(deriveCodingSessionModelVendor("gpt-5.6-sol"), "openai");
  assert.equal(deriveCodingSessionModelVendor("o3-mini"), "openai");
  assert.equal(deriveCodingSessionModelVendor("o1"), "openai");
  assert.equal(deriveCodingSessionModelVendor("grok-4"), "xai");
  assert.equal(deriveCodingSessionModelVendor("gemini-3-pro"), "google");
  assert.equal(deriveCodingSessionModelVendor("llama-4-70b"), "meta");
  // Case and surrounding space are typing, not meaning.
  assert.equal(deriveCodingSessionModelVendor("  Claude-Opus-5 "), "anthropic");
});

test("an id that does not name a vendor derives nothing, never a guess", () => {
  assert.equal(deriveCodingSessionModelVendor("qwen3-coder"), null);
  assert.equal(deriveCodingSessionModelVendor("openrouter/auto"), null);
  assert.equal(deriveCodingSessionModelVendor(""), null);
  assert.equal(deriveCodingSessionModelVendor(null), null);
  // `o*` is the OpenAI reasoning series, not every id starting with o.
  assert.equal(deriveCodingSessionModelVendor("olmo-2"), null);
});

test("a declared vendor wins only where the model id names nothing", () => {
  assert.deepEqual(
    resolveCodingSessionSeatVendor({ model: "qwen3-coder", vendor: "local" }),
    { vendor: "local", source: "declared" },
  );
  assert.deepEqual(resolveCodingSessionSeatVendor({ vendor: "local" }), {
    vendor: "local",
    source: "declared",
  });
  assert.deepEqual(resolveCodingSessionSeatVendor({ model: "claude-opus-5" }), {
    vendor: "anthropic",
    source: "derived",
  });
  assert.deepEqual(resolveCodingSessionSeatVendor({ model: "mystery-1" }), {
    vendor: null,
    source: "unknown",
  });
});

test("a declaration the model id contradicts is a conflict, never an answer", () => {
  assert.deepEqual(
    resolveCodingSessionSeatVendor({ model: "claude-opus-5", vendor: "local" }),
    {
      vendor: null,
      source: "conflict",
      declared: "local",
      derived: "anthropic",
    },
  );
  // Agreeing with the table, in any casing, is not a conflict.
  assert.deepEqual(
    resolveCodingSessionSeatVendor({
      model: "claude-opus-5",
      vendor: "Anthropic",
    }),
    { vendor: "anthropic", source: "declared" },
  );
});

test("a seat whose declaration its model contradicts cannot be launched", () => {
  const verdict = checkCodingSessionCrewFamilies([
    { role: "builder", model: "gpt-5.6-sol" },
    {
      role: "verifier",
      model: "claude-opus-5",
      vendor: "openai",
      actorLabel: "Quinn",
    },
  ]);
  assert.equal(verdict.ok, false);
  assert.match(verdict.reason, /Quinn/);
  assert.match(verdict.reason, /openai/);
  assert.match(verdict.reason, /anthropic/);
});

test("family is the model vendor, not the ACP runtime", () => {
  // Two Goose seats on different vendors are two families.
  assert.deepEqual(
    checkCodingSessionCrewFamilies([
      { role: "builder", model: "gpt-5.6-sol", vendor: "openai" },
      { role: "verifier", model: "claude-opus-5", vendor: "anthropic" },
    ]),
    { ok: true },
  );
  // Claude Code and Goose-on-Anthropic are one family, and refused.
  const clash = checkCodingSessionCrewFamilies([
    { role: "builder", model: "claude-opus-5" },
    { role: "verifier", model: "claude-sonnet-5", vendor: "anthropic" },
  ]);
  assert.equal(clash.ok, false);
  assert.match(clash.reason, /anthropic/);
});

test("an unknown vendor on a verifier or builder is refused, not assumed", () => {
  const verifier = checkCodingSessionCrewFamilies([
    { role: "builder", model: "gpt-5.6-sol" },
    { role: "verifier", model: "qwen3-coder", actorLabel: "Quinn" },
  ]);
  assert.equal(verifier.ok, false);
  assert.match(verifier.reason, /Declare the model vendor/);
  assert.match(verifier.reason, /Quinn/);

  const builder = checkCodingSessionCrewFamilies([
    { role: "builder", model: "qwen3-coder" },
    { role: "verifier", model: "claude-opus-5" },
  ]);
  assert.equal(builder.ok, false);
  assert.match(builder.reason, /Declare the model vendor/);
});

test("a crew with no verifier/builder pair has no family rule to break", () => {
  assert.deepEqual(
    checkCodingSessionCrewFamilies([
      { role: "lead", model: "mystery-1" },
      { role: "runner", model: "mystery-2" },
    ]),
    { ok: true },
  );
  assert.deepEqual(
    checkCodingSessionCrewFamilies([
      { role: "builder", model: "mystery-1" },
      { role: "lead", model: "mystery-2" },
    ]),
    { ok: true },
  );
});

const SEATS = [
  {
    personaId: "p-lead",
    role: "lead",
    actor: "a".repeat(64),
    actorLabel: "Fable",
    model: "claude-opus-5",
    vendor: null,
  },
  {
    personaId: "p-build",
    role: "builder",
    actor: "b".repeat(64),
    actorLabel: "Codey",
    model: "gpt-5.6-sol",
    vendor: null,
  },
];

test("the roster names every seat, its vendor, and which one is you", () => {
  const roster = codingSessionCrewRosterText({
    seats: SEATS,
    primaryPersonaId: "p-lead",
  });
  assert.match(roster, /^\[Crew\]/);
  assert.match(roster, /- lead: Fable \(anthropic · claude-opus-5\) — you/);
  assert.match(roster, /- builder: Codey \(openai · gpt-5\.6-sol\)$/m);
});

test("the first turn carries the goal and then the roster", () => {
  const text = codingSessionCrewFirstTurnText({
    goal: "  Close ledger item 53.  ",
    seats: SEATS,
    primaryPersonaId: "p-lead",
  });
  assert.ok(text.startsWith("Close ledger item 53.\n\n[Crew]"));
  assert.match(text, /Codey/);
});

test("a malformed crew block is no crew at all", () => {
  assert.equal(parseCodingSessionCrew(null), null);
  assert.equal(parseCodingSessionCrew({ seats: [] }), null);
  assert.equal(
    parseCodingSessionCrew({ primary: "p1", seats: [{ role: "lead" }] }),
    null,
  );
  // A primary that names no seat is a crew nothing can be addressed to.
  assert.equal(
    parseCodingSessionCrew({
      primary: "p9",
      seats: [{ personaId: "p1", role: "lead" }],
    }),
    null,
  );
});

test("a well-formed crew keeps seat order and drops unknown keys", () => {
  const crew = parseCodingSessionCrew({
    primary: "p1",
    seats: [
      { personaId: "p1", role: "lead", model: "claude-opus-5", extra: 1 },
      {
        personaId: "p2",
        role: "builder",
        vendor: "openai",
        driver: "codex-acp",
      },
    ],
  });
  assert.deepEqual(crew, {
    primary: "p1",
    seats: [
      { personaId: "p1", role: "lead", model: "claude-opus-5" },
      {
        personaId: "p2",
        role: "builder",
        driver: "codex-acp",
        vendor: "openai",
      },
    ],
  });
});

test("a refusal names where a crew is edited, because this build has no editor", () => {
  const clash = checkCodingSessionCrewFamilies([
    { role: "builder", model: "claude-opus-5" },
    { role: "verifier", model: "claude-sonnet-5" },
  ]);
  assert.equal(clash.ok, false);
  // "change one seat's model" instructed an action this build offers no path
  // to: crew mode never reaches the model picker and `create_team` writes no
  // crew block. Say where a crew actually lives instead.
  assert.doesNotMatch(clash.reason, /launch again/);
  assert.match(clash.reason, /teams\.json/);

  const undeclared = checkCodingSessionCrewFamilies([
    { role: "builder", model: "qwen3-coder" },
    { role: "verifier", model: "mystery-2" },
  ]);
  assert.equal(undeclared.ok, false);
  assert.match(undeclared.reason, /teams\.json/);
});

test("the roster never prints a declared vendor its own table contradicts", () => {
  const roster = codingSessionCrewRosterText({
    seats: [{ ...SEATS[0], vendor: "local" }],
    primaryPersonaId: "p-lead",
  });
  assert.match(roster, /declared local, but claude-opus-5 is anthropic/);
  assert.doesNotMatch(roster, /\(local ·/);
});

const BUILDER_SEAT = {
  role: "builder",
  actorLabel: "Codey",
  model: "claude-opus-5",
  vendor: "anthropic",
};
const VERIFIER_SEAT = {
  role: "verifier",
  actorLabel: "Grokker",
  model: "gpt-5.6-sol",
  vendor: "openai",
};

test("a seat's model must be one the selected provider actually offers", () => {
  // The whole point: on a Claude-only runtime the openai verifier passes the
  // vendor check and then runs on Anthropic, because the provider's
  // apply_model swaps an unknown model for its default without failing.
  const verdict = checkCodingSessionCrewSeatModels(
    [BUILDER_SEAT, VERIFIER_SEAT],
    { label: "claude-agent-acp", allowedModels: ["claude-opus-5[1m]"] },
  );
  assert.equal(verdict.ok, false);
  assert.match(verdict.reason, /gpt-5\.6-sol/);
  assert.match(verdict.reason, /verifier \(Grokker\)/);
  assert.match(verdict.reason, /claude-agent-acp/);
});

test("a bracketed catalog id offers the base model it names", () => {
  assert.deepEqual(
    checkCodingSessionCrewSeatModels([BUILDER_SEAT, VERIFIER_SEAT], {
      label: "goose",
      allowedModels: ["claude-opus-5[1m]", "gpt-5.6-sol[high]"],
    }),
    { ok: true },
  );
});

test("a seat with no model cannot be vendor-checked against a runtime", () => {
  const verdict = checkCodingSessionCrewSeatModels(
    [{ ...BUILDER_SEAT, model: null }, VERIFIER_SEAT],
    { label: "goose", allowedModels: ["claude-opus-5", "gpt-5.6-sol"] },
  );
  assert.equal(verdict.ok, false);
  assert.match(verdict.reason, /builder \(Codey\) — no model/);
});

test("an unknown catalog is refused, never treated as a passing check", () => {
  const verdict = checkCodingSessionCrewSeatModels(
    [BUILDER_SEAT, VERIFIER_SEAT],
    { label: null, allowedModels: [] },
  );
  assert.equal(verdict.ok, false);
  assert.match(verdict.reason, /cannot see which models/);
});

test("only the seats the vendor rule decides are checked", () => {
  // A crew with no verifier has no family separation to violate, so this
  // check has no opinion about its models either.
  assert.deepEqual(
    checkCodingSessionCrewSeatModels(
      [{ role: "lead", model: "some-local-weight" }],
      { label: "goose", allowedModels: ["claude-opus-5"] },
    ),
    { ok: true },
  );
  // …but a builder in the same crew is checked, verifier or not.
  assert.equal(
    checkCodingSessionCrewSeatModels(
      [{ role: "lead", model: "x" }, BUILDER_SEAT],
      { label: "goose", allowedModels: ["gpt-5.6-sol"] },
    ).ok,
    false,
  );
});
