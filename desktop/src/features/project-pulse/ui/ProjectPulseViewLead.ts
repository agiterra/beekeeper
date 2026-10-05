import type { PulseDigestSession } from "../lib/pulseFold.ts";
import type { PulseCoordinationState } from "../lib/pulseFoldTypes";

/** The three session groups `ProjectPulseView` draws, in its order. */
export type ProjectPulseSessionGroups = {
  providerReachable: PulseDigestSession[];
  openUnverified: PulseDigestSession[];
  closed: PulseDigestSession[];
};

/**
 * Take one session out of its group to lead the Pulse (SV-24: the session
 * view's Pulse surface puts its own session first).
 *
 * Matched by `sessionKey` or `sessionRef`. The lead keeps its group's
 * classification (`leadGroup`) so leaving the group does not drop whether
 * its provider answered, and it leaves `listed`, so it is drawn once. With
 * no key, or a key not visible under the current filter, nothing moves.
 */
export function splitProjectPulseLeadSession(
  groups: ProjectPulseSessionGroups,
  leadSessionKey: string | null | undefined,
): {
  lead: PulseDigestSession | null;
  leadGroup: PulseCoordinationState | null;
  listed: ProjectPulseSessionGroups;
} {
  if (!leadSessionKey) return { lead: null, leadGroup: null, listed: groups };
  const isLead = (session: PulseDigestSession) =>
    session.sessionKey === leadSessionKey ||
    session.sessionRef === leadSessionKey;
  const order: readonly [
    keyof ProjectPulseSessionGroups,
    PulseCoordinationState,
  ][] = [
    ["providerReachable", "provider_reachable"],
    ["openUnverified", "open_unverified"],
    ["closed", "closed"],
  ];
  for (const [group, state] of order) {
    const lead = groups[group].find(isLead);
    if (lead === undefined) continue;
    const without = (sessions: PulseDigestSession[]) =>
      sessions.filter((session) => session !== lead);
    return {
      lead,
      leadGroup: state,
      listed: {
        providerReachable: without(groups.providerReachable),
        openUnverified: without(groups.openUnverified),
        closed: without(groups.closed),
      },
    };
  }
  return { lead: null, leadGroup: null, listed: groups };
}
