/**
 * The managed agents a hire host may seat, and the facts the decision reads
 * off each one.
 *
 * Split from `useCodingSessionHire.ts` (size) so the mapping from this
 * computer's agent records to hire candidates is one tested function rather
 * than an inline object literal in the hook — and so the one project fact a
 * hire is scoped by, `projectRef`, cannot be dropped on the way through.
 */
import type { ManagedAgent } from "@/shared/api/types";
import type { CodingSessionHireCandidate } from "./codingSessionHirePolicy";

/** A managed agent, narrowed to what seating one needs. */
export type CodingSessionHireAgent = {
  pubkey: string;
  name: string;
  homeRole: string | null;
  /**
   * The project this agent durably belongs to, or null for none. Hiring
   * seats only agents of the umbrella's own project — see
   * `codingSessionHirePolicy.decideCodingSessionHire`.
   */
  projectRef: string | null;
  /**
   * The record's persona id. Read only to recognise a project team setup
   * actor, which is never hired (`isCodingSessionHireSetupActor`).
   */
  personaId?: string | null;
  hasRolePack?: boolean;
  model: string | null;
  /**
   * The runtime the record pins (`claude`, `codex`, `goose`), or null when it
   * inherits one. This decides the seat's runtime — see
   * `codingSessionHirePolicy.chooseProvider`.
   */
  runtime?: string | null;
  /** The record's inference provider, read only as a fallback for `runtime`. */
  provider?: string | null;
};

/**
 * This computer's managed agents as hire agents.
 *
 * Setup actors are **kept** here on purpose: the list is also how the host
 * names the seat that asked for a hire, and a setup session's lead may be the
 * requester. The decision excludes them from candidates instead.
 */
export function codingSessionHireAgentsFromManaged(
  agents: readonly ManagedAgent[],
): CodingSessionHireAgent[] {
  return agents.map((agent) => ({
    pubkey: agent.pubkey,
    name: agent.name,
    homeRole: agent.homeRole,
    // `undefined` is an older backend that records no association: no
    // project, which a project session will never seat.
    projectRef: agent.projectRef ?? null,
    personaId: agent.personaId,
    ...(agent.hasRolePack === undefined
      ? {}
      : { hasRolePack: agent.hasRolePack }),
    model: agent.model,
    runtime: agent.runtime,
    provider: agent.provider,
  }));
}

/** Hire agents as the decision's candidates. */
export function codingSessionHireCandidatesOf(
  agents: readonly CodingSessionHireAgent[],
): CodingSessionHireCandidate[] {
  return agents.map((agent) => ({
    pubkey: agent.pubkey,
    name: agent.name,
    homeRole: agent.homeRole,
    projectRef: agent.projectRef ?? null,
    ...(agent.personaId ? { personaId: agent.personaId } : {}),
    ...(agent.hasRolePack === undefined
      ? {}
      : { hasRolePack: agent.hasRolePack }),
    model: agent.model,
    runtime: agent.runtime ?? null,
    provider: agent.provider ?? null,
  }));
}
