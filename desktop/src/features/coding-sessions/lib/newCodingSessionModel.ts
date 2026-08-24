import { MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import type {
  CodingSessionProviderRuntime,
  CodingSessionRuntimeAuthState,
} from "@/shared/api/tauriSessionProvider";

import { formatCodingSessionRuntimeLabel } from "./codingSessionLabels";
import type {
  CodingSessionProviderCatalogProvider,
  TrustedCodingSessionProviderCatalog,
} from "./codingSessionProviderCatalog";
import type { CodingSessionWorkspaceStatus } from "./codingSessionTypes";

export {
  formatCodingSessionProviderLabel,
  formatCodingSessionRuntimeLabel,
} from "./codingSessionLabels";

/**
 * Host-side availability of the runtime behind a target.
 *
 * Only targets served by this computer's own provider carry it — a remote
 * provider's install/sign-in state is unknowable here, and its catalog is
 * taken at its word.
 */
export type NewCodingSessionTargetAvailability = {
  state: CodingSessionRuntimeAuthState;
  /** Human runtime label, e.g. "Claude Code". */
  label: string;
  /** Remediation for a non-ready runtime; `null` when ready. */
  hint: string | null;
};

/**
 * One creatable (channel, signer, provider) combination.
 *
 * The donor derived targets from the catalog's `projects[]` narrowing, because
 * every session it could create belonged to a project. Standalone creation
 * reads the catalog's top-level `providers[]` instead: a session needs a
 * channel to live in and a provider to run it, and nothing else. `projectRef`
 * is therefore absent here and published as an explicit `null`.
 */
export type NewCodingSessionTarget = {
  selectionKey: string;
  channelId: string;
  signerPubkey: string;
  provider: CodingSessionProviderCatalogProvider;
  availability?: NewCodingSessionTargetAvailability;
  /** Signed by this computer's own provider. A published catalog outlives its
   * signer, so a foreign entry may be a dead provider's last word — the local
   * provider is the only one whose liveness this host can vouch for. */
  isLocalProvider?: boolean;
};

/** Host-local knowledge that seeds and annotates the target list. */
export type NewCodingSessionLocalProvider = {
  providerPubkey: string;
  runtimes: readonly CodingSessionProviderRuntime[];
  /** Live per-runtime models, keyed by `instanceRef`, as they resolve. */
  modelsByInstanceRef?: ReadonlyMap<
    string,
    { defaultModel: string; allowedModels: readonly string[] }
  >;
};

/**
 * Every provider that can start a turn: catalog-discovered ones first, then
 * this computer's own runtimes that no catalog has advertised yet.
 *
 * A provider that cannot start a turn is not a creation target — offering it
 * would produce a session that can never be spoken to. Catalog entries win a
 * collision with a bootstrap runtime (same channel, signer, and instance ref):
 * the published catalog is the provider's own signed word about itself.
 * Catalog entries signed by this computer's provider are annotated with the
 * host runtime's live availability, so a signed-out runtime reads as such even
 * after its catalog exists.
 */
export function resolveNewCodingSessionTargets({
  catalogs,
  channelId = null,
  localProvider = null,
}: {
  catalogs: readonly TrustedCodingSessionProviderCatalog[];
  channelId?: string | null;
  localProvider?: NewCodingSessionLocalProvider | null;
}): NewCodingSessionTarget[] {
  const targets: NewCodingSessionTarget[] = [];
  const keys = new Set<string>();
  for (const entry of catalogs) {
    if (channelId !== null && entry.channelId !== channelId) continue;
    for (const provider of entry.catalog.providers) {
      if (!provider.capabilities.threadTurnStart) continue;
      const selectionKey = encodeTargetSelectionKey(
        entry.channelId,
        entry.signerPubkey,
        provider.providerInstanceRef,
      );
      if (keys.has(selectionKey)) continue;
      keys.add(selectionKey);
      const isLocalProvider =
        localProvider !== null &&
        entry.signerPubkey === localProvider.providerPubkey;
      const availability = isLocalProvider
        ? localRuntimeAvailability(
            localProvider.runtimes,
            provider.providerInstanceRef,
            provider.runtime,
          )
        : undefined;
      targets.push({
        selectionKey,
        channelId: entry.channelId,
        signerPubkey: entry.signerPubkey,
        provider,
        ...(availability ? { availability } : {}),
        ...(isLocalProvider ? { isLocalProvider } : {}),
      });
    }
  }
  if (channelId !== null && localProvider) {
    for (const runtime of localProvider.runtimes) {
      const selectionKey = encodeTargetSelectionKey(
        channelId,
        localProvider.providerPubkey,
        runtime.instanceRef,
      );
      if (keys.has(selectionKey)) continue;
      keys.add(selectionKey);
      targets.push(
        localCodingSessionProviderTarget({
          channelId,
          providerPubkey: localProvider.providerPubkey,
          runtime,
          models: localProvider.modelsByInstanceRef?.get(runtime.instanceRef),
        }),
      );
    }
  }
  return targets.sort((left, right) => {
    const byRuntime = left.provider.runtime.localeCompare(
      right.provider.runtime,
    );
    if (byRuntime !== 0) return byRuntime;
    const byProvider = left.provider.providerInstanceRef.localeCompare(
      right.provider.providerInstanceRef,
    );
    return byProvider !== 0
      ? byProvider
      : left.channelId.localeCompare(right.channelId);
  });
}

/** Whether the runtime behind a target can serve a session right now. */
export function isNewCodingSessionTargetReady(
  target: NewCodingSessionTarget,
): boolean {
  return !target.availability || target.availability.state === "ready";
}

export function selectInitialNewCodingSessionTarget(
  targets: readonly NewCodingSessionTarget[],
): NewCodingSessionTarget | null {
  // A foreign catalog entry always reads as ready — its host state is
  // unknowable — even when its provider died long ago. When this computer's
  // own provider offers the same runtime, default to the one whose liveness
  // the host actually supervises; a dead signer's catalog must be an explicit
  // choice, never the silent default.
  return (
    targets.find(
      (target) =>
        target.isLocalProvider && isNewCodingSessionTargetReady(target),
    ) ??
    targets.find(isNewCodingSessionTargetReady) ??
    null
  );
}

/**
 * The claude runtime descriptor assumed before (or without) the host runtimes
 * command answering.
 *
 * It matches the sidecar's zero-config default, and `ready` is deliberately
 * optimistic — exactly the pre-runtimes-command behavior, where a signed-out
 * Claude surfaced through the failed create receipt rather than up front.
 */
export function bootstrapClaudeCodingSessionRuntime(): CodingSessionProviderRuntime {
  return {
    instanceRef: "claude-primary",
    runtime: "claude",
    driver: "claude-agent-acp",
    label: "Claude Code",
    authState: "ready",
    defaultModel: "",
    allowedModels: [],
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: true,
      threadSteer: false,
      context: false,
      diff: false,
      plan: true,
    },
  };
}

