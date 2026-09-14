import { invokeTauri } from "@/shared/api/tauri";

/**
 * The active relay's own machine health from `GET /health/system`, untyped:
 * the relay owns the shape and `features/dashboard/lib/relaySystemHealth.ts`
 * validates it. The relay answers its community's stewards only; anyone
 * else gets an error whose message starts with `relay returned 403`, and a
 * relay predating the endpoint answers `relay returned 404`.
 */
export async function getRelaySystemHealth(): Promise<unknown> {
  return invokeTauri<unknown>("get_relay_system_health");
}
