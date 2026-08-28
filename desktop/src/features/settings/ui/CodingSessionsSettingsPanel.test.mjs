/**
 * The hiring policy, as a control a person reads.
 *
 * The rule this file exists to hold: every control says what it *enforces*,
 * with the refusal code the lead will receive. A switch labelled "Let leads
 * hire" that did not say what off means would be the same class of lie as a
 * status reading Idle over a dead provider.
 */
import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { managedAgentsQueryKey } from "@/features/agents/hooks";
import { CodingSessionHiringCard } from "./CodingSessionsSettingsPanel.tsx";
import {
  CODING_SESSION_HIRE_POLICY_STORAGE_KEY,
  readCodingSessionHirePolicy,
} from "@/features/coding-sessions/lib/codingSessionHirePolicy.ts";

const store = new Map();
globalThis.window = globalThis.window ?? {};
globalThis.window.localStorage = {
  getItem: (key) => (store.has(key) ? store.get(key) : null),
  setItem: (key, value) => store.set(key, value),
  removeItem: (key) => store.delete(key),
};

const AGENTS = [
  {
    pubkey: "a".repeat(64),
    name: "Ada",
    homeRole: "builder",
    hasRolePack: true,
  },
  {
    pubkey: "b".repeat(64),
    name: "Cai",
    homeRole: "architect",
    hasRolePack: true,
  },
];

const RUNTIMES = [
  { instanceRef: "claude-primary", label: "Claude Code", authState: "ready" },
  { instanceRef: "codex-primary", label: "Codex", authState: "needs_auth" },
];

function render() {
  const client = new QueryClient();
  client.setQueryData(managedAgentsQueryKey, AGENTS);
  client.setQueryData(["coding-session-provider-runtimes"], RUNTIMES);
  return renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(CodingSessionHiringCard, null),
    ),
  );
}

test("the card offers exactly the roles whose packs are installed here", () => {
  store.clear();
  const html = render();
  assert.match(html, /data-testid="coding-session-hire-role-builder"/);
  assert.match(html, /data-testid="coding-session-hire-role-architect"/);
  assert.doesNotMatch(html, /coding-session-hire-role-verifier/);
});

test("only a signed-in runtime is offered as a place a hire may run", () => {
  store.clear();
  const html = render();
  assert.match(
    html,
    /data-testid="coding-session-hire-provider-claude-primary"/,
  );
  assert.doesNotMatch(html, /coding-session-hire-provider-codex-primary/);
});

test("each control names the refusal it produces", () => {
  store.clear();
  const html = render();
  assert.match(html, /HIRE_LIMIT/);
  assert.match(html, /HIRE_ROLE_NOT_ALLOWED|whose pack is installed here/);
  assert.match(
    html,
    /HIRE_PROVIDER_NOT_ALLOWED|Every runtime this computer runs/,
  );
});

test("hiring off says every request is refused HIRE_OFF, not just 'disabled'", () => {
  store.clear();
  store.set(
    CODING_SESSION_HIRE_POLICY_STORAGE_KEY,
    JSON.stringify({
      enabled: false,
      allowedRoles: null,
      maxSeatsPerUmbrella: 4,
      allowedProviderInstanceRefs: null,
    }),
  );
  assert.equal(readCodingSessionHirePolicy().enabled, false);
  const html = render();
  assert.match(html, /refused HIRE_OFF/);
});

test("the seat ceiling shown is the stored one", () => {
  store.clear();
  store.set(
    CODING_SESSION_HIRE_POLICY_STORAGE_KEY,
    JSON.stringify({
      enabled: true,
      allowedRoles: ["builder"],
      maxSeatsPerUmbrella: 7,
      allowedProviderInstanceRefs: [],
    }),
  );
  const html = render();
  assert.match(html, /value="7"/);
  assert.match(html, /Only builder\./);
  assert.match(
    html,
    /No providers: every hire is refused HIRE_PROVIDER_NOT_ALLOWED/,
  );
});