/**
 * This computer's own runtime, as a target, before it has published anything.
 *
 * A provider only advertises its catalog into channels it is already a member
 * of, and it only becomes a member when a create flow adds it. Without this the
 * first session would be impossible: no catalog means no target, and no target
 * means nothing ever adds the provider to a channel.
 *
 * The target is deliberately thin on models: until the models command answers
 * for this runtime, the create publishes `model: null` and lets the provider
 * use its own default.
 */
export function localCodingSessionProviderTarget(input: {
  channelId: string;
  providerPubkey: string;
  runtime: CodingSessionProviderRuntime;
  models?: { defaultModel: string; allowedModels: readonly string[] };
}): NewCodingSessionTarget {
  // `instanceRef` routes a create to a catalog entry. It is not the
  // pubkey-derived `instanceId` that later appears in a session's cs-target.
  return {
    selectionKey: encodeTargetSelectionKey(
      input.channelId,
      input.providerPubkey,
      input.runtime.instanceRef,
    ),
    channelId: input.channelId,
    signerPubkey: input.providerPubkey,
    provider: {
      providerInstanceRef: input.runtime.instanceRef,
      driver: input.runtime.driver,
      runtime: input.runtime.runtime,
      defaultModel:
        input.models?.defaultModel ?? input.runtime.defaultModel ?? "",
      allowedModels: [
        ...(input.models?.allowedModels ?? input.runtime.allowedModels ?? []),
      ],
      capabilities: { ...input.runtime.capabilities },
    },
    availability: {
      state: input.runtime.authState,
      label: input.runtime.label,
      hint: codingSessionRuntimeAvailabilityHint(input.runtime),
    },
    isLocalProvider: true,
  };
}

