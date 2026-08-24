import { invokeTauri } from "@/shared/api/tauri";

/**
 * Host status of the first-party coding-session provider, for the active
 * community relay.
 *
 * Mirrors the Rust `CodingSessionProviderStatus` struct in
 * `desktop/src-tauri/src/session_provider/supervisor.rs`.
 */
export type CodingSessionProviderStatus = {
  /** A provider identity has been minted for this relay. */
  provisioned: boolean;
  /**
   * The desktop is supervising a provider process right now.
   *
   * True while a restart backoff is in flight too: the supervisor owns that
   * transient gap, so a caller should not react to it.
   */
  running: boolean;
  /** Provider identity pubkey. Absent until provisioned. */
  providerPubkey?: string;
  /** Stable `cs-target` instance id. Absent until provisioned. */
  instanceId?: string;
};

/** Live model selections exposed by one of this computer's runtime adapters. */
export type CodingSessionProviderModels = {
  /** Echoes the runtime instance the models belong to. */
  instanceRef: string;
  defaultModel: string;
  allowedModels: string[];
};

/**
 * Whether a host runtime can serve a session right now.
 *
 * `needs_auth` and `missing` runtimes still appear in the create flow —
 * disabled, with honest remediation — because hiding them would make an
 * installed-but-signed-out runtime indistinguishable from one that does not
 * exist.
 */
export type CodingSessionRuntimeAuthState = "ready" | "needs_auth" | "missing";

/** Capability vector one runtime advertises before its catalog is published. */
export type CodingSessionProviderRuntimeCapabilities = {
  threadTurnStart: boolean;
  threadTurnInterrupt: boolean;
  threadSteer: boolean;
  context: boolean;
  diff: boolean;
  plan: boolean;
};

/**
 * One agent runtime this computer's coding-session provider offers.
 *
 * Mirrors the Rust host runtime table in
 * `desktop/src-tauri/src/session_provider/runtimes.rs`. Every table row is
 * returned, including uninstalled ones, sorted by `instanceRef`.
 */
export type CodingSessionProviderRuntime = {
  /** Catalog coordinate a 44221 create names, e.g. `claude-primary`. */
  instanceRef: string;
  /** Runtime slug: `claude`, `codex`, `goose`. */
  runtime: string;
  /** Driver slug minted into every cs-target for this runtime's sessions. */
  driver: string;
  /** Human label from the managed-agent registry, e.g. `Claude Code`. */
  label: string;
  authState: CodingSessionRuntimeAuthState;
  /** Static default (`default`); live models come from the models command. */
  defaultModel: string;
  allowedModels: string[];
  capabilities: CodingSessionProviderRuntimeCapabilities;
};

/** This computer's ceiling on live agent processes, and what is in force. */
export type CodingSessionCapacitySettings = {
  /** Stored ceiling: `null` for the provider default, `0` for unlimited. */
  maxSessions: number | null;
  /** The provider's own default, so no surface has to hardcode the number. */
  defaultMaxSessions: number;
  /** What the *running* provider started with, when one is running. */
  runningMaxSessions: number | null;
};

/** Read the stored session ceiling alongside the one being enforced. */
export async function getCodingSessionCapacity(): Promise<CodingSessionCapacitySettings> {
  return invokeTauri<CodingSessionCapacitySettings>(
    "coding_session_capacity_settings",
  );
}

/**
 * Store a session ceiling. `null` restores the provider default, `0` removes
 * the ceiling entirely.
 *
 * The provider reads its ceiling from the environment at startup, so this
 * takes effect the next time it starts — callers must say so rather than
 * implying the new number is already being enforced.
 */
export async function setCodingSessionCapacity(
  maxSessions: number | null,
): Promise<CodingSessionCapacitySettings> {
  return invokeTauri<CodingSessionCapacitySettings>(
    "set_coding_session_capacity",
    { maxSessions },
  );
}

/** Read the provider's provisioning and supervision state. */
export async function getCodingSessionProviderStatus(): Promise<CodingSessionProviderStatus> {
  return invokeTauri<CodingSessionProviderStatus>(
    "coding_session_provider_status",
  );
}

/**
 * Discover the models one runtime accepts.
 *
 * `claude-primary` (the default when `instanceRef` is absent) probes the live
 * adapter; runtimes without model discovery answer with their static defaults
 * without spawning anything.
 */
export async function getCodingSessionProviderModels(
  instanceRef?: string,
): Promise<CodingSessionProviderModels> {
  return invokeTauri<CodingSessionProviderModels>(
    "coding_session_provider_models",
    instanceRef === undefined ? undefined : { instanceRef },
  );
}

/**
 * List every runtime this computer's provider knows how to run, with its
 * installation and sign-in state. Fast — auth probes are CLI exit-code checks,
 * never ACP adapter spawns.
 */
export async function getCodingSessionProviderRuntimes(): Promise<
  CodingSessionProviderRuntime[]
> {
  return invokeTauri<CodingSessionProviderRuntime[]>(
    "coding_session_provider_runtimes",
  );
}

/**
 * Mint a provider identity for the active relay, add it to the bridge trust
 * allowlist, and start it.
 *
 * Idempotent — an existing identity is reused, never replaced. Re-minting would
 * orphan every coding-session event already attributed to the old pubkey.
 */
export async function provisionCodingSessionProvider(): Promise<CodingSessionProviderStatus> {
  return invokeTauri<CodingSessionProviderStatus>(
    "provision_coding_session_provider",
  );
}

/** Start the provisioned provider if it is not already supervised. */
export async function ensureCodingSessionProviderRunning(): Promise<CodingSessionProviderStatus> {
  return invokeTauri<CodingSessionProviderStatus>(
    "ensure_coding_session_provider_running",
  );
}

/**
 * Stop the supervised provider.
 *
 * The identity and its state directory survive, so a later start resumes the
 * same provider with its durable outbox intact.
 */
export async function stopCodingSessionProvider(): Promise<CodingSessionProviderStatus> {
  return invokeTauri<CodingSessionProviderStatus>(
    "stop_coding_session_provider",
  );
}
