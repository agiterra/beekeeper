import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionCredentialLabelLooksLikeSecret,
  codingSessionRuntimeName,
  describeCodingSessionCapabilities,
  describeCodingSessionCredentialSource,
  listCodingSessionCapabilities,
} from "./codingSessionExecutionFacts.ts";

const SIGNER = "ab".repeat(32);
const TARGET = "coding-session/v1|claude-agent-acp|inst|sess|1";
const entry = (overrides = {}) => ({
  signerPubkey: SIGNER,
  targetKey: TARGET,
  metadata: {},
  ...overrides,
});

test("a 44223 that names a credential source shows the label and only the label", () => {
  const described = describeCodingSessionCredentialSource(
    [entry({ metadata: { credentialSource: "acme-vault" } })],
    { signerPubkey: SIGNER.toUpperCase(), targetKey: TARGET },
  );
  assert.equal(described.label, "acme-vault");
  assert.equal(described.known, true);
  assert.match(described.title, /never the credential/);
});

test("a 44223 with no credential source is a disclosed non-answer, never 'ambient login'", () => {
  const described = describeCodingSessionCredentialSource([entry()], {
    signerPubkey: SIGNER,
    targetKey: TARGET,
  });
  assert.equal(described.label, "credential source not reported");
  assert.match(described.title, /no credentialSource label/);
  assert.doesNotMatch(described.title, /ambient|own login/i);
  assert.equal(described.known, true);
});

test("no 44223 for this target is not reported and not known", () => {
  for (const target of [
    null,
    { signerPubkey: SIGNER, targetKey: "coding-session/v1|other" },
    { signerPubkey: "cd".repeat(32), targetKey: TARGET },
  ]) {
    const described = describeCodingSessionCredentialSource(
      [entry({ metadata: { credentialSource: "acme-vault" } })],
      target,
    );
    assert.equal(described.label, "not reported");
    assert.equal(described.known, false);
  }
});

test("the newest entry wins, and a path-, URL- or key-shaped label is withheld", () => {
  const newestFirst = [
    entry({ metadata: { credentialSource: "team-b" } }),
    entry({ metadata: { credentialSource: "team-a" } }),
  ];
  assert.equal(
    describeCodingSessionCredentialSource(newestFirst, {
      signerPubkey: SIGNER,
      targetKey: TARGET,
    }).label,
    "team-b",
  );
  for (const label of [
    "/Users/me/.config/creds.json",
    "~/.aws/credentials",
    "C:\\secrets\\key",
    "https://vault.example/token",
    "deadbeef".repeat(5),
    "sk-abc123",
    "token_xyz",
  ]) {
    assert.equal(
      codingSessionCredentialLabelLooksLikeSecret(label),
      true,
      label,
    );
    const described = describeCodingSessionCredentialSource(
      [entry({ metadata: { credentialSource: label } })],
      { signerPubkey: SIGNER, targetKey: TARGET },
    );
    assert.equal(described.label, "named source (label withheld)");
    assert.doesNotMatch(described.title, new RegExp(label.slice(0, 6)));
  }
  for (const label of ["acme-vault", "Andy's login", "ci-bot"]) {
    assert.equal(codingSessionCredentialLabelLooksLikeSecret(label), false);
  }
});

test("a descriptor-declared runtime renders by its name with its driver, never as unknown", () => {
  const plugged = codingSessionRuntimeName({
    runtime: "my-harness",
    provider: null,
    commandTarget: { driver: "acp-stdio" },
  });
  assert.deepEqual(plugged, {
    label: "My Harness",
    driver: "acp-stdio",
    unreported: false,
  });
  // The driver repeats the name for the bundled runtimes and is dropped.
  assert.deepEqual(
    codingSessionRuntimeName({
      runtime: "claude",
      provider: null,
      commandTarget: { driver: "claude-agent-acp" },
    }),
    { label: "Claude", driver: null, unreported: false },
  );
  assert.deepEqual(
    codingSessionRuntimeName({
      runtime: "codex",
      provider: "codex-local",
      commandTarget: { driver: "codex-acp" },
    }),
    { label: "Codex", driver: null, unreported: false },
  );
});

test("with no runtime word, the driver names it; with nothing, the card says so", () => {
  assert.deepEqual(
    codingSessionRuntimeName({
      runtime: null,
      provider: null,
      commandTarget: { driver: "goose" },
    }),
    { label: "Goose", driver: null, unreported: false },
  );
  assert.deepEqual(
    codingSessionRuntimeName({
      runtime: null,
      provider: null,
      commandTarget: null,
    }),
    { label: "Runtime not reported", driver: null, unreported: true },
  );
  assert.doesNotMatch(
    codingSessionRuntimeName({
      runtime: "  ",
      provider: "",
      commandTarget: null,
    }).label,
    /unknown/i,
  );
});

test("capabilities read yes/no for the required six and unknown for the optional two when unsaid", () => {
  const facts = listCodingSessionCapabilities({
    threadTurnStart: true,
    threadTurnInterrupt: false,
    threadSteer: true,
    context: true,
    diff: false,
    plan: false,
  });
  assert.deepEqual(
    facts.map((fact) => [fact.label, fact.word]),
    [
      ["Turns", "yes"],
      ["Steer", "yes"],
      ["Interrupt", "no"],
      ["Context", "yes"],
      ["Diff", "no"],
      ["Plan", "no"],
      ["Images", "unknown"],
      ["Model switch", "unknown"],
    ],
  );
  const declared = describeCodingSessionCapabilities({
    threadTurnStart: true,
    threadTurnInterrupt: true,
    threadSteer: false,
    context: true,
    diff: true,
    plan: true,
    promptImage: false,
    modelSwitch: true,
  });
  assert.equal(
    declared,
    "Turns yes · Steer no · Interrupt yes · Context yes · Diff yes · Plan yes · Images no · Model switch yes",
  );
  assert.equal(describeCodingSessionCapabilities(null), "Not declared");
  assert.equal(describeCodingSessionCapabilities(undefined), "Not declared");
});