function localRuntimeAvailability(
  runtimes: readonly CodingSessionProviderRuntime[],
  providerInstanceRef: string,
  runtimeSlug: string,
): NewCodingSessionTargetAvailability {
  const runtime = runtimes.find(
    (candidate) => candidate.instanceRef === providerInstanceRef,
  );
  if (!runtime) {
    // The catalog remembers a runtime the host no longer offers — say so
    // rather than letting a create against it fail opaquely.
    const label = formatCodingSessionRuntimeLabel(runtimeSlug);
    return {
      state: "missing",
      label,
      hint: `${label} is not installed on this computer.`,
    };
  }
  return {
    state: runtime.authState,
    label: runtime.label,
    hint: codingSessionRuntimeAvailabilityHint(runtime),
  };
}

/** Honest, actionable one-liner for a runtime that cannot serve right now. */
export function codingSessionRuntimeAvailabilityHint(runtime: {
  runtime: string;
  label: string;
  authState: CodingSessionRuntimeAuthState;
}): string | null {
  if (runtime.authState === "ready") return null;
  if (runtime.authState === "missing") {
    return `${runtime.label} is not installed on this computer.`;
  }
  return codingSessionAuthRemediation({
    runtime: runtime.runtime,
    label: runtime.label,
  }).message;
}

/**
 * The chosen target, or none.
 *
 * An explicit choice that no longer matches any target resolves to `null`
 * rather than silently falling back to the first one: the catalog changing
 * under a person mid-selection must not create a session against a provider
 * they never picked.
 */
export function resolveSelectedNewCodingSessionTarget(input: {
  targets: readonly NewCodingSessionTarget[];
  selectedTargetKey: string | null;
  selectionExplicit: boolean;
}): NewCodingSessionTarget | null {
  if (input.selectionExplicit) {
    return (
      input.targets.find(
        (target) => target.selectionKey === input.selectedTargetKey,
      ) ?? null
    );
  }
  return selectInitialNewCodingSessionTarget(input.targets);
}

export function resolveSelectedNewCodingSessionModel(input: {
  provider: CodingSessionProviderCatalogProvider | null;
  selectedModel: string | null;
  selectionExplicit: boolean;
}): string | null {
  if (!input.provider) return null;
  if (!input.selectionExplicit) return input.provider.defaultModel;
  return input.provider.allowedModels.includes(input.selectedModel ?? "")
    ? input.selectedModel
    : null;
}

/** Which host-local step, if any, the create flow is currently waiting on. */
export type NewCodingSessionHostPhase =
  | "idle"
  | "provisioning"
  | "starting"
  | "joining"
  | "publishing";

/**
 * How long any post-publish wait may stay quietly optimistic.
 *
 * The lifecycle resolution is deliberately clock-free — signed facts either
 * arrived or they did not — so elapsed time lives up here, in copy: a local
 * provider answers a create in about a second, and thirty seconds of silence
 * means a step failed somewhere signed facts cannot reach (dead provider,
 * dropped membership, lost catalog entry), not a slow one.
 */
export const NEW_CODING_SESSION_STALL_MS = 30_000;

/**
 * Which open-ended wait the create is parked in, if any.
 *
 * "opening" is the catalog join: the lifecycle already resolved to a created
 * session but no catalog entry names its target yet, so the screen cannot
 * navigate — the third wait that used to be unbounded.
 */
export function newCodingSessionWaitKey(input: {
  lifecycleState: string | null | undefined;
  resolvedGenerationId: string | null;
  hasTransaction: boolean;
}): "pending" | "awaiting-metadata" | "opening" | null {
  if (!input.hasTransaction) return null;
  switch (input.lifecycleState) {
    case "pending":
      return "pending";
    case "awaiting-metadata":
    case "awaiting-metadata-after-failed-initial-turn":
      return "awaiting-metadata";
    case "created":
    case "created-with-failed-initial-turn":
    case "resumed-without-context":
      return input.resolvedGenerationId === null ? "opening" : null;
    default:
      return null;
  }
}

/**
 * The header badge for the optimistic pending session screen.
 *
 * Optimism is the default — the signed create is durable and the provider
 * almost always accepts — so the badge reads Working/Idle exactly as the real
 * workspace will moments later. Only a definitive problem (publish failure,
 * failed receipt, conflicting receipts) drops to "Status unknown", whose
 * amber dot is the attention signal.
 */
