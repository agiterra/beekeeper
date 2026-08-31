/**
 * The standing hiring policy, and the five refusals it can produce.
 *
 * Every refusal here is a sentence a lead reads over the relay, so each code
 * is pinned to the fact that produced it: a policy switch, a role list, a seat
 * ceiling, a provider list, or this computer's own inventory of identities.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_HIRE_MAX_SEATS_CEILING,
  DEFAULT_CODING_SESSION_HIRE_POLICY,
  codingSessionHireAllowedRoles,
  decideCodingSessionHire,
  formatCodingSessionHireRefusal,
  parseCodingSessionHirePolicy,
  parseCodingSessionHireMaxSeatsInput,
  serializeCodingSessionHirePolicy,
} from "./codingSessionHirePolicy.ts";

const ADA = "a".repeat(64);
const BEN = "b".repeat(64);
const CAI = "c".repeat(64);

const CANDIDATES = [
  {
    pubkey: ADA,
    name: "Ada",
    homeRole: "builder",
    hasRolePack: true,
    model: "sonnet",
  },
  {
    pubkey: BEN,
    name: "Ben",
    homeRole: "builder",
    hasRolePack: true,
    model: null,
  },
  {
    pubkey: CAI,
    name: "Cai",
    homeRole: "architect",
    hasRolePack: true,
    model: null,
  },
];

function decide(overrides = {}) {
  return decideCodingSessionHire({
    request: { role: "builder", providerInstanceRef: null, model: null },
    policy: DEFAULT_CODING_SESSION_HIRE_POLICY,
    candidates: CANDIDATES,
    liveSeats: [],
    availableProviderInstanceRefs: ["claude-primary", "codex-primary"],
    ...overrides,
  });
}

test("the default policy hires a builder onto the host's first provider", () => {
  const decision = decide();
  assert.equal(decision.ok, true);
  assert.equal(decision.identity.pubkey, ADA);
  assert.equal(decision.role, "builder");
  assert.equal(decision.providerInstanceRef, "claude-primary");
  assert.equal(decision.model, "sonnet");
});

test("HIRE_OFF: hiring switched off refuses before anything else is read", () => {
  const decision = decide({
    policy: { ...DEFAULT_CODING_SESSION_HIRE_POLICY, enabled: false },
    candidates: [],
    availableProviderInstanceRefs: [],
  });
  assert.equal(decision.ok, false);
  assert.equal(decision.code, "HIRE_OFF");
  assert.equal(
    formatCodingSessionHireRefusal(decision),
    `hire refused: HIRE_OFF — ${decision.reason}`,
  );
});

test("HIRE_ROLE_NOT_ALLOWED: a role the operator did not allow", () => {
  const decision = decide({
    request: { role: "architect", providerInstanceRef: null, model: null },
    policy: {
      ...DEFAULT_CODING_SESSION_HIRE_POLICY,
      allowedRoles: ["builder"],
    },
  });
  assert.equal(decision.code, "HIRE_ROLE_NOT_ALLOWED");
  assert.match(decision.reason, /architect/);
});

test("the default role list is every role with an installed pack", () => {
  assert.deepEqual(
    codingSessionHireAllowedRoles(
      DEFAULT_CODING_SESSION_HIRE_POLICY,
      CANDIDATES,
    ),
    ["architect", "builder"],
  );
  // A role whose only identity has no pack on this computer is not a role
  // this computer can honestly offer.
  assert.deepEqual(
    codingSessionHireAllowedRoles(DEFAULT_CODING_SESSION_HIRE_POLICY, [
      { pubkey: ADA, name: "Ada", homeRole: "runner", hasRolePack: false },
    ]),
    [],
  );
  // `undefined` is "nobody asked", not "no pack": it must not silently
  // subtract a role.
  assert.deepEqual(
    codingSessionHireAllowedRoles(DEFAULT_CODING_SESSION_HIRE_POLICY, [
      { pubkey: ADA, name: "Ada", homeRole: "runner" },
    ]),
    ["runner"],
  );
});

test("HIRE_LIMIT: the umbrella is already at the seat ceiling", () => {
  const decision = decide({
    policy: { ...DEFAULT_CODING_SESSION_HIRE_POLICY, maxSeatsPerUmbrella: 2 },
    liveSeats: [
      { actor: CAI, role: "architect" },
      { actor: "d".repeat(64), role: "lead" },
    ],
  });
  assert.equal(decision.code, "HIRE_LIMIT");
  assert.match(decision.reason, /2/);
});

test("HIRE_PROVIDER_NOT_ALLOWED: a runtime the policy or the host does not offer", () => {
  const byPolicy = decide({
    request: {
      role: "builder",
      providerInstanceRef: "codex-primary",
      model: null,
    },
    policy: {
      ...DEFAULT_CODING_SESSION_HIRE_POLICY,
      allowedProviderInstanceRefs: ["claude-primary"],
    },
  });
  assert.equal(byPolicy.code, "HIRE_PROVIDER_NOT_ALLOWED");
  const byHost = decide({
    request: {
      role: "builder",
      providerInstanceRef: "gemini-primary",
      model: null,
    },
  });
  assert.equal(byHost.code, "HIRE_PROVIDER_NOT_ALLOWED");
  const none = decide({ availableProviderInstanceRefs: [] });
  assert.equal(none.code, "HIRE_PROVIDER_NOT_ALLOWED");
});

test("HIRE_NO_IDENTITY: nobody on this computer is that role, and the remedy is named", () => {
  const decision = decide({
    request: { role: "verifier", providerInstanceRef: null, model: null },
    policy: {
      ...DEFAULT_CODING_SESSION_HIRE_POLICY,
      allowedRoles: ["verifier"],
    },
  });
  assert.equal(decision.code, "HIRE_NO_IDENTITY");
  assert.match(decision.reason, /[Ii]nstall team roles/);
});

test("an identity already live in this umbrella is skipped, not re-seated", () => {
  const decision = decide({ liveSeats: [{ actor: ADA, role: "builder" }] });
  assert.equal(decision.ok, true);
  assert.equal(decision.identity.pubkey, BEN);
});

test("every builder already live is HIRE_ROLE_BUSY, not a duplicate seat", () => {
  const decision = decide({
    liveSeats: [
      { actor: ADA, role: "builder" },
      { actor: BEN, role: "builder" },
    ],
  });
  assert.equal(decision.code, "HIRE_ROLE_BUSY");
});

test("the request's provider and model win over the defaults", () => {
  const decision = decide({
    request: {
      role: "builder",
      providerInstanceRef: "codex-primary",
      model: "gpt-5.6-sol",
    },
  });
  assert.equal(decision.ok, true);
  assert.equal(decision.providerInstanceRef, "codex-primary");
  assert.equal(decision.model, "gpt-5.6-sol");
});

test("an identity with a role pack is preferred over one without", () => {
  const decision = decide({
    candidates: [
      { pubkey: BEN, name: "Ben", homeRole: "builder", hasRolePack: false },
      { pubkey: ADA, name: "Ada", homeRole: "builder", hasRolePack: true },
    ],
  });
  assert.equal(decision.identity.pubkey, ADA);
});

test("the stored policy round-trips, and a corrupt one is the default", () => {
  const policy = {
    enabled: false,
    allowedRoles: ["builder", "runner"],
    maxSeatsPerUmbrella: 6,
    allowedProviderInstanceRefs: ["claude-primary"],
  };
  assert.deepEqual(
    parseCodingSessionHirePolicy(serializeCodingSessionHirePolicy(policy)),
    policy,
  );
  for (const raw of [null, "", "{", "[]", '{"enabled":"yes"}']) {
    assert.deepEqual(
      parseCodingSessionHirePolicy(raw),
      DEFAULT_CODING_SESSION_HIRE_POLICY,
    );
  }
});

test("the seat ceiling field keeps the previous value rather than becoming zero", () => {
  assert.equal(parseCodingSessionHireMaxSeatsInput("", 4), 4);
  assert.equal(parseCodingSessionHireMaxSeatsInput("0", 4), 4);
  assert.equal(parseCodingSessionHireMaxSeatsInput("banana", 4), 4);
  assert.equal(parseCodingSessionHireMaxSeatsInput("7", 4), 7);
  assert.equal(
    parseCodingSessionHireMaxSeatsInput("9999", 4),
    CODING_SESSION_HIRE_MAX_SEATS_CEILING,
  );
});

test("the shipped default is on, every installed role, ten seats, all providers", () => {
  assert.deepEqual(DEFAULT_CODING_SESSION_HIRE_POLICY, {
    enabled: true,
    allowedRoles: null,
    maxSeatsPerUmbrella: 10,
    allowedProviderInstanceRefs: null,
  });
});

test("a model the chosen runtime's catalog offers is seated exactly as asked", () => {
  const result = decide({
    request: {
      role: "builder",
      providerInstanceRef: "claude-primary",
      model: "opus[1m]",
    },
    modelCatalogs: new Map([
      ["claude-primary", ["default", "haiku", "opus[1m]", "sonnet"]],
    ]),
  });
  assert.equal(result.ok, true);
  assert.equal(result.model, "opus[1m]");
  assert.equal(result.modelNotice, null);
});

// Brian's ruling, 2026-08-29: the catalog is the only model list, so a vendor
// family name the catalog does not publish is refused rather than matched onto
// one it does. This case used to seat `sonnet` and disclose the swap.
test("a Claude vendor alias the catalog does not publish is refused, not matched", () => {
  const result = decide({
    request: {
      role: "builder",
      providerInstanceRef: "claude-primary",
      model: "claude-sonnet-5",
    },
    modelCatalogs: new Map([
      ["claude-primary", ["default", "haiku", "opus[1m]", "sonnet"]],
    ]),
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, "HIRE_MODEL_NOT_OFFERED");
  assert.match(result.reason, /claude-sonnet-5/);
  assert.match(result.reason, /default, haiku, opus\[1m\], sonnet/);
});

test("a model the catalog does not offer is refused with the offered ids", () => {
  const result = decide({
    request: {
      role: "builder",
      providerInstanceRef: "claude-primary",
      model: "claude-sonnet-5",
    },
    // The live 2026-08-28 catalog, minus the alias — nothing to translate onto.
    modelCatalogs: new Map([
      ["claude-primary", ["default", "claude-fable-5[1m]"]],
    ]),
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, "HIRE_MODEL_NOT_OFFERED");
  assert.match(result.reason, /claude-sonnet-5/);
  assert.match(result.reason, /default, claude-fable-5\[1m\]/);
});

test("a catalog this host never read refuses nothing", () => {
  const result = decide({
    request: {
      role: "builder",
      providerInstanceRef: "claude-primary",
      model: "claude-sonnet-5",
    },
    modelCatalogs: new Map(),
  });
  assert.equal(result.ok, true);
  assert.equal(result.model, "claude-sonnet-5");
  assert.equal(result.modelNotice, null);
});

test("the identity's own model is taken when the hire names none", () => {
  const result = decide({
    request: {
      role: "builder",
      providerInstanceRef: "claude-primary",
      model: null,
    },
    modelCatalogs: new Map([["claude-primary", ["default", "sonnet"]]]),
  });
  assert.equal(result.ok, true);
  assert.equal(result.model, "sonnet");
  assert.equal(result.modelNotice, null);
});

// --- item 88(a),(b),(h),(i): what the live DogFood2 loop proved wrong -------

test("the identity's own model is checked against the same catalog the hire's is", () => {
  // Live 2026-08-28: a hire naming no model fell back to `opus[1m]` — a model
  // the host's own refusal had just claimed was not offered — because only
  // `request.model` ever reached the catalog check.
  const result = decide({
    request: {
      role: "builder",
      providerInstanceRef: "claude-primary",
      model: null,
    },
    candidates: [
      {
        pubkey: ADA,
        name: "Ada",
        homeRole: "builder",
        hasRolePack: true,
        model: "opus[1m]",
      },
    ],
    modelCatalogs: new Map([["claude-primary", ["default", "sonnet"]]]),
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, "HIRE_MODEL_NOT_OFFERED");
  // Named, so the operator knows which record to fix.
  assert.match(result.reason, /Ada/);
  assert.match(result.reason, /opus\[1m\]/);
  assert.match(result.reason, /default, sonnet/);
});

test("an identity model the catalog does offer is still seated untouched", () => {
  const result = decide({
    request: {
      role: "builder",
      providerInstanceRef: "claude-primary",
      model: null,
    },
    modelCatalogs: new Map([["claude-primary", ["default", "sonnet"]]]),
  });
  assert.equal(result.ok, true);
  assert.equal(result.model, "sonnet");
  assert.equal(result.modelNotice, null);
});

// Same ruling from the other side: an identity's own record gets no more
// benefit of the doubt than the lead's --model does. This case used to seat
// `opus[1m]` and disclose the swap.
test("an identity's vendor model alias is refused, naming the identity and the id", () => {
  const result = decide({
    request: {
      role: "builder",
      providerInstanceRef: "claude-primary",
      model: null,
    },
    candidates: [
      {
        pubkey: ADA,
        name: "Ada",
        homeRole: "builder",
        hasRolePack: true,
        model: "claude-opus-4-1",
      },
    ],
    modelCatalogs: new Map([["claude-primary", ["default", "opus[1m]"]]]),
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, "HIRE_MODEL_NOT_OFFERED");
  assert.match(result.reason, /Ada's record says claude-opus-4-1/);
  assert.match(result.reason, /default, opus\[1m\]/);
});

test("the seat runs on the identity's own runtime, not the umbrella's", () => {
  // Live 2026-08-28 (item 88(i)): Banksy — a codex identity on gpt-5.6-sol —
  // was seated on driver claude-agent-acp because the host took the
  // umbrella's runtime and passed the identity's model through it.
  const result = decide({
    request: {
      role: "designer",
      providerInstanceRef: "claude-primary",
      model: null,
    },
    policy: {
      ...DEFAULT_CODING_SESSION_HIRE_POLICY,
      allowedRoles: ["designer"],
    },
    candidates: [
      {
        pubkey: ADA,
        name: "Banksy",
        homeRole: "designer",
        hasRolePack: true,
        model: "gpt-5.6-sol",
        runtime: "codex",
      },
    ],
    providerRuntimeSlugs: new Map([
      ["claude-primary", "claude"],
      ["codex-primary", "codex"],
    ]),
    modelCatalogs: new Map([
      ["claude-primary", ["default", "sonnet"]],
      ["codex-primary", ["gpt-5.6-sol"]],
    ]),
  });
  assert.equal(result.ok, true);
  assert.equal(result.providerInstanceRef, "codex-primary");
  assert.equal(result.model, "gpt-5.6-sol");
  // The host overrode the runtime the hire named, so it says so.
  assert.match(result.providerNotice, /codex-primary/);
  assert.match(result.providerNotice, /claude-primary/);
});

test("an identity whose runtime this computer does not run is refused, never re-homed", () => {
  const result = decide({
    request: { role: "designer", providerInstanceRef: null, model: null },
    policy: {
      ...DEFAULT_CODING_SESSION_HIRE_POLICY,
      allowedRoles: ["designer"],
    },
    candidates: [
      {
        pubkey: ADA,
        name: "Banksy",
        homeRole: "designer",
        hasRolePack: true,
        model: "gpt-5.6-sol",
        runtime: "codex",
      },
    ],
    availableProviderInstanceRefs: ["claude-primary"],
    providerRuntimeSlugs: new Map([["claude-primary", "claude"]]),
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, "HIRE_PROVIDER_NOT_ALLOWED");
  assert.match(result.reason, /Banksy/);
  assert.match(result.reason, /codex/);
  assert.match(result.reason, /claude-primary/);
});

test("an identity naming no runtime still takes the hire's, then the host's default", () => {
  const result = decide({
    request: {
      role: "builder",
      providerInstanceRef: "codex-primary",
      model: null,
    },
    candidates: [
      { pubkey: BEN, name: "Ben", homeRole: "builder", hasRolePack: true },
    ],
    providerRuntimeSlugs: new Map([
      ["claude-primary", "claude"],
      ["codex-primary", "codex"],
    ]),
  });
  assert.equal(result.ok, true);
  assert.equal(result.providerInstanceRef, "codex-primary");
  assert.equal(result.providerNotice, null);
});

test("a role whose every identity is seated here is refused as busy, naming the seat", () => {
  // Live 2026-08-28 (item 88(h)): one builder existed and was seated-but-idle,
  // and the host said "or none is installed. Install team roles" — the wrong
  // remedy for the case that actually happened.
  const decision = decide({
    liveSeats: [
      { actor: ADA, role: "builder", generationId: "gen-7" },
      { actor: BEN, role: "builder" },
    ],
  });
  assert.equal(decision.ok, false);
  // Its own code since item 89: a busy role and an absent one are two facts
  // with two remedies, and a lead acts on the code before it reads the prose.
  assert.equal(decision.code, "HIRE_ROLE_BUSY");
  // The seat it should talk to instead, and how.
  assert.match(decision.reason, /already seated/);
  assert.match(decision.reason, /·builder/);
  assert.match(decision.reason, /gen-7/);
  assert.match(decision.reason, /bee sessions send --to builder/);
  // Never the install remedy: the role IS installed.
  assert.equal(/[Ii]nstall team roles/.test(decision.reason), false);
});

test("a role no installed identity holds keeps HIRE_NO_IDENTITY and the install remedy", () => {
  const decision = decide({
    request: { role: "verifier", providerInstanceRef: null, model: null },
    policy: {
      ...DEFAULT_CODING_SESSION_HIRE_POLICY,
      allowedRoles: ["verifier"],
    },
  });
  assert.equal(decision.code, "HIRE_NO_IDENTITY");
  assert.match(decision.reason, /[Ii]nstall team roles/);
  assert.equal(/already seated/.test(decision.reason), false);
});
