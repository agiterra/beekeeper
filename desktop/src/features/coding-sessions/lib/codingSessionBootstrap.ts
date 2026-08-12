import type { RelayEvent } from "@/shared/api/types";

export const CODING_SESSION_POPOUT_BOOTSTRAP_SCHEMA =
  "buzz-coding-session-popout-bootstrap/v1";

const MAX_CACHED_GENERATIONS = 64;

export type CodingSessionPopoutBootstrap = {
  schema: typeof CODING_SESSION_POPOUT_BOOTSTRAP_SCHEMA;
  channelId: string;
  generationId: string;
  authorityIdentity: string;
  relayEvents: RelayEvent[];
};

const acceptedBootstrapByCoordinate = new Map<
  string,
  CodingSessionPopoutBootstrap
>();

function coordinateKey(channelId: string, generationId: string): string {
  return JSON.stringify([channelId, generationId]);
}

/**
 * Remember the raw signed relay events behind an accepted catalog generation.
 *
 * The donor filled this cache from its kind-9 projection store; here the
 * source is `TrustedCodingSessionIngressStore.retainedRawEvents`, which keeps
 * the verified 442xx bytes per generation for exactly this purpose.
 *
 * This cache is process-local to the current webview. A pop-out stages one
 * exact entry in Tauri's ephemeral process state; no transcript is written to
 * disk and the receiving webview re-runs the normal signature and authority
 * checks before rendering it.
 */
export function rememberCodingSessionPopoutBootstrap(input: {
  channelId: string;
  generationId: string;
  authorityIdentity: string | null;
  relayEvents: readonly RelayEvent[];
}): void {
  if (!input.authorityIdentity || input.relayEvents.length === 0) return;
  const key = coordinateKey(input.channelId, input.generationId);
  acceptedBootstrapByCoordinate.delete(key);
  acceptedBootstrapByCoordinate.set(key, {
    schema: CODING_SESSION_POPOUT_BOOTSTRAP_SCHEMA,
    channelId: input.channelId,
    generationId: input.generationId,
    authorityIdentity: input.authorityIdentity,
    relayEvents: [...input.relayEvents],
  });
  while (acceptedBootstrapByCoordinate.size > MAX_CACHED_GENERATIONS) {
    const oldestKey = acceptedBootstrapByCoordinate.keys().next().value;
    if (typeof oldestKey !== "string") break;
    acceptedBootstrapByCoordinate.delete(oldestKey);
  }
}

/** Return an isolated copy of the exact accepted generation, if cached. */
export function getCodingSessionPopoutBootstrap(
  channelId: string,
  generationId: string,
): CodingSessionPopoutBootstrap | null {
  const cached = acceptedBootstrapByCoordinate.get(
    coordinateKey(channelId, generationId),
  );
  if (!cached) return null;
  return {
    ...cached,
    relayEvents: [...cached.relayEvents],
  };
}

/** Drop every remembered generation. Community switches must not leak. */
export function resetCodingSessionPopoutBootstrapCache(): void {
  acceptedBootstrapByCoordinate.clear();
}

/** Strictly decode an ephemeral bootstrap returned by the native process. */
export function parseCodingSessionPopoutBootstrap(
  value: unknown,
  expected: { channelId: string; generationId: string },
): CodingSessionPopoutBootstrap | null {
  if (!isRecord(value)) return null;
  if (value.schema !== CODING_SESSION_POPOUT_BOOTSTRAP_SCHEMA) return null;
  if (
    value.channelId !== expected.channelId ||
    value.generationId !== expected.generationId ||
    typeof value.authorityIdentity !== "string" ||
    value.authorityIdentity.length === 0 ||
    !Array.isArray(value.relayEvents)
  ) {
    return null;
  }
  return {
    schema: CODING_SESSION_POPOUT_BOOTSTRAP_SCHEMA,
    channelId: expected.channelId,
    generationId: expected.generationId,
    authorityIdentity: value.authorityIdentity,
    // Relay events intentionally remain unknown-shaped at this boundary. The
    // strict trusted-ingress classifier verifies their structure, signature,
    // channel, generation, and configured provider authority before anything
    // is rendered.
    relayEvents: value.relayEvents as RelayEvent[],
  };
}

/** Load the exact snapshot staged for this native pop-out window. */
export async function loadCodingSessionPopoutBootstrap(
  label: string,
  expected: { channelId: string; generationId: string },
): Promise<CodingSessionPopoutBootstrap | null> {
  const { invoke } = await import("@tauri-apps/api/core");
  const value = await invoke<unknown>("get_coding_session_popout_bootstrap", {
    label,
    channelId: expected.channelId,
    generationId: expected.generationId,
  });
  return parseCodingSessionPopoutBootstrap(value, expected);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
