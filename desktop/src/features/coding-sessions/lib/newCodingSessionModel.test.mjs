import assert from "node:assert/strict";
import test from "node:test";

import {
  isCodingSessionAuthFailure,
  isCodingSessionWorkdirFailure,
  localCodingSessionProviderTarget,
  newCodingSessionStatusMessage,
  resolveNewCodingSessionTargets,
  resolveSelectedNewCodingSessionModel,
  resolveSelectedNewCodingSessionTarget,
  withCodingSessionTurnPrelude,
} from "./newCodingSessionModel.ts";

const PROVIDER_A = "a".repeat(64);
const PROVIDER_B = "b".repeat(64);

function provider(overrides = {}) {
  return {
    providerInstanceRef: "0123456789abcdef",
    driver: "claude-agent-acp",
    runtime: "claude-agent-acp",
    defaultModel: "sonnet",
    allowedModels: ["sonnet", "opus"],
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: true,
      threadSteer: true,
      context: false,
      diff: false,
      plan: true,
    },
    ...overrides,
  };
}

function catalog(overrides = {}) {
  return {
    channelId: "channel-a",
    signerPubkey: PROVIDER_A,
    eventId: "e".repeat(64),
    createdAt: 1_800_000_000,
    catalog: {
      schema: "buzz-coding-session-provider-catalog/v1",
      revision: 1,
      providers: [provider()],
    },
    ...overrides,
  };
}

test("targets come from the catalog's top-level providers, not its projects", () => {
  const targets = resolveNewCodingSessionTargets({
    catalogs: [
      catalog({
        catalog: {
          schema: "buzz-coding-session-provider-catalog/v1",
          revision: 1,
          providers: [provider()],
          // A project narrowing that names no provider must not remove the
          // provider from standalone creation: a standalone session belongs to
          // no project, so the narrowing has nothing to say about it.
          projects: [
            { projectRef: "30621:owner:other", repoRef: null, providers: [] },
          ],
        },
      }),
    ],
  });

  assert.equal(targets.length, 1);
  assert.equal(targets[0].channelId, "channel-a");
  assert.equal(targets[0].signerPubkey, PROVIDER_A);
});

test("a provider that cannot start a turn is not a creation target", () => {
  const targets = resolveNewCodingSessionTargets({
    catalogs: [
      catalog({
        catalog: {
          schema: "buzz-coding-session-provider-catalog/v1",
          revision: 1,
          providers: [
            provider({
              capabilities: {
                ...provider().capabilities,
                threadTurnStart: false,
              },
            }),
          ],
        },
      }),
    ],
  });

  assert.deepEqual(targets, []);
});

test("targets narrow to one channel when asked, and dedupe by selection key", () => {
  const catalogs = [
    catalog(),
    catalog({ channelId: "channel-b", signerPubkey: PROVIDER_B }),
    catalog(),
  ];

  assert.equal(resolveNewCodingSessionTargets({ catalogs }).length, 2);
  const scoped = resolveNewCodingSessionTargets({
    catalogs,
    channelId: "channel-b",
  });
  assert.equal(scoped.length, 1);
  assert.equal(scoped[0].channelId, "channel-b");
});

test("an explicit selection that no longer exists resolves to nothing", () => {
  const targets = resolveNewCodingSessionTargets({ catalogs: [catalog()] });

  assert.equal(
    resolveSelectedNewCodingSessionTarget({
      targets,
      selectedTargetKey: "gone",
      selectionExplicit: true,
    }),
    null,
    "the catalog changing under a person must not create a session against a provider they never picked",
  );
  assert.equal(
    resolveSelectedNewCodingSessionTarget({
      targets,
      selectedTargetKey: null,
      selectionExplicit: false,
    }),
    targets[0],
  );
});

test("model falls back to the provider default until it is chosen explicitly", () => {
  const chosen = provider();

  assert.equal(
    resolveSelectedNewCodingSessionModel({
      provider: chosen,
      selectedModel: "opus",
      selectionExplicit: false,
    }),
    "sonnet",
  );
  assert.equal(
    resolveSelectedNewCodingSessionModel({
      provider: chosen,
      selectedModel: "opus",
      selectionExplicit: true,
    }),
    "opus",
  );
  assert.equal(
    resolveSelectedNewCodingSessionModel({
      provider: chosen,
      selectedModel: "haiku-from-a-stale-catalog",
      selectionExplicit: true,
    }),
    null,
  );
});

test("this computer's provider is a target before it has published anything", () => {
  const target = localCodingSessionProviderTarget({
    channelId: "channel-a",
    providerPubkey: PROVIDER_A,
  });

  assert.equal(target.channelId, "channel-a");
  assert.equal(target.signerPubkey, PROVIDER_A);
  assert.equal(target.provider.providerInstanceRef, "claude-primary");
  assert.deepEqual(
    target.provider.allowedModels,
    [],
    "no model list until a real catalog arrives — the create sends null and the provider defaults",
  );
  assert.equal(target.provider.capabilities.threadTurnStart, true);
});

test("the host phases speak before the relay has anything to say", () => {
  const base = { isPublishing: false, publishError: null, lifecycle: null };

  assert.match(
    newCodingSessionStatusMessage({ ...base, hostPhase: "provisioning" })
      .message,
    /Setting up/,
  );
  assert.match(
    newCodingSessionStatusMessage({ ...base, hostPhase: "joining" }).message,
    /Adding the provider to the channel/,
  );
  assert.equal(
    newCodingSessionStatusMessage({ ...base, hostPhase: "idle" }),
    null,
  );
});

test("a publish error outranks every other status", () => {
  const status = newCodingSessionStatusMessage({
    hostPhase: "joining",
    isPublishing: true,
    publishError: "relay refused the event",
    lifecycle: { state: "pending" },
  });

  assert.deepEqual(status, {
    tone: "destructive",
    message: "relay refused the event",
  });
});

test("the two actionable failure codes get actionable copy", () => {
  const workdir = newCodingSessionStatusMessage({
    isPublishing: false,
    publishError: null,
    lifecycle: {
      state: "failed",
      error: { code: "PROJECT_CWD_UNRESOLVED", message: "no cwd" },
    },
  });
  assert.match(workdir.message, /working directory/);
  assert.equal(isCodingSessionWorkdirFailure("PROJECT_CWD_UNRESOLVED"), true);

  const auth = newCodingSessionStatusMessage({
    isPublishing: false,
    publishError: null,
    lifecycle: {
      state: "failed",
      error: { code: "PROVIDER_AUTH_REQUIRED", message: "not logged in" },
    },
  });
  assert.match(auth.message, /claude/);
  assert.equal(isCodingSessionAuthFailure("PROVIDER_AUTH_REQUIRED"), true);

  // An unrecognized code passes the provider's own words through rather than
  // inventing friendlier copy that would hide what happened.
  const unknown = newCodingSessionStatusMessage({
    isPublishing: false,
    publishError: null,
    lifecycle: {
      state: "failed",
      error: { code: "SOMETHING_NEW", message: "the provider said this" },
    },
  });
  assert.equal(unknown.message, "the provider said this");
});

test("a prelude is dropped rather than costing the user their turn", () => {
  assert.equal(
    withCodingSessionTurnPrelude("do the thing", null),
    "do the thing",
  );
  assert.equal(
    withCodingSessionTurnPrelude("do the thing", "context:"),
    "context:\n\ndo the thing",
  );

  const nearCap = "x".repeat(12 * 1024 - 8);
  assert.equal(
    withCodingSessionTurnPrelude(nearCap, "a".repeat(1024)),
    nearCap,
    "an over-cap combination sends the draft alone",
  );
});
