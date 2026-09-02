/**
 * Reading crews off the local team store, and turning a crew's seats into
 * seats the launch can actually create.
 *
 * The teams query in `shared/api` projects a fixed field set and drops
 * anything it does not name, so the crew block is read here from the raw
 * `list_teams` payload instead. That keeps the crew contract (plan D8) in one
 * place with the launch that consumes it.
 *
 * `resolveCodingSessionCrewSeats` used to live here and is deleted. It bound a
 * whole crew's seats to managed agents, which is what a launch did before D14
 * seated the lead alone; after the one form replaced the Team tab it had zero
 * production callers, and its own tests kept it green (REVIEW-B3 F1/F5). The
 * question it answered — what model does this seat run on — is now answered
 * for the one seat a launch creates, by
 * `resolveCodingSessionLeadModel` in `codingSessionLaunchForm.ts`, where the
 * form can be blocked rather than quietly borrowing a model.
 */
import { invokeTauri } from "@/shared/api/tauri";
import {
  parseCodingSessionCrew,
  type CodingSessionCrew,
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
