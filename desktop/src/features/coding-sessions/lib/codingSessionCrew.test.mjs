import assert from "node:assert/strict";
import test from "node:test";

import {
  checkCodingSessionCrewFamilies,
  checkCodingSessionCrewSeatModels,
  codingSessionCrewFirstTurnText,
  codingSessionCrewLaunchBlock,
  codingSessionCrewRosterText,
  deriveCodingSessionModelVendor,
  describeCodingSessionSeatVendor,
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
  assert.match(roster, /^\[Team\]/);
  assert.match(roster, /- lead: Fable \(anthropic · claude-opus-5\) — you/);
  assert.match(roster, /- builder: Codey \(openai · gpt-5\.6-sol\)$/m);
});

test("the first turn carries the goal and then the roster", () => {
  const text = codingSessionCrewFirstTurnText({
    goal: "  Close ledger item 53.  ",
    seats: SEATS,
    primaryPersonaId: "p-lead",
  });
  assert.ok(text.startsWith("Close ledger item 53.\n\n[Team]"));
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

test("the adapter's `default` alias is a vendor nobody knows, not a vendor", () => {
  // `default` is the alias adapters publish for "you choose" — a runtime
  // without live model discovery publishes exactly allowedModels: ["default"].
  // A seat carrying it declares a vendor for a model the adapter has not
  // picked yet, so the declaration is a hope, not an answer.
  assert.deepEqual(
    resolveCodingSessionSeatVendor({ model: "default", vendor: "openai" }),
    { vendor: null, source: "adapter-default", declared: "openai" },
  );
  assert.deepEqual(resolveCodingSessionSeatVendor({ model: " Default " }), {
    vendor: null,
    source: "adapter-default",
    declared: null,
  });
  // A bracketed alias is the same alias.
  assert.deepEqual(
    resolveCodingSessionSeatVendor({ model: "default[1m]", vendor: "xai" }),
    { vendor: null, source: "adapter-default", declared: "xai" },
  );
});

test("two seats that both say `default` cannot pass the family check", () => {
  // Both run the one adapter's own model, so "openai verifier reviewing an
  // anthropic builder" is two declarations about a model nothing ran.
  const verdict = checkCodingSessionCrewFamilies([
    {
      role: "builder",
      actorLabel: "Codey",
      model: "default",
      vendor: "anthropic",
    },
    {
      role: "verifier",
      actorLabel: "Grokker",
      model: "default",
      vendor: "openai",
    },
  ]);
  assert.equal(verdict.ok, false);
  assert.match(verdict.reason, /builder \(Codey\)/);
  assert.match(verdict.reason, /verifier \(Grokker\)/);
  assert.match(verdict.reason, /default/);
  assert.match(verdict.reason, /teams\.json/);
});

test("the alias defeats the family check even where the catalog offers it", () => {
  // The seat-model check passes — the runtime really does publish `default` —
  // and the family check must still refuse, because passing the first one says
  // nothing about which vendor runs.
  assert.deepEqual(
    checkCodingSessionCrewSeatModels(
      [
        { role: "builder", model: "default", vendor: "anthropic" },
        { role: "verifier", model: "default", vendor: "openai" },
      ],
      { label: "claude-agent-acp", allowedModels: ["default"] },
    ),
    { ok: true },
  );
  assert.equal(
    checkCodingSessionCrewFamilies([
      { role: "builder", model: "default", vendor: "anthropic" },
      { role: "verifier", model: "default", vendor: "openai" },
    ]).ok,
    false,
  );
});

test("the roster says the alias names no vendor rather than the seat's hope", () => {
  const roster = codingSessionCrewRosterText({
    seats: [{ ...SEATS[0], model: "default", vendor: "anthropic" }],
    primaryPersonaId: "p-lead",
  });
  assert.doesNotMatch(roster, /anthropic/);
  assert.match(roster, /unknown/);
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

test("a provider-locked runtime names the seat's vendor whatever the model alias is", () => {
  // F7 (SESSION_STATE item 77): `sonnet` names no vendor on its own, so every
  // seat the installer wrote read "vendor not declared · sonnet" and the D8
  // rule refused the launch. The Claude adapter cannot run anything but
  // Anthropic, so the seat's runtime settles it — including the `default`
  // alias, whose *model* is still the adapter's to pick.
  for (const model of ["sonnet", "opus", "haiku", "fable", "default"]) {
    assert.deepEqual(
      resolveCodingSessionSeatVendor({ driver: "claude-agent-acp", model }),
      { vendor: "anthropic", source: "runtime" },
      `${model} on claude-agent-acp`,
    );
  }
  assert.deepEqual(
    resolveCodingSessionSeatVendor({ driver: "claude-agent-acp[1m]" }),
    { vendor: "anthropic", source: "runtime" },
    "a bracketed driver slug is the same driver",
  );
  assert.deepEqual(
    resolveCodingSessionSeatVendor({ driver: "codex-acp", model: "default" }),
    { vendor: "openai", source: "runtime" },
  );
  // The alias is only unknown when the runtime is.
  assert.deepEqual(resolveCodingSessionSeatVendor({ model: "sonnet" }), {
    vendor: null,
    source: "unknown",
  });
  // Goose runs whatever its config points at, so its slug settles nothing.
  assert.deepEqual(
    resolveCodingSessionSeatVendor({ driver: "goose-acp", model: "sonnet" }),
    { vendor: null, source: "unknown" },
  );
});

test("a seat whose declaration contradicts its runtime is a conflict", () => {
  assert.deepEqual(
    resolveCodingSessionSeatVendor({
      driver: "claude-agent-acp",
      model: "sonnet",
      vendor: "openai",
    }),
    {
      vendor: null,
      source: "conflict",
      declared: "openai",
      derived: "anthropic",
      // Which statement lost, and to what: the copy cannot be honest without
      // knowing the runtime is the half this build can check.
      runtime: "claude-agent-acp",
      via: "declaration",
    },
  );
  // Agreement is not a conflict, and the declaration is what it says it is.
  assert.deepEqual(
    resolveCodingSessionSeatVendor({
      driver: "claude-agent-acp",
      vendor: "anthropic",
    }),
    { vendor: "anthropic", source: "declared" },
  );
});

test("the roster the installer writes passes the family check and can launch", () => {
  // The whole point of F7: what `install_role_packs` writes must launch as
  // installed. `crew_roles.rs` seats lead, architect, builder and runner, each
  // on `claude-agent-acp`, with `anthropic` declared — and seats no verifier,
  // because every seat of one launch runs on the one selected provider.
  const seats = ["lead", "architect", "builder", "runner"].map((role) => ({
    role,
    actorLabel: role,
    driver: "claude-agent-acp",
    vendor: "anthropic",
    model: "sonnet",
  }));
  assert.deepEqual(checkCodingSessionCrewFamilies(seats), { ok: true });
  assert.deepEqual(
    checkCodingSessionCrewSeatModels(seats, {
      allowedModels: ["default", "sonnet", "haiku"],
      instanceRef: "claude-primary",
      label: "Claude Code",
    }),
    { ok: true },
  );
});

test("a seat cannot declare a vendor the selected provider cannot run", () => {
  // The written vendor is only worth anything if it is checked against the
  // runtime that will actually run the seat: every seat is created against the
  // one `providerInstanceRef` the dialog selected.
  const verdict = checkCodingSessionCrewSeatModels(
    [
      {
        role: "builder",
        actorLabel: "Levain",
        driver: "claude-agent-acp",
        vendor: "anthropic",
        model: "default",
      },
    ],
    {
      allowedModels: ["default"],
      instanceRef: "codex-primary",
      label: "Codex",
    },
  );
  assert.equal(verdict.ok, false);
  assert.match(verdict.reason, /anthropic/);
  assert.match(verdict.reason, /openai/);
});

test("a lead seat cannot claim a vendor the selected provider will not run", () => {
  // Not only the verifier/builder pair: the roster prints a vendor for every
  // seat, and a seat created against a Codex provider does not run Anthropic
  // because its seat says so.
  const verdict = checkCodingSessionCrewSeatModels(
    [
      {
        role: "lead",
        actorLabel: "Keystone",
        driver: "claude-agent-acp",
        vendor: "anthropic",
        model: "default",
      },
    ],
    {
      allowedModels: ["default"],
      instanceRef: "codex-primary",
      label: "Codex",
    },
  );
  assert.equal(verdict.ok, false);
  assert.match(verdict.reason, /lead \(Keystone\)/);
});

test("a seat pinned to a Claude runtime says so, instead of calling its OpenAI model anthropic", () => {
  // Item 79(c): the roster read "declared openai, but gpt-5.6-sol is
  // anthropic" — two false claims in one line. The seat declared anthropic
  // (the installer wrote it), and gpt-5.6-sol is not an Anthropic model. What
  // is true is the runtime it is pinned to.
  const seat = {
    role: "architect",
    actorLabel: "Sol",
    driver: "claude-agent-acp",
    model: "gpt-5.6-sol",
    vendor: "anthropic",
  };
  const line = describeCodingSessionSeatVendor(seat);
  assert.match(line, /runs on Claude Code \(anthropic\)/);
  assert.match(line, /gpt-5\.6-sol is an OpenAI model/);
  assert.match(line, /a team launch runs every seat on one provider/);
  assert.doesNotMatch(line, /gpt-5\.6-sol is anthropic/);
  assert.doesNotMatch(line, /declared openai/);
});

test("a seat that declares a vendor its runtime cannot run names the declaration, not the model", () => {
  const line = describeCodingSessionSeatVendor({
    driver: "codex-acp",
    model: "gpt-5.6-sol",
    vendor: "anthropic",
  });
  assert.match(line, /runs on Codex \(openai\)/);
  assert.match(line, /this seat declares anthropic/);
});

test("two statements a seat makes about itself still contradict each other plainly", () => {
  // No runtime pins this seat, so the disagreement really is between the
  // declaration and the model id — and that copy was already true.
  assert.equal(
    describeCodingSessionSeatVendor({
      model: "claude-opus-5",
      vendor: "local",
    }),
    "declared local, but claude-opus-5 is anthropic",
  );
});

test("a launch is never disabled without a sentence saying why", () => {
  const ready = {
    hasTeam: true,
    seatCount: 3,
    hasChannel: true,
    canCreateChannel: false,
    createInFlight: false,
    isLaunching: false,
    goal: "Close ledger item 79.",
  };
  assert.equal(codingSessionCrewLaunchBlock(ready), null);

  assert.match(
    codingSessionCrewLaunchBlock({ ...ready, hasTeam: false, seatCount: 0 }),
    /no team/i,
  );
  assert.match(
    codingSessionCrewLaunchBlock({ ...ready, seatCount: 0 }),
    /no seats/i,
  );
  assert.match(
    codingSessionCrewLaunchBlock({ ...ready, hasChannel: false }),
    /^Pick a channel first/,
  );
  assert.match(
    codingSessionCrewLaunchBlock({ ...ready, goal: "   " }),
    /^Write the goal — the lead's first turn carries it\./,
  );
  assert.match(
    codingSessionCrewLaunchBlock({ ...ready, isLaunching: true }),
    /launching/i,
  );
  assert.match(
    codingSessionCrewLaunchBlock({ ...ready, createInFlight: true }),
    /in flight/i,
  );
});

test("a project whose sessions channel is not published yet is not missing a channel", () => {
  // The channel is a fact the launch will mint; refusing it would be the
  // front door telling the operator to go find something that does not exist.
  assert.equal(
    codingSessionCrewLaunchBlock({
      hasTeam: true,
      seatCount: 3,
      hasChannel: false,
      canCreateChannel: true,
      createInFlight: false,
      isLaunching: false,
      goal: "Close ledger item 79.",
    }),
    null,
  );
});

test("a refusal about a seat pinned to the wrong runtime does not call it a model-id fight", () => {
  const verdict = checkCodingSessionCrewFamilies([
    {
      role: "builder",
      actorLabel: "Codey",
      driver: "claude-agent-acp",
      model: "gpt-5.6-sol",
      vendor: "anthropic",
    },
    { role: "verifier", actorLabel: "Quinn", model: "grok-4" },
  ]);
  assert.equal(verdict.ok, false);
  assert.doesNotMatch(
    verdict.reason,
    /declared vendor contradicts its model id/,
  );
  assert.match(verdict.reason, /runs on Claude Code \(anthropic\)/);
  assert.match(verdict.reason, /gpt-5\.6-sol is an OpenAI model/);
});