export function pendingCodingSessionWorkspaceStatus(input: {
  lifecycleState: string | null | undefined;
  publishError: string | null;
  hasInitialTurn: boolean;
}): CodingSessionWorkspaceStatus {
  if (
    input.publishError !== null ||
    input.lifecycleState === "failed" ||
    input.lifecycleState === "conflict"
  ) {
    return { kind: "unknown", label: "Status unknown" };
  }
  return input.hasInitialTurn
    ? { kind: "working", label: "Working" }
    : { kind: "idle", label: "Idle" };
}

/**
 * Whether "Retry this exact request" is actionable. Extracted so the pending
 * session screen and the create form's edit view can never drift: retry is
 * pointless while a publish or lifecycle read is in flight, once the session
 * resolved, or after a stall (the relay already holds these exact bytes).
 */
export function canRetryNewCodingSessionCreate(input: {
  isPublishing: boolean;
  lifecycleIsLoading: boolean;
  lifecycleErrorMessage: string | null;
  lifecycleState: string | null | undefined;
  stalled: boolean;
}): boolean {
  return !(
    input.isPublishing ||
    input.lifecycleIsLoading ||
    input.lifecycleErrorMessage !== null ||
    input.lifecycleState === "created" ||
    input.lifecycleState === "created-with-failed-initial-turn" ||
    input.lifecycleState === "resumed-without-context" ||
    input.stalled
  );
}

/**
 * Metadata for the awaited session is arriving but this build cannot decode
 * it. Waiting longer cannot fix a version mismatch, so the copy names the
 * actual remedy.
 */
export const METADATA_DRIFT_MESSAGE =
  "Its metadata is arriving in a format this app does not recognize. The " +
  "app and session-provider versions likely disagree — update both to the " +
  "same release, then restart the app.";

/**
 * What a `resumed_without_context` receipt actually means, in the two halves a
 * person needs: what happened, and what it costs them.
 *
 * The heading is the fact; the body is the consequence. The provider's own
 * message is appended when it has one, because only it knows *why* the context
 * was unrecoverable — this copy must never replace that explanation, only
 * frame it.
 */
export const RESUMED_WITHOUT_CONTEXT_HEADING =
  "Reconnected without prior context";
export const RESUMED_WITHOUT_CONTEXT_BODY =
  "The provider could not restore this execution's previous context. The " +
  "agent starts fresh; the durable session transcript is unaffected.";

/** The heading, the body, and the provider's own reason when it gave one. */
export function codingSessionResumedWithoutContextMessage(error?: {
  message?: string;
}): string {
  const reason = error?.message?.trim();
  return `${RESUMED_WITHOUT_CONTEXT_HEADING}. ${RESUMED_WITHOUT_CONTEXT_BODY}${
    reason ? ` ${reason}` : ""
  }`;
}

/**
 * The one line of status the create screen shows.
 *
 * Every branch is derived, never remembered: this is a pure reading of the
 * publish state plus the signed lifecycle resolution, so a reload that
 * rebuilds both from storage and the relay lands on the same message.
 */
