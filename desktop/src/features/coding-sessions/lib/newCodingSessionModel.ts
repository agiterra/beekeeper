import { MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";

import type {
  CodingSessionProviderCatalogProvider,
  TrustedCodingSessionProviderCatalog,
} from "./codingSessionProviderCatalog";

export {
  formatCodingSessionProviderLabel,
  formatCodingSessionRuntimeLabel,
} from "./codingSessionLabels";

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
};

/**
 * Every provider that can start a turn, across the trusted catalogs.
 *
 * A provider that cannot start a turn is not a creation target — offering it
 * would produce a session that can never be spoken to.
 */
export function resolveNewCodingSessionTargets({
  catalogs,
  channelId = null,
}: {
  catalogs: readonly TrustedCodingSessionProviderCatalog[];
  channelId?: string | null;
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
      targets.push({
        selectionKey,
        channelId: entry.channelId,
        signerPubkey: entry.signerPubkey,
        provider,
      });
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

export function selectInitialNewCodingSessionTarget(
  targets: readonly NewCodingSessionTarget[],
): NewCodingSessionTarget | null {
  return targets[0] ?? null;
}

/**
 * This computer's own provider, as a target, before it has published anything.
 *
 * A provider only advertises its catalog into channels it is already a member
 * of, and it only becomes a member when a create flow adds it. Without this the
 * first session would be impossible: no catalog means no target, and no target
 * means nothing ever adds the provider to a channel.
 *
 * The target is deliberately thin — no model list, no declared capabilities.
 * Everything specific arrives with the catalog the provider publishes once it
 * is in the channel; until then the create publishes `model: null` and lets the
 * provider use its own default.
 */
export function localCodingSessionProviderTarget(input: {
  channelId: string;
  providerPubkey: string;
  instanceId: string;
}): NewCodingSessionTarget {
  return {
    selectionKey: encodeTargetSelectionKey(
      input.channelId,
      input.providerPubkey,
      input.instanceId,
    ),
    channelId: input.channelId,
    signerPubkey: input.providerPubkey,
    provider: {
      providerInstanceRef: input.instanceId,
      driver: "claude-agent-acp",
      runtime: "claude-agent-acp",
      defaultModel: "",
      allowedModels: [],
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: true,
        context: false,
        diff: false,
        plan: true,
      },
    },
  };
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
    | { state: "awaiting-metadata" }
    | {
        state: "awaiting-metadata-after-failed-initial-turn";
        error: { message: string };
      }
    | { state: "created" }
    | {
        state: "created-with-failed-initial-turn";
        error: { message: string };
      }
    | { state: "conflict" }
    | null;
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
      return {
        tone: "muted",
        message: "Waiting for the session provider to accept this request…",
      };
    case "failed":
      return {
        tone: "destructive",
        message: newCodingSessionFailureMessage(input.lifecycle.error),
      };
    case "awaiting-metadata":
      return {
        tone: "muted",
        message: "Session created. Waiting for its signed metadata…",
      };
    case "awaiting-metadata-after-failed-initial-turn":
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
      return { tone: "muted", message: "Opening the exact signed session…" };
    case "created-with-failed-initial-turn":
      return {
        tone: "destructive",
        message: `Session created, but its initial turn failed: ${input.lifecycle.error.message}`,
      };
    default:
      return null;
  }
}

/**
 * The two failure codes a person can actually do something about.
 *
 * Everything else passes the provider's own message through — inventing
 * friendlier copy for a code we do not recognize would hide what happened.
 */
export function newCodingSessionFailureMessage(error: {
  code?: string;
  message: string;
}): string {
  if (error.code === "PROJECT_CWD_UNRESOLVED") {
    return "The provider could not resolve a working directory for this session. Choose one below and try again.";
  }
  if (error.code === "PROVIDER_AUTH_REQUIRED") {
    return "Claude Code is not signed in on this computer. Run `claude` in a terminal, complete the login, then try again.";
  }
  return error.message;
}

/** Whether a failed receipt is asking for a working directory specifically. */
export function isCodingSessionWorkdirFailure(code: string | undefined) {
  return code === "PROJECT_CWD_UNRESOLVED";
}

/** Whether a failed receipt is asking for a Claude Code login specifically. */
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
