import assert from "node:assert/strict";
import test from "node:test";

import {
  bootstrapClaudeCodingSessionRuntime,
  codingSessionAuthRemediation,
  connectableCodingSessionAuthMethods,
  isHeadlessCodingSessionLogin,
  isCodingSessionAuthFailure,
  isCodingSessionWorkdirFailure,
  isNewCodingSessionTargetReady,
  localCodingSessionProviderTarget,
  newCodingSessionStatusMessage,
  resolveNewCodingSessionTargets,
  resolveSelectedNewCodingSessionModel,
  resolveSelectedNewCodingSessionTarget,
  selectInitialNewCodingSessionTarget,
  withCodingSessionTurnPrelude,
} from "./newCodingSessionModel.ts";

const PROVIDER_A = "a".repeat(64);
const PROVIDER_B = "b".repeat(64);

function runtime(overrides = {}) {
  return {
    instanceRef: "claude-primary",
    runtime: "claude",
    driver: "claude-agent-acp",
    label: "Claude Code",
    authState: "ready",
    defaultModel: "default",
    allowedModels: ["default"],
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: true,
      threadSteer: false,
      context: false,
      diff: false,
      plan: true,
    },
    ...overrides,
  };
}

function codexRuntime(overrides = {}) {
  return runtime({
    instanceRef: "codex-primary",
    runtime: "codex",
    driver: "codex-acp",
    label: "Codex",
    capabilities: { ...runtime().capabilities, plan: false },
    ...overrides,
  });
}

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

test("this computer's runtime is a target before it has published anything", () => {
  const target = localCodingSessionProviderTarget({
    channelId: "channel-a",
    providerPubkey: PROVIDER_A,
    runtime: runtime(),
    models: {
      defaultModel: "default",
      allowedModels: ["default", "haiku", "sonnet"],
    },
  });

  assert.equal(target.channelId, "channel-a");
  assert.equal(target.signerPubkey, PROVIDER_A);
  assert.equal(target.provider.providerInstanceRef, "claude-primary");
  assert.equal(target.provider.driver, "claude-agent-acp");
  assert.equal(target.provider.defaultModel, "default");
  assert.deepEqual(target.provider.allowedModels, [
    "default",
    "haiku",
    "sonnet",
  ]);
  assert.equal(target.provider.capabilities.threadTurnStart, true);
  assert.equal(target.availability.state, "ready");
  assert.equal(target.availability.hint, null);
  assert.equal(isNewCodingSessionTargetReady(target), true);
});

test("every host runtime becomes a bootstrap target when no catalog exists", () => {
  const targets = resolveNewCodingSessionTargets({
    catalogs: [],
    channelId: "channel-a",
    localProvider: {
      providerPubkey: PROVIDER_A,
      runtimes: [runtime(), codexRuntime()],
      modelsByInstanceRef: new Map([
        [
          "claude-primary",
          { defaultModel: "sonnet", allowedModels: ["sonnet", "opus"] },
        ],
      ]),
    },
  });

  assert.equal(targets.length, 2);
  assert.equal(
    new Set(targets.map((target) => target.selectionKey)).size,
    targets.length,
    "runtimes sharing one signer still need distinct React/selection keys",
  );
  assert.equal(targets[0].signerPubkey, targets[1].signerPubkey);
  // Sorted by runtime slug: claude before codex.
  assert.equal(targets[0].provider.providerInstanceRef, "claude-primary");
  assert.equal(targets[1].provider.providerInstanceRef, "codex-primary");
  // Live models win for the runtime that has them; the codex runtime keeps
  // its static defaults.
  assert.equal(targets[0].provider.defaultModel, "sonnet");
  assert.deepEqual(targets[1].provider.allowedModels, ["default"]);
});

