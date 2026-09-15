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
  agentMaySeatInProject,
  normalizeProjectCoordinate,
} from "@/shared/lib/projectAgentAssociation";
import {
  defaultCodingSessionSeatRole,
  resolveCodingSessionActorSeat,
  type CodingSessionActorSeat,
} from "./codingSessionActorSeat";
import type { CodingSessionSeatAgent } from "./codingSessionSeatAgent";

/**
 * Which agents a seat in this session may hold, by the session's project.
 *
 * - `project`: only agents associated with `projectRef`.
 * - `none`: the session belongs to no project — only agents in no project.
 * - `unknown`: this screen cannot tell yet. The join then signs no project,
 *   so the seat is held to the projectless rule (agents in no project) and
 *   the field says the project is unknown.
 */
export type CodingSessionSeatProjectScope =
  | { kind: "project"; projectRef: string }
  | { kind: "none" }
  | { kind: "unknown" };

/**
 * The project a join's seat is scoped to, read the way the join is signed.
 *
 * A join create signs the umbrella's inherited project — the first execution
 * whose active generation names one (`addCodingSessionProviderModel.ts`
 * `inheritedUmbrellaRef`) — and the hire host reads the same value. So the
 * scope is that signed project when any execution names one. When none does,
 * the answer is `none` only once at least one execution's provider metadata
 * has been read (a null project there is the provider's answer); before that
 * it is `unknown`. The channel's project is deliberately not used: the join
 * would not sign it, so scoping the picker to it would offer agents for a
 * project the create does not name.
 */
export function resolveCodingSessionSeatProjectScope(input: {
  executions: ReadonlyArray<{
    activeGeneration: {
      projectRef: string | null;
      statusEventId: string | null;
    };
  }>;
}): CodingSessionSeatProjectScope {
  for (const execution of input.executions) {
    const signed = normalizeProjectCoordinate(
      execution.activeGeneration.projectRef,
    );
    if (signed !== null) return { kind: "project", projectRef: signed };
  }
  const reported = input.executions.some(
    (execution) => execution.activeGeneration.statusEventId !== null,
  );
  return reported ? { kind: "none" } : { kind: "unknown" };
}

/** Split agents into the ones a seat here may hold and the ones it may not. */
export function partitionCodingSessionSeatAgents<
  T extends CodingSessionSeatAgent & { projectRef?: string | null },
>(input: {
  agents: readonly T[];
  scope: CodingSessionSeatProjectScope;
}): { eligible: T[]; excludedCount: number } {
  // An unknown scope signs no project (`inheritedUmbrellaRef` is null), so it
  // takes exactly the agents a projectless create may take — never every
  // agent, which would let another project's agent into the seat.
  const projectRef =
    input.scope.kind === "project" ? input.scope.projectRef : null;
  const eligible = input.agents.filter((agent) =>
    agentMaySeatInProject(agent, projectRef),
  );
  return { eligible, excludedCount: input.agents.length - eligible.length };
}

/** The sentence under "Seat an agent" that says who is not listed, or null. */
export function codingSessionSeatScopeSentence(input: {
  scope: CodingSessionSeatProjectScope;
  excludedCount: number;
}): string | null {
  const { scope, excludedCount: count } = input;
  if (scope.kind === "unknown") {
    return "This session's project is not known yet — no execution has reported one — so the seat is created without a project and only agents that belong to no project are listed.";
  }
  if (count <= 0) return null;
  const one = count === 1;
  const agents = one ? "1 agent" : `${count} agents`;
  if (scope.kind === "none") {
    return `${agents} on this computer ${one ? "belongs" : "belong"} to a project, so ${one ? "it" : "they"} can't be seated in this session, which belongs to none.`;
  }
  return `${agents} on this computer ${one ? "isn't" : "aren't"} this project's, so ${one ? "it" : "they"} can't be seated here. Associate one on the project's Agents tab.`;
}

export type CodingSessionSeatDraft<
  T extends CodingSessionSeatAgent = CodingSessionSeatAgent,
> = {
  /** Pubkey of the chosen agent, or null for an unseated create. */
  actor: string | null;
  /** The chosen agent itself, when this computer still manages it. */
  agent: T | null;
  /** The agents the picker offers: those a seat in this session may hold. */
  agents: T[];
  /** Why some agents are not offered, or that the project is unknown. */
  scopeSentence: string | null;
  /** Display name for failure copy, or null when there is no seat. */
  label: string | null;
  onActorChange: (actor: string | null) => void;
  onRoleChange: (role: string) => void;
  /** The role box's text: the agent's primary role when it has one. */
  role: string;
  /** True when the chosen agent's primary role fixes the seat role. */
  roleLocked: boolean;
  /** Both halves or neither, resolved exactly as the signed create does. */
  seat: CodingSessionActorSeat | null;
  /** Why this seat cannot be signed yet, or null. */
  error: string | null;
};

const UNKNOWN_SCOPE: CodingSessionSeatProjectScope = { kind: "unknown" };

/**
 * Hold a seat draft over a list of this computer's managed agents.
 *
 * Choosing an agent fills the role box from its home role only while the box
 * is untouched — a role a person typed is never overwritten, not even by
 * switching agents — and clearing the agent clears both halves, because half a
 * seat is refused anyway.
 *
 * With a `scope`, only agents a seat in that session may hold are offered, and
 * a chosen agent that stops being eligible (the scope resolved after the pick)
 * reads as no seat rather than signing it.
 */
export function useCodingSessionSeatDraft<
  T extends CodingSessionSeatAgent & { projectRef?: string | null },
>(
  allAgents: readonly T[],
  scope: CodingSessionSeatProjectScope = UNKNOWN_SCOPE,
): CodingSessionSeatDraft<T> {
  const [chosen, setActor] = React.useState<string | null>(null);
  const [role, setRole] = React.useState("");
  const [roleTouched, setRoleTouched] = React.useState(false);

  const { eligible: agents, excludedCount } = React.useMemo(
    () => partitionCodingSessionSeatAgents({ agents: allAgents, scope }),
    [allAgents, scope],
  );
  const stale =
    chosen !== null &&
    allAgents.some((candidate) => candidate.pubkey === chosen) &&
    !agents.some((candidate) => candidate.pubkey === chosen);
  const actor = stale ? null : chosen;
  const agent = agents.find((candidate) => candidate.pubkey === actor) ?? null;

  const onActorChange = React.useCallback(
    (next: string | null) => {
      setActor(next);
      const picked =
        next === null
          ? null
          : (agents.find((candidate) => candidate.pubkey === next) ?? null);
      setRole((current) =>
        defaultCodingSessionSeatRole({
          agent: picked,
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

  // A stale pick is no seat: its role text would otherwise sign half of one.
  // An agent with a primary role takes that role: a new seat never relabels
  // an agent (a builder is never seated as its own verifier), and the host
  // refuses the create otherwise (`SEAT_ROLE_NOT_PRIMARY`).
  const primaryRole = agent?.homeRole?.trim() || null;
  const effectiveRole = stale ? "" : (primaryRole ?? role);
  const resolution = resolveCodingSessionActorSeat({
    actor,
    role: effectiveRole,
  });
  return {
    actor,
    agent,
    agents,
    scopeSentence: codingSessionSeatScopeSentence({ scope, excludedCount }),
    label: agent?.name ?? null,
    onActorChange,
    onRoleChange,
    role: effectiveRole,
    roleLocked: primaryRole !== null,
    seat: resolution.seat,
    error: resolution.error,
  };
}