export function newCodingSessionStatusMessage(input: {
  hostPhase?: NewCodingSessionHostPhase;
  isPublishing: boolean;
  publishError: string | null;
  lifecycle:
    | { state: "pending" }
    | { state: "failed"; error: { code?: string; message: string } }
    | { state: "awaiting-metadata"; malformedMetadataCount?: number }
    | {
        state: "awaiting-metadata-after-failed-initial-turn";
        error: { message: string };
        malformedMetadataCount?: number;
      }
    | { state: "created" }
    | {
        state: "created-with-failed-initial-turn";
        error: { message: string };
      }
    | {
        state: "resumed-without-context";
        error: { message?: string };
      }
    | { state: "conflict" }
    | null;
  /** The runtime the failed command targeted, for auth-failure copy. */
  authRuntime?: { runtime: string; label?: string } | null;
  /** True once a wait state has outlived {@link NEW_CODING_SESSION_STALL_MS};
   * escalates the muted spinner copy to a destructive diagnosis. */
  stalled?: boolean;
}): { tone: "muted" | "destructive"; message: string } | null {
  if (input.publishError) {
    return { tone: "destructive", message: input.publishError };
  }
  switch (input.hostPhase) {
    case "provisioning":
      return {
        tone: "muted",
        message: "Setting up this computer's coding-session provider…",
      };
    case "starting":
      return { tone: "muted", message: "Starting the session provider…" };
    case "joining":
      return {
        tone: "muted",
        message: "Adding the provider to the channel…",
      };
    default:
      break;
  }
  if (input.isPublishing) {
    return { tone: "muted", message: "Publishing signed session request…" };
  }
  switch (input.lifecycle?.state) {
    case "pending":
      if (input.stalled) {
        return {
          tone: "destructive",
          message:
            "The session provider has not accepted this request after " +
            "30 seconds. It may be offline or not a member of this " +
            "channel. Start fresh to try again.",
        };
      }
      return {
        tone: "muted",
        message: "Waiting for the session provider to accept this request…",
      };
    case "failed":
      return {
        tone: "destructive",
        message: newCodingSessionFailureMessage(
          input.lifecycle.error,
          input.authRuntime,
        ),
      };
    case "awaiting-metadata":
      if ((input.lifecycle.malformedMetadataCount ?? 0) > 0) {
        return {
          tone: "destructive",
          message: `Session created. ${METADATA_DRIFT_MESSAGE}`,
        };
      }
      if (input.stalled) {
        return {
          tone: "destructive",
          message:
            "Session created, but its signed metadata has not arrived " +
            "after 30 seconds. The provider may have restarted. Start " +
            "fresh to try again.",
        };
      }
      return {
        tone: "muted",
        message: "Session created. Waiting for its signed metadata…",
      };
    case "awaiting-metadata-after-failed-initial-turn":
      if ((input.lifecycle.malformedMetadataCount ?? 0) > 0) {
        return {
          tone: "destructive",
          message: `Session created, but its initial turn failed: ${input.lifecycle.error.message} ${METADATA_DRIFT_MESSAGE}`,
        };
      }
      return {
        tone: "destructive",
        message: `Session created, but its initial turn failed: ${input.lifecycle.error.message} Waiting for its signed metadata…`,
      };
    case "conflict":
      return {
        tone: "destructive",
        message:
          "Conflicting signed lifecycle receipts were received. Start a fresh request.",
      };
    case "created":
      if (input.stalled) {
        return {
          tone: "destructive",
          message:
            "The session was created but has not appeared in this " +
            "channel's catalog after 30 seconds. Start fresh, or reopen " +
            "this screen.",
        };
      }
      return { tone: "muted", message: "Opening the exact signed session…" };
    case "created-with-failed-initial-turn":
      return {
        tone: "destructive",
        message: `Session created, but its initial turn failed: ${input.lifecycle.error.message}`,
      };
    case "resumed-without-context":
      // Not a spinner: the session is open, and the loss of context is
      // something the person has to act on (re-brief the agent), not wait out.
      return {
        tone: "destructive",
        message: codingSessionResumedWithoutContextMessage(
          input.lifecycle.error,
        ),
      };
    default:
      return null;
  }
}

/**
 * The failure codes a person can actually do something about.
 *
 * Everything else passes the provider's own message through — inventing
 * friendlier copy for a code we do not recognize would hide what happened.
 */
export function newCodingSessionFailureMessage(
  error: {
    code?: string;
    message: string;
  },
  authRuntime?: { runtime: string; label?: string } | null,
): string {
  if (error.code === "PROJECT_CWD_UNRESOLVED") {
    return "The provider could not resolve a working directory for this session. Choose one below and try again.";
  }
  if (error.code === "PROVIDER_AUTH_REQUIRED") {
    return codingSessionAuthRemediation(authRuntime).message;
  }
  if (error.code === "SESSION_LIMIT") {
    // Read as an account limit — "there can only be 4 concurrent Claude
    // sessions?" — when it is this computer's own provider holding at most N
    // live agent processes. The provider's sentence carries the number and
    // the remediation; this adds who is imposing it.
    return `${error.message}. This is Bee Keeper's own cap on this computer, not a limit from the model provider — every other session in this community is unaffected.`;
  }
  if (
    error.code === "PROVIDER_UNAVAILABLE" &&
    error.message.includes("unknown providerInstanceRef")
  ) {
    // The running provider offers the runtime set it was spawned with; a
    // runtime installed afterwards shows as ready in the picker but is not in
    // that offer until the provider restarts. Without this line the raw
    // receipt reads like a dead end, when a restart is the whole fix.
    return `${error.message}. This usually means the runtime was installed after the provider started — restart Bee Keeper to refresh its available runtimes, then try again.`;
  }
  return error.message;
}

/**
 * The subset of an adapter's advertised auth methods a Connect button may
 * launch: the ones the runtime's own CLI drives (`type: "terminal"`, or a
 * `terminal-auth` command in `_meta`). Mirrors `uses_terminal_auth` in
 * `commands/agent_auth.rs` — anything else (API-key entry and other
 * adapter-mediated schemes) is deliberately not offered here: CLI login is
 * the only credential path for coding sessions.
 */
