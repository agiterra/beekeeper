import * as React from "react";

import { useUsersBatchQuery } from "@/features/profile/hooks";
import { resolveUserLabel } from "@/features/profile/lib/identity";

import { pulseDigestPubkeys, type PulseAuthorNames } from "../lib/pulseAuthors";
import type { ProjectPulseDigest } from "../lib/pulseFold.ts";

/**
 * Resolve every author a Pulse digest names, using the same batch profile read
 * the social Pulse's `NoteCard` path uses (`useUsersBatchQuery`).
 *
 * Deliberately a hook over the digest rather than a lookup inside the row: the
 * presentational components stay pure so `ProjectPulseView` can be rendered in
 * a plain unit test with an explicit name map, and one batched query serves the
 * whole screen instead of one query per row.
 */
export function usePulseAuthorNames(
  digest: ProjectPulseDigest | null,
): PulseAuthorNames {
  const pubkeys = React.useMemo(() => pulseDigestPubkeys(digest), [digest]);
  const profiles = useUsersBatchQuery(pubkeys).data?.profiles;
  return React.useMemo(() => {
    const names = new Map<string, string>();
    for (const pubkey of pubkeys) {
      // No `currentPubkey`: "You" reads as a self-reference in a list whose
      // whole job is telling two people apart, and the entry may equally be
      // this operator's agent rather than the operator.
      names.set(pubkey, resolveUserLabel({ pubkey, profiles }));
    }
    return names;
  }, [profiles, pubkeys]);
}
