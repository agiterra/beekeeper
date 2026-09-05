/**
 * Host deps for a seated create: the real membership, custody and pack-source
 * calls behind {@link SeatedCodingSessionCreateDeps}, as one module-level
 * object so the submit callback that closes over it stays stable. Split from
 * `useNewCodingSessionCreate.ts` for size.
 */
import { ensureActorChannelMembership } from "./actorSeatChannelMembership";
import {
  clearCodingSessionActorSeat,
  stageCodingSessionActorSeat,
} from "./codingSessionActorSeatCustody";
import type { SeatedCodingSessionCreateDeps } from "./codingSessionSeatedCreate";
import { fetchCodingSessionSeatPackSource } from "./codingSessionSeatPackSource";

export const codingSessionSeatDeps: SeatedCodingSessionCreateDeps = {
  ensureMembership: (seatInput: {
    channelId: string;
    actorPubkey: string;
    actorLabel: string | null;
  }) => ensureActorChannelMembership(seatInput),
  stageSeat: stageCodingSessionActorSeat,
  clearSeat: clearCodingSessionActorSeat,
  // The project's 30624, read the way the launch dialog's preview reads
  // it, so the seat is staged from the repository the dialog named.
  fetchPackSource: fetchCodingSessionSeatPackSource,
};
