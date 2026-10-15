import type { ManagedAgentRuntimeStatus } from "@/shared/api/types";

/**
 * The one word a status pill shows for an agent in this community.
 *
 * `Here` is a working agent: a local runtime that is ready, or a host-managed
 * pair with a signed execution lease. The host-managed words below it each
 * name a different wire fact (NIP-CSL § Provider presence), so a person can
 * tell at a glance which of "idle but here", "signed stopped under a live
 * provider", "provider live, execution word missing", "provider not heard
 * from" and "host up, execution not running" they are looking at — states
 * the old single `Unavailable` collapsed. `Unavailable` remains for a local
 * runtime that stopped or failed, and for a host-managed pair whose backend
 * reported no presence at all (unconfirmed, not disconnected).
 *
 * "Provider not heard from" is deliberately not "unreachable": an expired or
 * never-seen heartbeat says only that no heartbeat arrived within its TTL.
 * Nothing tested reachability, so the word does not claim it.
 */
export type AgentCommunityAvailability =
  | "Here"
  | "Idle"
  | "Stopped"
  | "Execution not reported"
  | "Provider not heard from"
  | "Not running"
  | "Waking"
  | "Needs setup on this device"
  | "Unavailable";
// Host ownership alone does not certify runtime/account availability.

/** The `Badge` variant that reads truthfully for each availability word. */
export type AgentCommunityBadgeVariant =
  | "default"
  | "success"
  | "info"
  | "warning"
  | "secondary"
  | "destructive";

/**
 * Only an expired or absent heartbeat reads as not heard from; a
 * null/undefined `providerPresence` is a backend that did not report, which
 * is unconfirmed. Neither is a reachability test.
 */
export function hostManagedPresenceLost(
  runtime: Pick<
    ManagedAgentRuntimeStatus,
    "runtimeAvailable" | "providerPresence"
  >,
): boolean {
  return (
    runtime.runtimeAvailable !== true &&
    (runtime.providerPresence === "expired" ||
      runtime.providerPresence === "none")
  );
}

/**
 * A host-managed pair with no live execution lease is idle, not gone, only
 * when the provider's presence heartbeat is live *and the execution word
 * says so* (`idle`, or `live` while the lease has not been verified). A live
 * heartbeat whose `liveTargets` does not include it (`execution: "absent"`)
 * is a provider that is up without this execution — never idle. A signed
 * `stopped` word is Stopped, not idle, and no word at all is reported as
 * missing rather than read as idle: the Tauri fold returns no execution
 * word when the rows carry none (`headless_management.rs`), and "Idle"
 * with a Stop button over that would be a guess.
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
    (runtime.execution === "idle" || runtime.execution === "live")
  );
}

/**
 * The provider's heartbeat is live and this pair's last execution is signed
 * stopped: the provider is here, the execution is not, and Start is the
 * only honest action.
 */
export function hostManagedStoppedUnderLivePresence(
  runtime: Pick<
    ManagedAgentRuntimeStatus,
    "runtimeAvailable" | "providerPresence" | "execution"
  >,
): boolean {
  return (
    runtime.runtimeAvailable !== true &&
    runtime.providerPresence === "live" &&
    runtime.execution === "stopped"
  );
}

/**
 * The provider's heartbeat is live but the status row carries no execution
 * word for this pair. The backend reported presence and nothing about the
 * execution, so the pill says exactly that and offers no Stop.
 */
export function hostManagedExecutionUnreported(
  runtime: Pick<
    ManagedAgentRuntimeStatus,
    "runtimeAvailable" | "providerPresence" | "execution"
  >,
): boolean {
  return (
    runtime.runtimeAvailable !== true &&
    runtime.providerPresence === "live" &&
    (runtime.execution === null || runtime.execution === undefined)
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
    if (hostManagedIdleWithLivePresence(runtime)) return "Idle";
    if (hostManagedStoppedUnderLivePresence(runtime)) return "Stopped";
    if (hostManagedExecutionUnreported(runtime))
      return "Execution not reported";
    if (hostManagedAbsentFromLivePresence(runtime)) return "Not running";
    if (hostManagedPresenceLost(runtime)) return "Provider not heard from";
    return "Unavailable";
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

/**
 * The `Badge` variant for an availability word. Exported beside the word so
 * every pill that shows one colours it the same way: primary for working,
 * green for idle-but-here, amber for a provider nobody has heard from, muted
 * for an execution that is stopped, not running, or not reported, and for a
 * stopped local runtime, red only for a local runtime that failed.
 */
export function agentCommunityBadgeVariant(
  runtime: ManagedAgentRuntimeStatus,
): AgentCommunityBadgeVariant {
  switch (agentCommunityAvailability(runtime)) {
    case "Here":
      return "default";
    case "Idle":
      return "success";
    case "Waking":
      return "info";
    case "Provider not heard from":
      return "warning";
    case "Needs setup on this device":
    case "Stopped":
    case "Execution not reported":
    case "Not running":
      return "secondary";
    case "Unavailable":
      return runtime.lifecycle === "failed" ? "destructive" : "secondary";
  }
}

/**
 * The detail sentence under the pill. Every sentence names the evidence it
 * rests on and nothing more: the host's status row carries the presence
 * *verdict* (`live`/`expired`/`none`), not the heartbeat's sequence or
 * TTL, so a lost heartbeat is reported as expired and never as "last seen
 * N seconds ago" — that number is not on this wire.
 */
/**
 * What Start does for a pair the host already holds. True by construction:
 * `beekeeper_agent_management::start` (lifecycle.rs) takes the management
 * lock, and for an adopted pair returns `already_adopted: true,
 * execution_created: false` after observing status — it never creates a
 * second execution for a held pair, so a Start pressed while the provider is
 * merely slow to heartbeat duplicates nothing.
 */
const START_IS_FENCED =
  "Start is safe: the host reconciles a pair it already holds and does not create a second execution.";

export function agentCommunityStatusDetail(
  runtime: ManagedAgentRuntimeStatus,
): string | null {
  if (runtime.lifecycle === "host_managed") {
    if (runtime.runtimeAvailable === true)
      return "Managed by the host. Working: a signed relay lease proves an execution is live.";
    if (hostManagedIdleWithLivePresence(runtime))
      return "Managed by the host. Here and idle: the host's presence heartbeat is live and no execution is running.";
    if (hostManagedStoppedUnderLivePresence(runtime))
      return "Managed by the host. Stopped: the provider is live; this agent's execution is signed stopped. Start creates a new one.";
    if (hostManagedExecutionUnreported(runtime))
      return "Managed by the host. Execution not reported: the provider reported its presence but no execution state for this agent, so nothing here can be stopped.";
    if (hostManagedAbsentFromLivePresence(runtime))
      return "Managed by the host. Not running: the host is up but reports no execution for this agent. Start creates one.";
    if (runtime.providerPresence === "expired")
      return `Managed by the host. Provider not heard from: its last presence heartbeat expired and no execution lease is live. ${START_IS_FENCED}`;
    if (runtime.providerPresence === "none")
      return `Managed by the host. Provider not heard from: no presence heartbeat has been seen from it and no execution lease is live. ${START_IS_FENCED}`;
    if (runtime.runtimeAvailable === false)
      return "Managed by the host. No current verified execution lease is available in this community, and the host did not report its presence.";
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
 * Missing availability is not termination proof; Start preserves backend
 * fences (`START_IS_FENCED`). Stop is offered only over a verified lease or
 * an idle word under a live heartbeat — never over a signed stopped
 * execution or a missing execution word. */
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
