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

/** Live model selections exposed by this computer's Claude Code adapter. */
export type CodingSessionProviderModels = {
  defaultModel: string;
  allowedModels: string[];
};

/** Read the provider's provisioning and supervision state. */
export async function getCodingSessionProviderStatus(): Promise<CodingSessionProviderStatus> {
  return invokeTauri<CodingSessionProviderStatus>(
    "coding_session_provider_status",
  );
}

/** Discover every Claude Code model selectable through the installed adapter. */
export async function getCodingSessionProviderModels(): Promise<CodingSessionProviderModels> {
  return invokeTauri<CodingSessionProviderModels>(
    "coding_session_provider_models",
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
