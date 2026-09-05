import { formatAge } from "@/features/roles/ui/rolesCopy";
import { truncatePubkey } from "@/shared/lib/pubkey";

import type { ContributorRow } from "../lib/contributorsModel";

/**
 * Every sentence the Contributors tab shows, as values a test can assert.
 *
 * Only two membership states have evidence tonight (ruling §1): a row this
 * computer can currently see holding a seat reads `Seated · <status> ·
 * <age>`; everything else reads `Past contributor`. Neither Eligible nor
 * Offered appears anywhere here — no consent record exists on the wire.
 */

export const CONTRIBUTORS_TITLE = "Contributors";

export const CONTRIBUTORS_SUBTITLE =
  "Seat history for this project — who has held a seat here, and when it was last observed. This is not a list of who is available.";

/** A project id the route named that this reader cannot read. */
export const CONTRIBUTORS_PROJECT_MISSING =
  "This project is not readable here.";

export const CONTRIBUTORS_LOADING = "Reading seat history…";

export const CONTRIBUTORS_EMPTY = "No agent has held a seat on this project.";

export const CONTRIBUTOR_ROLES_EMPTY = "no role";

export const PAST_CONTRIBUTOR = "Past contributor";

/**
 * Landings (push records / gate rows) are not counted on this surface: they
 * are read per session channel through a subscription this fold does not
 * hold, and the shelf this fold reads carries no push or gate data.
 */
export const CONTRIBUTORS_LANDINGS_NOTE =
  "Landings are not counted here yet: they are recorded per session channel, and this list does not subscribe to them.";

/** Last line on every render of this surface, loaded or not. */
export const CONTRIBUTORS_FOOTNOTE =
  "Offers and eligibility are not recorded yet.";

/** The shelf's own sentence, joined the way every other seat surface joins it. */
export function contributorsNoticeSentence(
  message: string,
  detail: string | null,
): string {
  return detail ? `${message} — ${detail}` : message;
}

/** `{n} seats` / `1 seat`. */
export function contributorSeatsText(seatCount: number): string {
  return `${seatCount} ${seatCount === 1 ? "seat" : "seats"}`;
}

/** Role slugs joined ` · `; none is disclosed, not left blank. */
export function contributorRolesText(roles: readonly string[]): string {
  return roles.length > 0 ? roles.join(" · ") : CONTRIBUTOR_ROLES_EMPTY;
}

/**
 * The only two Last-seen strings this surface may render (ruling's evidence
 * table): a seat this computer can currently see reads its status and age;
 * anything else — a closed seat, or a seat this fold no longer sees open —
 * reads `Past contributor`, never a duration of absence.
 */
export function contributorLastSeenText(
  row: Pick<ContributorRow, "isSeatedNow" | "lastStatus" | "lastAgeSeconds">,
): string {
  if (!row.isSeatedNow) return PAST_CONTRIBUTOR;
  return `Seated · ${row.lastStatus} · ${formatAge(row.lastAgeSeconds)}`;
}

/** `data-contributor-state`: the only two values this surface renders. */
export function contributorStateAttr(
  row: Pick<ContributorRow, "isSeatedNow">,
): "seated" | "past" {
  return row.isSeatedNow ? "seated" : "past";
}

/**
 * The row's display name: the managed agent's name, or its short pubkey.
 *
 * The short form is the app's canonical `abcd1234…wxyz`, not a hand-rolled
 * prefix. A truncated pubkey is a recognition aid and a ground-out prefix
 * forges one cheaply, so every surface must truncate the same way — the full
 * key stays in the cell's `title`.
 */
export function contributorNameText(
  row: Pick<ContributorRow, "name" | "agentPubkey">,
): string {
  return row.name ?? truncatePubkey(row.agentPubkey);
}
