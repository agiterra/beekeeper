import type { ManagedAgentRuntimeStatus } from "@/shared/api/types";

export type AgentCommunityAvailability =
  | "Here"
  | "Waking"
  | "Needs setup on this device"
  | "Unavailable";
// Host ownership alone does not certify runtime/account availability.

/**
 * A host-managed pair with no live execution lease is idle, not gone, when
 * the provider's presence heartbeat is live *and names this pair's
 * execution*. A live heartbeat whose `liveTargets` does not include it
 * (`execution: "absent"`) is a provider that is up without this execution —
 * never idle. Only an expired or absent heartbeat (or no presence report at
 * all) reads as disconnected.
 */
export function hostManagedIdleWithLivePresence(
  runtime: Pick<
    ManagedAgentRuntimeStatus,
    "runtimeAvailable" | "providerPresence" | "execution"
  >,
): boolean {
  return (
    runtime.runtimeAvailable !== true &&
    runtime.providerPresence === "live" &&
    runtime.execution !== "absent"
  );
}

/**
 * The provider's heartbeat is live but it does not run this pair's
 * execution: the only state in which "Here" and "Stop" would both lie.
 */
export function hostManagedAbsentFromLivePresence(
  runtime: Pick<
    ManagedAgentRuntimeStatus,
    "runtimeAvailable" | "providerPresence" | "execution"
  >,
): boolean {
  return (
    runtime.runtimeAvailable !== true &&
    runtime.providerPresence === "live" &&
    runtime.execution === "absent"
  );
}

export function agentCommunityAvailability(
  runtime: ManagedAgentRuntimeStatus,
): AgentCommunityAvailability {
  if (runtime.lifecycle === "host_managed") {
    if (runtime.runtimeAvailable === true) return "Here";
    return hostManagedIdleWithLivePresence(runtime) ? "Here" : "Unavailable";
  }
  if (!runtime.localSetup) return "Needs setup on this device";

  switch (runtime.lifecycle) {
    case "starting":
    case "listening":
    case "waking":
      return "Waking";
    case "ready":
      return "Here";
    case "failed":
    case "stopped":
      return "Unavailable";
  }
}

export function agentCommunityStatusDetail(
  runtime: ManagedAgentRuntimeStatus,
): string | null {
  if (runtime.lifecycle === "host_managed") {
    if (runtime.runtimeAvailable === true)
      return "Managed by the host. Current execution availability is verified by a signed relay lease.";
    if (hostManagedIdleWithLivePresence(runtime))
      return "Managed by the host. Idle: the provider's presence heartbeat is live and no execution is running.";
    if (hostManagedAbsentFromLivePresence(runtime))
      return "Managed by the host. Absent: the provider is running but reports no execution for this agent; start it to create one.";
    if (runtime.providerPresence === "expired")
      return "Managed by the host. Disconnected: the provider's last presence heartbeat has expired and no execution lease is live.";
    if (runtime.runtimeAvailable === false)
      return "Managed by the host. No current verified execution lease is available in this community.";
    return "Managed by the host. Runtime availability is unconfirmed; relay status could not be verified.";
  }
  if (runtime.lifecycle === "failed" && runtime.error) return runtime.error;
  if (!runtime.localSetup)
    return "Set up this agent on this device to start it.";
  if (runtime.lifecycle === "stopped") return "Stopped by you";
  if (runtime.lifecycle === "failed")
    return runtime.error ?? "Could not connect";
  return null;
}

export function managedAgentRuntimeKey(
  runtime: Pick<ManagedAgentRuntimeStatus, "pubkey" | "relayUrl">,
): string {
  return JSON.stringify([runtime.pubkey, runtime.relayUrl]);
}

export type ManagedAgentPairAction = "start" | "stop" | "restart";

/** Reconcile one agent+community pair through the shared lifecycle path.
 * Missing availability is not termination proof; Start preserves backend fences. */
export function managedAgentPairAction(
  runtime: ManagedAgentRuntimeStatus | undefined,
): ManagedAgentPairAction {
  if (!runtime || runtime.lifecycle === "stopped") return "start";
  if (
    runtime.lifecycle === "host_managed" &&
    runtime.runtimeAvailable !== true &&
    !hostManagedIdleWithLivePresence(runtime)
  )
    return "start";
  if (runtime.lifecycle === "failed") return "restart";
  return "stop";
}

export const MANAGED_AGENT_PAIR_ACTION_LABELS: Record<
  ManagedAgentPairAction,
  string
> = {
  start: "Start Agent",
  stop: "Stop Agent",
  restart: "Restart Agent",
};

/**
 * Canonicalize a relay URL the way the backend keys runtime pairs, so a
 * stored community URL (e.g. `ws://localhost:3000`) matches backend rows
 * (`ws://127.0.0.1:3000`). Mirrors buzz-core's `normalize_relay_url`
 * (`crates/beekeeper-core/src/relay.rs`): lowercase host, loopback hosts folded
 * to 127.0.0.1, default ports and root-path trailing slash stripped.
 * Returns null when the URL cannot be parsed as ws/wss.
 */
export function canonicalRelayUrl(raw: string): string | null {
  let url: URL;
  try {
    url = new URL(raw.trim());
  } catch {
    return null;
  }
  if (url.protocol !== "ws:" && url.protocol !== "wss:") return null;
  let host = url.hostname.toLowerCase();
  if (host === "localhost" || host === "[::1]" || host.startsWith("127.")) {
    host = "127.0.0.1";
  }
  const defaultPort = url.protocol === "ws:" ? "80" : "443";
  const port = url.port && url.port !== defaultPort ? `:${url.port}` : "";
  const path = url.pathname === "/" ? "" : url.pathname;
  // The backend trims trailing slashes from the final rendered URL.
  return `${url.protocol}//${host}${port}${path}${url.search}`.replace(
    /\/+$/,
    "",
  );
}

export function findManagedAgentRuntime(
  runtimes: readonly ManagedAgentRuntimeStatus[],
  pubkey: string,
  relayUrl: string,
): ManagedAgentRuntimeStatus | undefined {
  const normalizedPubkey = pubkey.toLowerCase();
  // Backend rows carry the canonical pair URL; the caller passes the
  // community's stored URL, which may differ in spelling (localhost vs
  // 127.0.0.1, default port, trailing slash). Compare canonically, keeping
  // the exact-string checks as a fallback for unparsable stored URLs.
  const canonical = canonicalRelayUrl(relayUrl);
  return runtimes.find(
    (runtime) =>
      runtime.pubkey.toLowerCase() === normalizedPubkey &&
      (runtime.relayUrl === relayUrl ||
        runtime.requestedRelayUrl === relayUrl ||
        (canonical !== null && runtime.relayUrl === canonical)),
  );
}