test("a single ready runtime yields exactly one target, as before", () => {
  const targets = resolveNewCodingSessionTargets({
    catalogs: [],
    channelId: "channel-a",
    localProvider: {
      providerPubkey: PROVIDER_A,
      runtimes: [bootstrapClaudeCodingSessionRuntime()],
    },
  });

  assert.equal(targets.length, 1);
  assert.equal(targets[0].provider.providerInstanceRef, "claude-primary");
  assert.equal(targets[0].provider.runtime, "claude");
  assert.equal(isNewCodingSessionTargetReady(targets[0]), true);
  assert.equal(selectInitialNewCodingSessionTarget(targets), targets[0]);
});

test("a catalog entry beats the bootstrap runtime for the same coordinate", () => {
  const targets = resolveNewCodingSessionTargets({
    catalogs: [
      catalog({
        signerPubkey: PROVIDER_A,
        catalog: {
          schema: "buzz-coding-session-provider-catalog/v1",
          revision: 1,
          providers: [
            provider({
              providerInstanceRef: "claude-primary",
              runtime: "claude",
              defaultModel: "opus",
              allowedModels: ["opus", "sonnet"],
            }),
          ],
        },
      }),
    ],
    channelId: "channel-a",
    localProvider: {
      providerPubkey: PROVIDER_A,
      runtimes: [runtime(), codexRuntime()],
    },
  });

  assert.equal(targets.length, 2);
  const claude = targets.find(
    (target) => target.provider.providerInstanceRef === "claude-primary",
  );
  // The catalog's signed word about models wins over static defaults …
  assert.equal(claude.provider.defaultModel, "opus");
  // … and the host still annotates its own catalog entry with availability.
  assert.equal(claude.availability.state, "ready");
  // The codex runtime, absent from the catalog, still bootstraps.
  assert.ok(
    targets.some(
      (target) => target.provider.providerInstanceRef === "codex-primary",
    ),
  );
});

test("a signed-out runtime stays visible, disabled, honestly hinted, never auto-selected", () => {
  const targets = resolveNewCodingSessionTargets({
    catalogs: [],
    channelId: "channel-a",
    localProvider: {
      providerPubkey: PROVIDER_A,
      runtimes: [
        codexRuntime({ authState: "needs_auth" }),
        runtime({
          instanceRef: "goose-primary",
          runtime: "goose",
          driver: "goose-acp",
          label: "Goose",
          authState: "missing",
        }),
        runtime(),
      ],
    },
  });

  assert.equal(targets.length, 3, "non-ready runtimes are shown, not hidden");
  const codex = targets.find((target) => target.provider.runtime === "codex");
  assert.equal(isNewCodingSessionTargetReady(codex), false);
  assert.match(codex.availability.hint, /codex login/);
  const goose = targets.find((target) => target.provider.runtime === "goose");
  assert.equal(goose.availability.state, "missing");
  assert.match(goose.availability.hint, /not installed/);
  // Initial selection skips past the disabled runtimes to the ready one.
  const initial = selectInitialNewCodingSessionTarget(targets);
  assert.equal(initial.provider.runtime, "claude");
});

test("no ready runtime means no initial selection at all", () => {
  const targets = resolveNewCodingSessionTargets({
    catalogs: [],
    channelId: "channel-a",
    localProvider: {
      providerPubkey: PROVIDER_A,
      runtimes: [codexRuntime({ authState: "needs_auth" })],
    },
  });

  assert.equal(targets.length, 1);
  assert.equal(selectInitialNewCodingSessionTarget(targets), null);
});

