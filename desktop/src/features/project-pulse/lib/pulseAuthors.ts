/**
 * Names, not hashes.
 *
 * `VISION_ACTIVITY.md` is explicit that a coordination surface shows a person
 * and a filename, never a raw pubkey or event id. The fold speaks in hex
 * because hex is what the wire carries; everything between the fold and the
 * screen goes through here, so a pubkey renders as its author's name and the
 * hex survives only in a `title` a reader can copy.
 *
 * Pure, so the view can be unit-tested with an explicit name map and the real
 * screen can fill the same map from `useUsersBatchQuery`.
 */
import { truncatePubkey } from "@/shared/lib/pubkey";

import type { ProjectPulseDigest, PulseDigestEntry } from "./pulseFold.ts";

/** Resolved display names, keyed by lowercase pubkey. */
export type PulseAuthorNames = ReadonlyMap<string, string>;

/**
 * Every pubkey the Pulse of one project renders: entry authors, plus the
 * authors named by supersession claims (an unresolved claim carries `null` and
 * contributes nothing — there is no author to name).
 */
export function pulseDigestPubkeys(
  digest: ProjectPulseDigest | null,
): string[] {
  if (!digest) return [];
  const pubkeys = new Set<string>();
  for (const entry of digest.entries) {
    pubkeys.add(entry.pubkey.toLowerCase());
    for (const claim of entry.supersededBy) {
      if (claim.pubkey) pubkeys.add(claim.pubkey.toLowerCase());
    }
  }
  return [...pubkeys].sort();
}

/**
 * The name to print for one author. Falls back to the truncated pubkey — the
 * same fallback the social Pulse's `NoteCard` uses — so an unresolved profile
 * degrades to a short hash rather than to a blank or a guess.
 */
export function pulseAuthorLabel(
  pubkey: string,
  names?: PulseAuthorNames,
): string {
  return names?.get(pubkey.toLowerCase()) ?? truncatePubkey(pubkey);
}

/**
 * Who retired this entry, by name — the fact behind a superseded row's
 * `Replaced by …` line.
 *
 * A retired entry that renders only as a dimmed card states the outcome and
 * hides the actor; this is the actor. An honored claim always carries its
 * author (the fold only honors same-author claims it resolved), so an empty
 * result means the entry is not retired at all.
 */
export function honoredSupersessionAuthors(
  entry: PulseDigestEntry,
  names?: PulseAuthorNames,
): string[] {
  return entry.supersededBy
    .filter((claim) => claim.honored && claim.pubkey !== null)
    .map((claim) => pulseAuthorLabel(claim.pubkey as string, names));
}
