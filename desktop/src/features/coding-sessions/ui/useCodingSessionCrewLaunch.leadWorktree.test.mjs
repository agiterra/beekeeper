import assert from "node:assert/strict";
import test from "node:test";

import { generateSecretKey, getPublicKey, finalizeEvent } from "nostr-tools";

/**
 * Ledger 188: a team launch cut the lead's tree and never recorded it.
 *
 * The evidence was the live store on Brian's machine: 47 worktree records,
 * and the kettle lead's directory in none of them — only
 * `pending["csl-86e283cf-…"]`, the one-shot create hint. The solo path has
 * had `useCodingSessionWorktreeRecorder` since finding 82; this path never
 * did, so the one tree a project's *lead* runs in was the one tree the host
 * could not name.
 *
 * This pins the second half of the create: when the provider's receipt names
 * the session, the tree it named at create time is filed against it.
 */
const OPERATOR_SECRET = generateSecretKey();
const OPERATOR_PUBKEY = getPublicKey(OPERATOR_SECRET);
const PROVIDER_PUBKEY = "cc".repeat(32);
const SESSION_REF = "3f2504e0-4f89-41d3-9a0c-0305e82c3301";
const CHANNEL_ID = "85b8db75-4b60-4741-bcfa-7f75cc238ff0";

test("a team launch files the lead's tree against the session that settles", async () => {
  const { JSDOM } = await import("jsdom");
  const dom = new JSDOM("<!doctype html><html><body></body></html>", {
    url: "http://localhost",
  });
  const tauriInternals = {
    invoke: async () => {
      throw new Error("no Tauri IPC in this unit test");
    },
    transformCallback: () => Math.random(),
  };
  Object.assign(globalThis, {
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    self: dom.window,
    window: dom.window,
    __TAURI_INTERNALS__: tauriInternals,
  });
  dom.window.__TAURI_INTERNALS__ = tauriInternals;

  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionCrewLaunch } = await import(
    "./useCodingSessionCrewLaunch.ts"
  );
  const { launchCodingSessionCrew } = await import(
    "../lib/codingSessionCrewLaunch.ts"
  );
  const { publishCodingSessionGenesis } = await import(
    "../lib/codingSessionGenesis.ts"
  );
  const { publishCodingSessionGoal } = await import(
    "../lib/codingSessionGoal.ts"
  );

  const signer = async (input) =>
    finalizeEvent({ created_at: 1_700_000_000, ...input }, OPERATOR_SECRET);
  const publisher = { publishEvent: async (event) => event };
  const client = { publisher, signer };

  /** Every `record_coding_session_worktree` call, in order. */
  const recorded = [];
  /** Every one-shot create hint, so the old behaviour is still asserted. */
  const hints = [];

  const deps = {
    runLaunch: launchCodingSessionCrew,
    createWorktree: async (input) => ({
      path: `/Users/brian/Projects/pivot-test-wt-${input.name}`,
    }),
    newSessionRef: () => SESSION_REF,
    newSeatCommandId: () => "csl-86e283cf-dd2c-4d6c-a0ee-b23cd1f9a8c1",
    newTurnCommandId: () => "csc-turn-1",
    ensureProviderMembership: async () => {},
    publishGenesis: (input) => publishCodingSessionGenesis(input, client),
    publishGoal: (input) => publishCodingSessionGoal(input, client),
    stageCreateHint: async (input) => hints.push(input),
    recordWorkdirUse: async () => {},
    recordWorktree: async (input) => {
      recorded.push(input);
      return true;
    },
    seatDeps: {
      ensureMembership: async () => {},
      stageSeat: async () => ({ packStaged: true, packRef: null }),
      clearSeat: async () => {},
      fetchPackSource: async () => null,
    },
    signer,
    publisher,
    recordPendingLifecycle: () => {},
    awaitSeatReceipt: async ({ commandId }) => ({
      driver: "claude-agent-acp",
      instanceId: "claude-primary",
      sessionId: `74495ca8-for-${commandId}`,
      generation: 1,
    }),
    ensureCreateOperatorGrants: async () => ({ ok: true }),
    ensureSeatGrant: async () => ({ ok: true }),
    ensureProjectActionsGrant: async () => ({ ok: true, disclosure: null }),
    publishCommand: async () => ({}),
  };

  const mounted = renderHook(() =>
    useCodingSessionCrewLaunch({
      workdir: "/Users/brian/Projects/pivot-test",
      title: null,
      deps,
    }),
  );

  await act(async () => {
    await mounted.result.current.launch(
      {
        channelId: CHANNEL_ID,
        goal: "Build the kettle CLI",
        projectRef: `30621:${OPERATOR_PUBKEY}:pivot-test`,
        seats: [
          {
            personaId: "p-lead",
            role: "lead",
            actor: "aa".repeat(32),
            actorLabel: "Levain",
            model: "opus[1m]",
            vendor: null,
          },
        ],
        primaryPersonaId: "p-lead",
        provider: {
          label: "claude-agent-acp",
          allowedModels: ["opus[1m]"],
          instanceRef: "claude-primary",
        },
        workdir: "/Users/brian/Projects/pivot-test",
        leadWorktree: { name: "build-kettle-cli-lead", source: null },
      },
      {
        provider: { providerInstanceRef: "claude-primary" },
        signerPubkey: PROVIDER_PUBKEY,
      },
    );
  });

  // The tree the launch cut, filed against the session the receipt named.
  // Before this, only `hints` was written — keyed by the create's commandId,
  // which nothing afterwards knows how to look up.
  assert.equal(recorded.length, 1, JSON.stringify(recorded));
  assert.deepEqual(recorded[0], {
    sessionRef: SESSION_REF,
    seatLabel: "Levain",
    path: "/Users/brian/Projects/pivot-test-wt-build-kettle-cli-lead",
    sessionId: "74495ca8-for-csl-86e283cf-dd2c-4d6c-a0ee-b23cd1f9a8c1",
  });
  // And the create hint is still written — the record is an addition, not a
  // replacement, and the one-shot hint is what the provider's create reads.
  assert.equal(
    hints[0]?.path,
    "/Users/brian/Projects/pivot-test-wt-build-kettle-cli-lead",
  );
});
