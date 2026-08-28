/**
 * The seat half of a create form, in one place so founding and joining a
 * session cannot drift apart.
 *
 * "Seat an agent" behaves identically whether the create founds a session or
 * attaches a provider to one already running: the same agent list, the same
 * home-role default, the same disclosures, and the same refusal of a
 * half-filled seat before anything is signed. Two copies of that would be two
 * chances for one of them to quietly stop saying which pack a seat carries.
 */
import * as React from "react";

import {
  defaultCodingSessionSeatRole,
  resolveCodingSessionActorSeat,
  type CodingSessionActorSeat,
} from "./codingSessionActorSeat";
import type { CodingSessionSeatAgent } from "./codingSessionSeatAgent";

export type CodingSessionSeatDraft = {
  /** Pubkey of the chosen agent, or null for an unseated create. */
  actor: string | null;
  /** The chosen agent itself, when this computer still manages it. */
  agent: CodingSessionSeatAgent | null;
  /** Display name for failure copy, or null when there is no seat. */
  label: string | null;
  onActorChange: (actor: string | null) => void;
  onRoleChange: (role: string) => void;
  /** The role box's text, defaulted from the agent's home role. */
  role: string;
  /** Both halves or neither, resolved exactly as the signed create does. */
  seat: CodingSessionActorSeat | null;
  /** Why this seat cannot be signed yet, or null. */
  error: string | null;
};

/**
 * Hold a seat draft over a list of this computer's managed agents.
 *
 * Choosing an agent fills the role box from its home role only while the box
 * is untouched — a role a person typed is never overwritten, not even by
 * switching agents — and clearing the agent clears both halves, because half a
 * seat is refused anyway.
 */
export function useCodingSessionSeatDraft(
  agents: readonly CodingSessionSeatAgent[],
): CodingSessionSeatDraft {
  const [actor, setActor] = React.useState<string | null>(null);
  const [role, setRole] = React.useState("");
  const [roleTouched, setRoleTouched] = React.useState(false);

  const agent = agents.find((candidate) => candidate.pubkey === actor) ?? null;

  const onActorChange = React.useCallback(
    (next: string | null) => {
      setActor(next);
      const chosen =
        next === null
          ? null
          : (agents.find((candidate) => candidate.pubkey === next) ?? null);
      setRole((current) =>
        defaultCodingSessionSeatRole({
          agent: chosen,
          roleTouched,
          role: current,
        }),
      );
      // Clearing the seat clears the memory of the typed role with it, so the
      // next agent chosen defaults from its own home role again.
      if (next === null) setRoleTouched(false);
    },
    [agents, roleTouched],
  );

  const onRoleChange = React.useCallback((next: string) => {
    setRoleTouched(true);
    setRole(next);
  }, []);

  const resolution = resolveCodingSessionActorSeat({ actor, role });
  return {
    actor,
    agent,
    label: agent?.name ?? null,
    onActorChange,
    onRoleChange,
    role,
    seat: resolution.seat,
    error: resolution.error,
  };
}
