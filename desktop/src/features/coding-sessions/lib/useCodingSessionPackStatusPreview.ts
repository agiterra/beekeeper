import * as React from "react";

import {
  codingSessionPackStatus,
  type CodingSessionPackStatusResult,
} from "@/features/coding-sessions/lib/codingSessionPackStatus";

/**
 * The seat field's pack preview, resolved (or not) for the current project
 * and role (LANE-L23).
 *
 * `null` covers every reason there is nothing to show: no project is known
 * yet, no agent has been seated, no role has been typed, the probe is still in
 * flight, or the probe failed. A failure is never surfaced as an error to the
 * person filling in the form: this is a preview, and a build that cannot
 * answer offers none, exactly like {@link codingSessionSeatPackNotice}'s own
 * `undefined`-renders-nothing rule for the sibling "no role pack on this
 * computer" disclosure.
 *
 * The seat is part of the question, not a detail: the host previews *this
 * agent at this role*, so an unseated form has nothing to preview rather than
 * a generic answer that no seat would actually get.
 */
export function useCodingSessionPackStatusPreview(input: {
  projectRef: string | null;
  role: string;
  /** The managed agent being seated, or `null` for an unseated execution. */
  agentPubkey: string | null;
  checkout?: string | null;
  probe?: typeof codingSessionPackStatus;
}): CodingSessionPackStatusResult | null {
  const probe = input.probe ?? codingSessionPackStatus;
  const role = input.role.trim();
  const agentPubkey = input.agentPubkey;
  const [result, setResult] =
    React.useState<CodingSessionPackStatusResult | null>(null);

  React.useEffect(() => {
    if (!input.projectRef || !agentPubkey || role.length === 0) {
      setResult(null);
      return;
    }
    let cancelled = false;
    setResult(null);
    void probe({
      projectRef: input.projectRef,
      role,
      agentPubkey,
      checkout: input.checkout ?? null,
    })
      .then((value) => {
        if (!cancelled) setResult(value);
      })
      // No host, an agent this computer does not manage, a packs repository
      // that could not be fetched, or a malformed response — all read the
      // same to this form: no preview.
      .catch(() => {
        if (!cancelled) setResult(null);
      });
    return () => {
      cancelled = true;
    };
  }, [agentPubkey, input.checkout, input.projectRef, probe, role]);

  return result;
}
