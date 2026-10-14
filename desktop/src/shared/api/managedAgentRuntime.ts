// Host-managed agent runtime status as the Tauri backend reports it
// (`desktop/src-tauri/src/managed_agents/runtime_types.rs`). Split out of
// `types.ts`, which re-exports these, to keep that file under the repository
// file-size ceiling.

export type ManagedAgentRuntimeLifecycle =
  | "starting"
  | "listening"
  | "waking"
  | "ready"
  | "host_managed"
  | "failed"
  | "stopped";

/** What the provider's presence heartbeats say (NIP-CSL § Provider presence). */
export type ProviderPresence = "live" | "expired" | "none";

/** `beekeeper-agent-management`'s `execution` word for one responsibility row. */
export type ManagedAgentExecutionWord =
  | "live"
  | "idle"
  | "absent"
  | "disconnected"
  | "stopped";

export type ManagedAgentRuntimeStatus = {
  pubkey: string;
  /** Exact submitted descriptor, present only on startup reconcile results. */
  requestedRelayUrl?: string;
  /** Canonical, backend-owned pair identity component. Do not normalize in TS. */
  relayUrl: string;
  localSetup: boolean;
  lifecycle: ManagedAgentRuntimeLifecycle;
  /** Verified signed relay availability; null/absent means unconfirmed. */
  runtimeAvailable?: boolean | null;
  /**
   * The provider instance's kind-24224 presence heartbeat, from the
   * management `status` row (`providerPresence`): `live` means the provider
   * is running and reachable even with no execution lease (idle); `expired`
   * or `none` is the only evidence that reads as disconnected. Null/absent
   * means the backend did not report it.
   */
  providerPresence?: ProviderPresence | null;
  /**
   * The management `execution` word folded across the pair's
   * responsibilities (`live`/`idle`/`absent`/`disconnected`/`stopped`).
   * `absent` is the one presence cannot express alone: the provider's
   * heartbeat is live but names none of this pair's executions — the host
   * is up and this agent's execution is not among what it runs. Never read
   * as idle. Null/absent means the backend did not report it.
   */
  execution?: ManagedAgentExecutionWord | null;
  pid: number | null;
  error: string | null;
  logPath: string | null;
};