export function connectableCodingSessionAuthMethods<
  M extends { type: string | null; meta: unknown },
>(methods: readonly M[]): M[] {
  return methods.filter((method) => {
    if (method.type === "terminal") return true;
    if (typeof method.meta !== "object" || method.meta === null) return false;
    const terminalAuth = (method.meta as Record<string, unknown>)[
      "terminal-auth"
    ];
    return (
      typeof terminalAuth === "object" &&
      terminalAuth !== null &&
      "command" in terminalAuth
    );
  });
}

/**
 * Whether a Connect launch completes without a terminal window: the Claude
 * subscription login runs headless (the CLI drives a browser flow). Mirrors
 * `is_claude_subscription_login` in `commands/agent_auth.rs`; used only to
 * pick the right "what happens next" guidance line.
 */
export function isHeadlessCodingSessionLogin(
  runtime: string,
  methodId: string,
): boolean {
  return (
    runtime === "claude" &&
    (methodId === "claude-login" || methodId === "claude-ai-login")
  );
}

/** What a signed-out runtime needs a person to do, in that runtime's terms. */
export type CodingSessionAuthRemediation = {
  /** Alert heading, e.g. "Claude login needed". */
  title: string;
  /** Full remediation sentence, terminal command in backticks when known. */
  message: string;
  /** The exact terminal command that fixes it, when one is known. */
  command: string | null;
};

/**
 * Provider-appropriate sign-in copy for `PROVIDER_AUTH_REQUIRED`.
 *
 * With no runtime context — a receipt observed without knowing which target
 * it addressed — this falls back to the bundled default runtime, claude,
 * which is also the pre-multi-runtime behavior.
 */
export function codingSessionAuthRemediation(
  runtime?: { runtime: string; label?: string } | null,
): CodingSessionAuthRemediation {
  const tokens = new Set(
    (runtime?.runtime ?? "claude")
      .toLowerCase()
      .split(/[-_\s]+/)
      .filter(Boolean),
  );
  if (tokens.has("claude") || tokens.has("cc")) {
    return {
      title: "Claude login needed",
      message:
        "Claude Code is not signed in on this computer. Use Connect to sign in, or run `claude` in a terminal, then try again.",
      command: "claude",
    };
  }
  if (tokens.has("codex")) {
    return {
      title: "Codex login needed",
      message:
        "Codex is not signed in on this computer. Use Connect to sign in, or run `codex login` in a terminal, then try again.",
      command: "codex login",
    };
  }
  const label =
    runtime?.label && runtime.label.trim().length > 0
      ? runtime.label
      : formatCodingSessionRuntimeLabel(runtime?.runtime ?? "");
  return {
    title: `${label} login needed`,
    message: `${label} is not signed in on this computer. Complete its sign-in, then try again.`,
    command: null,
  };
}

/** Whether a failed receipt is asking for a working directory specifically. */
export function isCodingSessionWorkdirFailure(code: string | undefined) {
  return code === "PROJECT_CWD_UNRESOLVED";
}

/** Whether a failed receipt is asking for a runtime sign-in specifically. */
export function isCodingSessionAuthFailure(code: string | undefined) {
  return code === "PROVIDER_AUTH_REQUIRED";
}

/**
 * Build a new session's initial turn, prepending an optional prelude block.
 *
 * The draft always wins. `initialTurn` is capped at
 * {@link MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES} by the lifecycle
 * command schema, but a compose box only measures the user's own draft — so a
 * near-limit draft plus a prelude would overflow the cap and the signed command
 * would be rejected. A prelude must never cost the user their session, so when
 * the combined turn would not fit, the prelude is dropped and the draft is sent
 * alone.
 */
export function withCodingSessionTurnPrelude(
  draftText: string,
  prelude: string | null | undefined,
): string {
  if (!prelude || prelude.trim().length === 0) return draftText;
  const combined = `${prelude}\n\n${draftText}`;
  const bytes = new TextEncoder().encode(combined).byteLength;
  if (bytes > MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES) {
    return draftText;
  }
  return combined;
}

function encodeTargetSelectionKey(...fields: (string | null)[]): string {
  const encoder = new TextEncoder();
  return fields
    .map((field) => {
      const value = field ?? "";
      return `${encoder.encode(value).byteLength}:${value}`;
    })
    .join("");
}
