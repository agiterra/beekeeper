/**
 * The lead's first turn after a team launch: the goal, then who it may hire.
 *
 * Split from `codingSessionCrewLaunch.ts` so the sequence and the words stay
 * separately readable; the launch re-exports it.
 */
import { isCodingSessionHireSetupActor } from "./codingSessionHirePolicy";
import { agentMaySeatInProject } from "@/shared/lib/projectAgentAssociation";
import { truncatePubkey } from "@/shared/lib/pubkey";
import {
  codingSessionCrewRosterText,
  type ResolvedCodingSessionCrewSeat,
} from "./codingSessionCrew";

/** One agent on this computer the lead may hire, as its own primary role. */
export type CodingSessionCrewHireRosterAgent = {
  pubkey: string;
  name: string;
  role: string;
};

/**
 * Who the lead may hire, as this computer can seat them.
 *
 * Built from the association rule the hire host enforces: a project session's
 * own agents, or, for a session in no project, the agents that belong to none.
 * The founder's computer answers every hire, so its agents are the whole
 * offer — saying "nobody else" while project agents exist here is the lie this
 * type exists to end.
 */
export type CodingSessionCrewHireRoster = {
  /** Null for a session that belongs to no project. */
  projectRef: string | null;
  projectName: string | null;
  /** May include the lead; it is left out of the text. */
  agents: ReadonlyArray<CodingSessionCrewHireRosterAgent>;
};

/**
 * The agents a lead of this session may hire, by role: this project's agents
 * on this computer, or — outside a project — the ones that belong to none.
 * Only agents with a home role, because a hire seats an agent as its own
 * primary role and a roleless agent cannot be hired as anything.
 */
export function codingSessionCrewHireRosterAgents(input: {
  agents: ReadonlyArray<{
    pubkey: string;
    name: string;
    homeRole: string | null;
    projectRef?: string | null;
    personaId?: string | null;
    hasRolePack?: boolean;
  }>;
  projectRef: string | null;
}): CodingSessionCrewHireRosterAgent[] {
  const roster: CodingSessionCrewHireRosterAgent[] = [];
  for (const agent of input.agents) {
    const role = agent.homeRole?.trim();
    if (!role) continue;
    // The same exclusions the hire host applies, so the lead is never told it
    // can hire an agent the host will refuse: a setup actor is never hired,
    // and an agent this computer holds no role pack for is not offered.
    if (isCodingSessionHireSetupActor(agent)) continue;
    if (agent.hasRolePack === false) continue;
    if (!agentMaySeatInProject(agent, input.projectRef)) continue;
    roster.push({ pubkey: agent.pubkey, name: agent.name, role });
  }
  return roster;
}

const HIRE_COMMAND =
  "  bee sessions hire --channel <uuid> --session-ref <uuid> --role <role> --brief <file>";
const HIRE_BRIEF_RULE =
  "A hired seat's first turn IS the brief — do not send a second start " +
  "message. End your turn after hiring.";

/** `- builder: Builder (1c47d440…0000)`, sorted by role then name. */
function hireRosterLines(
  agents: ReadonlyArray<CodingSessionCrewHireRosterAgent>,
): string[] {
  return [...agents]
    .sort(
      (left, right) =>
        left.role.localeCompare(right.role) ||
        left.name.localeCompare(right.name) ||
        left.pubkey.localeCompare(right.pubkey),
    )
    .map(
      (agent) =>
        `- ${agent.role}: ${agent.name} (${truncatePubkey(agent.pubkey)})`,
    );
}

function rosterFirstTurnText(input: {
  goal: string;
  lead: ResolvedCodingSessionCrewSeat;
  roster: CodingSessionCrewHireRoster;
}): string {
  const { goal, lead, roster } = input;
  const leadKey = lead.actor.toLowerCase();
  const others = roster.agents.filter(
    (agent) => agent.pubkey.toLowerCase() !== leadKey,
  );
  const lines = hireRosterLines(others);
  if (roster.projectRef !== null) {
    const name = roster.projectName?.trim() || "this project";
    const discovery =
      "List this project's agents any time with `bee projects agents`. " +
      "Hire by role with `bee sessions hire`; this computer seats only this " +
      "project's agents.";
    if (lines.length === 0) {
      return [
        goal,
        "",
        `You are seated. No other ${name} agent is on this computer, so a ` +
          "hire from this session is refused until one is installed or " +
          "associated with the project here. `bee projects agents` may list " +
          "agents on other computers; this computer cannot seat those.",
        "",
        discovery,
      ].join("\n");
    }
    return [
      goal,
      "",
      `You are seated. No one else is seated yet: the ${name} agents below are on ` +
        "this computer, and are who you may hire, by role, once you know " +
        "what the work is.",
      "",
      `[${name} agents on this computer]`,
      ...lines,
      "",
      discovery,
      HIRE_COMMAND,
      HIRE_BRIEF_RULE,
    ].join("\n");
  }
  if (lines.length === 0) {
    return `${goal}\n\nYou are seated. No one else is seated, and there is nobody to hire: this session belongs to no project, and no agent on this computer outside a project carries a role.`;
  }
  return [
    goal,
    "",
    "You are seated. No one else is seated yet: the agents below belong to no project " +
      "and are on this computer. They are who you may hire, by role, once " +
      "you know what the work is.",
    "",
    "[Agents on this computer in no project]",
    ...lines,
    "",
    "Hire by role with `bee sessions hire`; this session belongs to no " +
      "project, so this computer seats only agents that belong to none.",
    HIRE_COMMAND,
    HIRE_BRIEF_RULE,
  ].join("\n");
}

/**
 * The lead's first turn: the goal, then the team it may hire — labelled as an
 * offer, with the verb that takes it up.
 *
 * The distinction is the whole point. A roster that reads like a seated team
 * has the lead addressing three agents that do not exist, waiting for reports
 * that cannot come; naming it as hireable, with the command, is what turns the
 * same list into work it can actually start.
 *
 * With a `roster`, the offer is the agents this computer would actually seat
 * for this session — never the launch seats alone, which for a founded Team
 * Start are only the lead and so always read "nobody else".
 */
export function codingSessionCrewLeadFirstTurnText(input: {
  goal: string;
  lead: ResolvedCodingSessionCrewSeat;
  hireable: ReadonlyArray<ResolvedCodingSessionCrewSeat>;
  roster?: CodingSessionCrewHireRoster | null;
}): string {
  const goal = input.goal.trim();
  if (input.roster) {
    return rosterFirstTurnText({
      goal,
      lead: input.lead,
      roster: input.roster,
    });
  }
  if (input.hireable.length === 0) {
    return `${goal}\n\nYou are seated. Nobody else is — this team has no other roles on this computer.`;
  }
  const roster = codingSessionCrewRosterText({
    seats: input.hireable,
    primaryPersonaId: input.lead.personaId,
  });
  return [
    goal,
    "",
    "You are seated. Nobody else is: the roster below is who you may hire, " +
      "one at a time, once you know what the work is.",
    "",
    roster,
    "",
    "Hire with:",
    HIRE_COMMAND,
    HIRE_BRIEF_RULE,
  ].join("\n");
}
