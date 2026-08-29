import assert from "node:assert/strict";
import test from "node:test";

import { hostOwnedModelProviderSubmission } from "./hostOwnedModelProviderSubmission.ts";

// Item 90: model and provider are HOST-owned identity facts — what this
// computer runs the identity on. The edit dialog used to omit both whenever the
// agent was linked to a definition, so the control rendered, saved, and did
// nothing for every team identity.

test("a linked identity submits the model the host picked", () => {
  const submission = hostOwnedModelProviderSubmission({
    linked: true,
    model: "gpt-5.6-sol",
    provider: "openai",
    agentModel: null,
    agentProvider: null,
    providerRuntimeCapability: "capable",
  });
  assert.equal(submission.model, "gpt-5.6-sol");
  assert.equal(submission.provider, "openai");
});

test("a linked identity submits an explicit clear", () => {
  const submission = hostOwnedModelProviderSubmission({
    linked: true,
    model: null,
    provider: null,
    agentModel: "gpt-5.6-sol",
    agentProvider: "openai",
    providerRuntimeCapability: "capable",
  });
  assert.equal(submission.model, null);
  assert.equal(submission.provider, null);
});

test("an unchanged model or provider is omitted from the patch", () => {
  const submission = hostOwnedModelProviderSubmission({
    linked: true,
    model: "gpt-5.6-sol",
    provider: "openai",
    agentModel: "gpt-5.6-sol",
    agentProvider: "openai",
    providerRuntimeCapability: "capable",
  });
  assert.equal(submission.model, undefined);
  assert.equal(submission.provider, undefined);
});

test("a provider-locked runtime clears a set provider and omits an unset one", () => {
  assert.equal(
    hostOwnedModelProviderSubmission({
      linked: true,
      model: null,
      provider: "openai",
      agentModel: null,
      agentProvider: "openai",
      providerRuntimeCapability: "locked",
    }).provider,
    null,
  );
  assert.equal(
    hostOwnedModelProviderSubmission({
      linked: true,
      model: null,
      provider: null,
      agentModel: null,
      agentProvider: null,
      providerRuntimeCapability: "locked",
    }).provider,
    undefined,
  );
});

test("an unknown provider capability never writes a provider", () => {
  const submission = hostOwnedModelProviderSubmission({
    linked: false,
    model: "gpt-5.6-sol",
    provider: "openai",
    agentModel: null,
    agentProvider: "anthropic",
    providerRuntimeCapability: "unknown",
  });
  assert.equal(submission.provider, undefined);
  assert.equal(submission.model, "gpt-5.6-sol");
});
