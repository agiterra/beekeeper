/**
 * Reading crews off the local team store, and turning a crew's seats into
 * seats the launch can actually create.
 *
 * The teams query in `shared/api` projects a fixed field set and drops
 * anything it does not name, so the crew block is read here from the raw
 * `list_teams` payload instead. That keeps the crew contract (plan D8) in one
 * place with the launch that consumes it.
 */
import { invokeTauri } from "@/shared/api/tauri";
import {
  parseCodingSessionCrew,
  type CodingSessionCrew,
  type ResolvedCodingSessionCrewSeat,
} from "./codingSessionCrew";

/** A team that carries a crew block — the only kind a crew launch can use. */
export type CodingSessionCrewTeam = {
  id: string;
  name: string;
  crew: CodingSessionCrew;
};

type RawTeamWithCrew = {
  id?: unknown;
  name?: unknown;
  crew?: unknown;
};

/** Keep only the teams that are crews, in the order the store returned them. */
export function readCodingSessionCrewTeams(
  raw: readonly RawTeamWithCrew[],
): CodingSessionCrewTeam[] {
  const teams: CodingSessionCrewTeam[] = [];
  for (const team of raw) {
    if (typeof team.id !== "string" || typeof team.name !== "string") continue;
    const crew = parseCodingSessionCrew(team.crew);
    if (crew) teams.push({ id: team.id, name: team.name, crew });
  }
  return teams;
}

/** List this computer's crew teams. */
export async function listCodingSessionCrewTeams(): Promise<
  CodingSessionCrewTeam[]
> {
  return readCodingSessionCrewTeams(
    await invokeTauri<RawTeamWithCrew[]>("list_teams"),
  );
}

/** A managed agent, as much of it as a seat needs. */
export type CodingSessionCrewAgent = {
  pubkey: string;
  name: string;
  personaId: string | null;
  model: string | null;
  /**
   * Whether this computer can stage the agent's role pack. Carried onto the
   * resolved seat so the roster can disclose a packless seat *before* the
   * launch, rather than after staging has already reported it.
   */
  hasRolePack?: boolean;
};

export type CodingSessionCrewSeatResolution =
  | { seats: ResolvedCodingSessionCrewSeat[]; error: null }
  | { seats: null; error: string };

/**
 * Bind each crew seat to the managed agent that fills it.
 *
 * A seat whose persona has no managed agent on this computer stops the whole
 * resolution: a crew launched with a hole in it is a crew whose lead addresses
 * a role nobody holds. The seat's own `model` wins over the agent's, because
 * the crew is the thing declaring what this seat is *for*, and
 * `fallbackModel` — the dialog's model — is last, for a seat that names none.
 *
 * **This is the only place a seat's model is decided.** The launch publishes
 * `seat.model` verbatim, so the family check, the roster, and the create all
 * read one value. A second fallback applied at publish time is how a crew
 * passes a vendor check on one model and then runs on another.
 *
 * Deciding the model once is necessary but *not sufficient* for that: the
 * launch also has to verify the selected provider runtime actually offers it
 * (`checkCodingSessionCrewSeatModels`), because a model an adapter does not
 * have is silently replaced by that adapter's default. One value, checked
 * against the runtime that will run it — either half alone still lets a seat
 * pass a vendor check on one model and run on another.
 */
export function resolveCodingSessionCrewSeats(input: {
  crew: CodingSessionCrew;
  agents: readonly CodingSessionCrewAgent[];
  /** Model the create falls back to when neither seat nor agent names one. */
  fallbackModel?: string | null;
}): CodingSessionCrewSeatResolution {
  const seats: ResolvedCodingSessionCrewSeat[] = [];
  const taken = new Set<string>();
  for (const seat of input.crew.seats) {
    const agent = input.agents.find(
      (candidate) =>
        candidate.personaId === seat.personaId && !taken.has(candidate.pubkey),
    );
    if (!agent) {
      return {
        seats: null,
        error: `No agent on this computer fills the ${seat.role} seat (persona ${seat.personaId}). Create one, then launch again.`,
      };
    }
    taken.add(agent.pubkey);
    const resolved: ResolvedCodingSessionCrewSeat = {
      personaId: seat.personaId,
      role: seat.role,
      actor: agent.pubkey.toLowerCase(),
      actorLabel: agent.name,
      model: seat.model ?? agent.model ?? input.fallbackModel ?? null,
      vendor: seat.vendor ?? null,
    };
    // Set only when the agent actually answered: an absent field and a field
    // set to `undefined` are the same to a reader, but only the first says
    // "nobody asked" to anything comparing the seat's shape.
    if (agent.hasRolePack !== undefined) {
      resolved.hasRolePack = agent.hasRolePack;
    }
    seats.push(resolved);
  }
  return { seats, error: null };
}
