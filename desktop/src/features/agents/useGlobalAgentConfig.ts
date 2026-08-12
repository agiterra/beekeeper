/**
 * React hook: load the global agent configuration defaults.
 *
 * Backed by TanStack Query with a stable query key so the config is fetched
 * once per QueryClient lifetime and shared across all callers — dialogs always
 * receive the already-populated value on first render, eliminating the
 * per-mount IPC race that caused required-env-key rows to be missing on open.
 *
 * On fetch error the query falls back to EMPTY_CONFIG (safe — the absence of
 * a global config is never an error state for callers).
 */
import { useQuery, type QueryClient } from "@tanstack/react-query";

import { getGlobalAgentConfig } from "@/shared/api/tauriGlobalAgentConfig";
import type { GlobalAgentConfig } from "@/shared/api/types";

const EMPTY_CONFIG: GlobalAgentConfig = {
  env_vars: {},
  provider: null,
  model: null,
  preferred_runtime: null,
  // Fail-closed: with no persisted config, no signer is trusted.
  "allowed-bridge-pubkeys": [],
};

export const globalAgentConfigQueryKey = ["globalAgentConfig"] as const;

export function useGlobalAgentConfig(): {
  globalConfig: GlobalAgentConfig;
  isLoading: boolean;
} {
  const { data, isPending } = useQuery({
    queryKey: globalAgentConfigQueryKey,
    queryFn: getGlobalAgentConfig,
    // Config is only mutated via setGlobalAgentConfig — treat as stable until
    // `publishSavedGlobalAgentConfig` (below) invalidates it after a save.
    staleTime: Number.POSITIVE_INFINITY,
    // Never show a stale empty flash while a background refetch runs.
    placeholderData: EMPTY_CONFIG,
  });

  return {
    globalConfig: data ?? EMPTY_CONFIG,
    isLoading: isPending,
  };
}

/** The two cache operations a save needs. Satisfied by a real `QueryClient`. */
export type GlobalAgentConfigCache = Pick<
  QueryClient,
  "setQueryData" | "invalidateQueries"
>;

/**
 * Publish a just-saved global config to every reader of the shared query.
 *
 * Two steps, and both are load-bearing:
 *
 * 1. `setQueryData` hands mounted consumers the backend's canonical config
 *    synchronously, so no dialog has to wait out a second IPC round-trip.
 * 2. `invalidateQueries` is what makes the on-disk file authoritative again.
 *    `staleTime` is `Infinity`, so without it this cache entry is fresh
 *    forever and the query function never runs a second time in the app's
 *    lifetime. That matters here because the desktop writes this same file
 *    from Rust behind the UI's back — `session_provider/trust.rs` appends the
 *    local provider to `allowed-bridge-pubkeys` on every provider start. A
 *    seed-only save would leave the settings surface showing a trust list that
 *    disagrees with the one the consumer actually enforces, and the
 *    disagreement would survive until the app restarted.
 */
export function publishSavedGlobalAgentConfig(
  cache: GlobalAgentConfigCache,
  config: GlobalAgentConfig,
): void {
  cache.setQueryData(globalAgentConfigQueryKey, config);
  void cache.invalidateQueries({ queryKey: globalAgentConfigQueryKey });
}
