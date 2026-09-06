import { useQuery } from "@tanstack/react-query";

import { getBakedBuildEnv, getBakedBuildEnvKeys } from "@/shared/api/tauri";

export const bakedBuildEnvKeysQueryKey = ["baked-build-env-keys"] as const;
export const bakedBuildEnvQueryKey = ["baked-build-env"] as const;

/**
 * Query safely displayable baked build env entries. The backend masks secrets,
 * so this is only used for inherited provider/model/effort labels.
 */
export function useBakedBuildEnvQuery(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: bakedBuildEnvQueryKey,
    queryFn: () => getBakedBuildEnv(),
    enabled: options?.enabled ?? true,
    staleTime: Infinity,
    refetchInterval: false,
    retry: false,
  });
}

/**
 * Query the key names of baked build env vars.
 *
 * Internal (Block) builds bake provider credentials into the binary at compile
 * time. This query returns the *key names only* so dialogs can treat baked keys
 * as satisfying their requirements — mirroring the backend readiness gate.
 *
 * The value is a compile-time constant, so `staleTime: Infinity` is correct.
 * In web-dev and E2E contexts where the Tauri command doesn't exist the query
 * fails soft and resolves to `undefined` without crashing (same class as
 * `useRuntimeFileConfigQuery`).
 */
export function useBakedBuildEnvKeysQuery(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: bakedBuildEnvKeysQueryKey,
    queryFn: () => getBakedBuildEnvKeys(),
    enabled: options?.enabled ?? true,
    staleTime: Infinity,
    refetchInterval: false,
    retry: false,
  });
}