test("the bootstrap claude runtime matches the sidecar's zero-config default", () => {
  const bootstrap = bootstrapClaudeCodingSessionRuntime();

  assert.equal(bootstrap.instanceRef, "claude-primary");
  assert.equal(bootstrap.driver, "claude-agent-acp");
  assert.equal(bootstrap.runtime, "claude");
  assert.equal(bootstrap.authState, "ready");
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

  // The same failure against a codex target names codex's own login command.
  const codexAuth = newCodingSessionStatusMessage({
    isPublishing: false,
    publishError: null,
    lifecycle: {
      state: "failed",
      error: { code: "PROVIDER_AUTH_REQUIRED", message: "not logged in" },
    },
    authRuntime: { runtime: "codex", label: "Codex" },
  });
  assert.match(codexAuth.message, /codex login/);
  assert.doesNotMatch(codexAuth.message, /Claude/);

  // A runtime installed after the provider started shows ready in the picker
  // but is missing from the running provider's offer. The receipt alone reads
  // like a dead end; the copy has to name the fix (restart).
  const staleOffer = newCodingSessionStatusMessage({
    isPublishing: false,
    publishError: null,
    lifecycle: {
      state: "failed",
      error: {
        code: "PROVIDER_UNAVAILABLE",
        message:
          'unknown providerInstanceRef "codex-primary"; this provider offers: claude-primary',
      },
    },
  });
  assert.match(staleOffer.message, /codex-primary/);
  assert.match(staleOffer.message, /restart Buzz/i);

  // Other PROVIDER_UNAVAILABLE failures (adapter would not start) keep the
  // provider's own words — a restart hint there would be a guess.
  const adapterDown = newCodingSessionStatusMessage({
    isPublishing: false,
    publishError: null,
    lifecycle: {
      state: "failed",
      error: {
        code: "PROVIDER_UNAVAILABLE",
        message: "the adapter exited before session/new completed",
      },
    },
  });
  assert.equal(
    adapterDown.message,
    "the adapter exited before session/new completed",
  );

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

test("auth remediation speaks each runtime's own language", () => {
  const claude = codingSessionAuthRemediation({ runtime: "claude" });
  assert.equal(claude.title, "Claude login needed");
  assert.equal(claude.command, "claude");
  assert.match(claude.message, /run `claude` in a terminal/);
  assert.match(claude.message, /Use Connect to sign in/);

  // Legacy catalogs carry the driver-shaped slug; it still reads as claude.
  const legacy = codingSessionAuthRemediation({ runtime: "claude-agent-acp" });
  assert.equal(legacy.command, "claude");

  const codex = codingSessionAuthRemediation({ runtime: "codex" });
  assert.equal(codex.title, "Codex login needed");
  assert.equal(codex.command, "codex login");

  // An unknown runtime gets its label and no invented command.
  const goose = codingSessionAuthRemediation({
    runtime: "goose",
    label: "Goose",
  });
  assert.equal(goose.title, "Goose login needed");
  assert.equal(goose.command, null);
  assert.match(goose.message, /Goose is not signed in/);

  // No runtime context falls back to the bundled default, claude.
  assert.equal(codingSessionAuthRemediation(null).command, "claude");
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

test("only CLI-driven auth methods are connectable", () => {
  const terminalTyped = {
    id: "codex-login",
    type: "terminal",
    meta: null,
  };
  const terminalMeta = {
    id: "claude-login",
    type: null,
    meta: { "terminal-auth": { command: "node", args: ["/tmp/cli.js"] } },
  };
  const apiKey = { id: "api-key", type: "api-key", meta: null };
  const metaWithoutCommand = {
    id: "odd",
    type: null,
    meta: { "terminal-auth": {} },
  };

  assert.deepEqual(
    connectableCodingSessionAuthMethods([
      terminalTyped,
      terminalMeta,
      apiKey,
      metaWithoutCommand,
    ]).map((method) => method.id),
    ["codex-login", "claude-login"],
  );
});

test("only the claude subscription login counts as headless", () => {
  assert.equal(isHeadlessCodingSessionLogin("claude", "claude-login"), true);
  assert.equal(isHeadlessCodingSessionLogin("claude", "claude-ai-login"), true);
  assert.equal(isHeadlessCodingSessionLogin("claude", "other"), false);
  assert.equal(isHeadlessCodingSessionLogin("codex", "claude-login"), false);
});
