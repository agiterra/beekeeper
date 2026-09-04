/**
 * Community icon, Buzz extension to NIP-43 + standard NIP-11 `icon`.
 *
 * An admin/owner publishes a kind:9033 command carrying the icon in an
 * `["icon", value]` tag; the relay validates the sender's relay role and
 * stores the icon per community, serving it in the standard `icon` field of
 * its NIP-11 relay information document. Every member's client reads NIP-11,
 * so the whole community sees the same icon.
 *
 * The icon value is a small `data:image/*` URL (downscaled client-side
 * before publish) so it renders for INACTIVE communities straight from the
 * document — no cross-relay media fetch behind another relay's auth wall.
 */

import { relayClient } from "@/shared/api/relayClient";
import { invokeTauri, signRelayEvent } from "@/shared/api/tauri";

/** Buzz: admin command to set the community profile (icon). */
export const KIND_SET_COMMUNITY_PROFILE = 9033;

/**
 * Fetch a community's icon from its relay's NIP-11 document (plain
 * unauthenticated HTTP via the Tauri backend — works for inactive
 * communities too). Unreachable relay or no icon → null.
 */
export async function fetchCommunityIcon(
  relayUrl: string,
): Promise<string | null> {
  const icon = await invokeTauri<string | null>("fetch_workspace_icon", {
    relayUrl,
  });
  return icon || null;
}

/**
 * Fetch a relay's own disclosed build commit from its NIP-11 document
 * (`software_commit`, finding 32 —
 * `review-2026-09-01/LIVE-RUN-TeamRolesV1.md`). Plain unauthenticated HTTP
 * via the Tauri backend, same shape as {@link fetchCommunityIcon} — works for
 * an inactive community too, since `EditCommunityDialog` may be editing one
 * that is not the currently active workspace.
 *
 * Returns the full 40-hex commit, or `null` for every case that discloses as
 * "unknown": unreachable relay, malformed document, or a relay predating this
 * field. Callers that display it should truncate for the UI (8 hex, matching
 * `bee git check --ref`'s own truncation) rather than treat `null` as an
 * error.
 */
export async function fetchRelayBuildCommit(
  relayUrl: string,
): Promise<string | null> {
  return invokeTauri<string | null>("get_relay_build_commit", { relayUrl });
}

/** What a relay discloses about the build serving a request (NIP-11). */
export type RelayBuildIdentity = {
  /** Full 40-hex commit, or `null` for every case that reads as "unknown". */
  commit: string | null;
  /** `git rev-list --count` of {@link commit}, or `null`. */
  commitCount: number | null;
  /** RFC 3339 UTC build stamp, or `null`. */
  buildTime: string | null;
  /** The repository URL the relay names as its own source. */
  software: string | null;
};

/**
 * Fetch a relay's full disclosed build identity in one round trip.
 *
 * {@link fetchRelayBuildCommit} is a projection of this and stays for the
 * callers that only want the commit. Every field is independently `null`:
 * a relay predating a field, or one that could not determine it, answers
 * `null` rather than reading as unreachable — so `null` is a disclosed
 * non-answer, never an error to retry.
 */
export async function fetchRelayBuildIdentity(
  relayUrl: string,
): Promise<RelayBuildIdentity> {
  return invokeTauri<RelayBuildIdentity>("get_relay_build_identity", {
    relayUrl,
  });
}

/**
 * Publish a kind:9033 command setting (or clearing, with "") the community
 * icon on the active relay. Requires relay admin/owner role — the relay
 * rejects the command otherwise.
 */
export async function setCommunityIcon(icon: string): Promise<void> {
  const event = await signRelayEvent({
    kind: KIND_SET_COMMUNITY_PROFILE,
    content: "",
    tags: [["icon", icon]],
  });
  await relayClient.publishEvent(
    event,
    "Timed out while updating the community icon.",
    "Failed to update the community icon.",
  );
}
