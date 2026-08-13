import type {
  CodingSessionCatalogRecord,
  CodingSessionCatalogSnapshot,
  CodingSessionWorkspaceStatus,
} from "./codingSessionTypes";
import { deriveCodingSessionWorkspaceStatus } from "./codingSessionWorkspaceModel";

export type ChannelCodingSessionIngressEntry = {
  session: CodingSessionCatalogRecord;
  status: CodingSessionWorkspaceStatus;
};

/**
 * Resolve the sessions that may be offered from a channel's own chrome.
 *
 * The snapshot-channel check prevents a render during channel transitions from
 * exposing the previous channel's generations. Authority failures are
 * fail-closed; malformed, rejected-author, and invalid-signature events never
 * enter `catalog.entries` in the first place.
 */
export function resolveChannelCodingSessionIngress(input: {
  activeChannelId: string | null;
  catalog: CodingSessionCatalogSnapshot;
}): ChannelCodingSessionIngressEntry[] {
  if (
    !input.activeChannelId ||
    input.catalog.channelId !== input.activeChannelId ||
    input.catalog.authorityErrorMessage
  ) {
    return [];
  }

  return input.catalog.entries.map((session) => ({
    session,
    status: deriveCodingSessionWorkspaceStatus(
      session.transcript,
      session.status,
    ),
  }));
}
