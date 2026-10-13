import assert from "node:assert/strict";
import test from "node:test";
import { applyMockManagedAgentUpdate } from "./e2eBridgeManagedAgentUpdates.ts";
function fixture() {
  const persona = {
    id: "owned",
    display_name: "Original",
    avatar_url: null,
    system_prompt: "old",
    runtime: "codex",
    model: null,
    provider: null,
    is_builtin: false,
    shared: false,
    updated_at: "observed",
    env_vars: { SECRET: "retained" },
  };
  const agent = {
    pubkey: "actor",
    persona_id: "owned",
    backend: { type: "local" },
    name: "Original",
    model: null,
    system_prompt: "old",
    respond_to: "owner-only",
    respond_to_allowlist: [],
    updated_at: "observed",
  };
  const input = {
    pubkey: "actor",
    name: "New",
    ownedPersonaUpdate: {
      expectedUpdatedAt: "observed",
      definition: {
        id: "owned",
        displayName: "New",
        systemPrompt: "  private new prompt  ",
        runtime: "codex",
      },
    },
  };
  return { persona, agent, input };
}
test("owned profile commits both records and preserves exact private prompt; exact retry succeeds", () => {
  const { persona, agent, input } = fixture();
  applyMockManagedAgentUpdate(agent, input, [persona], [agent]);
  assert.equal(persona.display_name, "New");
  assert.equal(agent.name, "New");
  assert.equal(agent.system_prompt, "  private new prompt  ");
  assert.equal(persona.system_prompt, agent.system_prompt);
  assert.deepEqual(persona.env_vars, { SECRET: "retained" });
  applyMockManagedAgentUpdate(agent, input, [persona], [agent]);
  input.ownedPersonaUpdate.definition.systemPrompt = "unseen new request";
  const before = JSON.stringify([persona, agent]);
  assert.throws(
    () => applyMockManagedAgentUpdate(agent, input, [persona], [agent]),
    /changed/,
  );
  assert.equal(JSON.stringify([persona, agent]), before);
});
test("invalid owned edit refuses before either cache changes", () => {
  for (const scenario of [
    "foreign",
    "stale",
    "builtin",
    "shared",
    "team",
    "catalog",
    "duplicate",
    "multiple",
    "remote",
  ]) {
    const { persona, agent, input } = fixture();
    const personas = [persona];
    const agents = [agent];
    if (scenario === "foreign")
      input.ownedPersonaUpdate.definition.id = "other";
    if (scenario === "stale")
      input.ownedPersonaUpdate.expectedUpdatedAt = "unseen";
    if (scenario === "builtin") persona.is_builtin = true;
    if (scenario === "shared") persona.shared = true;
    if (scenario === "team") persona.source_team = "team";
    if (scenario === "catalog")
      persona.catalog_source = { persona_id: "catalog" };
    if (scenario === "duplicate") personas.push({ ...persona });
    if (scenario === "multiple") agents.push({ ...agent, pubkey: "sibling" });
    if (scenario === "remote") agent.backend = { type: "provider" };
    const before = JSON.stringify([personas, agents]);
    assert.throws(
      () => applyMockManagedAgentUpdate(agent, input, personas, agents),
      undefined,
      scenario,
    );
    assert.equal(JSON.stringify([personas, agents]), before, scenario);
  }
});
test("ordinary instance/provider update leaves linked definition unchanged", () => {
  const { persona, agent } = fixture();
  agent.backend = { type: "provider" };
  const before = JSON.stringify(persona);
  applyMockManagedAgentUpdate(
    agent,
    {
      pubkey: "actor",
      name: "Remote",
      systemPrompt: "instance",
      respondTo: "anyone",
    },
    [persona],
    [agent],
  );
  assert.equal(agent.name, "Remote");
  assert.equal(agent.system_prompt, "instance");
  assert.equal(agent.respond_to, "anyone");
  assert.equal(JSON.stringify(persona), before);
});
