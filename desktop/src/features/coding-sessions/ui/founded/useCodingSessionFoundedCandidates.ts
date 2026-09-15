import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { useManagedAgentsQuery } from "@/features/agents/hooks";
import { useCommunities } from "@/features/communities/useCommunities";
import { useCodingSessionProject } from "@/features/projects-container/hooks";
import {
  installedRolesForProject,
  useProjectInstalledRolesQuery,
} from "@/features/roles/lib/projectInstalledRoles";
import type { ManagedAgent } from "@/shared/api/types";
import { codingSessionCrewHireRosterAgents } from "../../lib/codingSessionCrewLaunchFirstTurn";
import {
  type CodingSessionCrewTeam,
  listCodingSessionCrewTeams,
} from "../../lib/codingSessionCrewTeams";
import {
  applyCodingSessionInstalledRoles,
  codingSessionCandidateExclusionSentence,
  codingSessionLeadEmptySentence,
  codingSessionProjectLeadDefault,
  groupCodingSessionCandidates,
  partitionCodingSessionCandidates,
  resolveCodingSessionLeadActor,
} from "../../lib/codingSessionLeadCandidateGroups";
import type { CodingSessionLaunchLead } from "../../lib/codingSessionLaunchForm";
import type { CodingSessionSetupMode } from "../../lib/codingSessionSetupMode";
import type { NewCodingSessionBenchOption } from "../NewCodingSessionBenchField";
import {
  resolveNewCodingSessionLead,
  type NewCodingSessionLeadCandidate,
} from "../NewCodingSessionLeadField";

/** Role-assignment records, read once per screen to label each identity. */
export const codingSessionCrewTeamsQueryKey = ["coding-session-crew-teams"];

/** Solo's lead: the founder, always, whatever the Team picker last held. */
const YOU_LEAD: CodingSessionLaunchLead = { kind: "you", label: "You" };

const EMPTY_AGENTS: ManagedAgent[] = [];

/**
 * Who may lead, who may be benched and who the lead may hire, for the founded
 * setup card — all from one association rule (`ManagedAgent.projectRef`),
 * the same one the Agents page and the hire host read.
 *
 * Until the channel record is read, `projectRef` null means "not read", not
 * "no project", so nothing is offered: listing the projectless set and then
 * swapping it for the project's would let a pick land on the wrong side. An
 * explicit pick that is no longer eligible resolves to unset.
 */
export function useCodingSessionFoundedCandidates(input: {
  channelId: string;
  projectRef: string | null;
  channelReader: "loading" | "errored" | "resolved";
  mode: CodingSessionSetupMode;
}) {
  const { channelId, projectRef, channelReader, mode } = input;
  const managedAgentsQuery = useManagedAgentsQuery();
  const managedAgents = managedAgentsQuery.data ?? EMPTY_AGENTS;
  // Read only to label which role each identity carries; the launch never
  // creates more than the lead's own seat.
  const roleHintsQuery = useQuery({
    queryKey: codingSessionCrewTeamsQueryKey,
    queryFn: listCodingSessionCrewTeams,
    staleTime: 30_000,
  });
  const roleHintRecords = React.useMemo<CodingSessionCrewTeam[]>(
    () => roleHintsQuery.data ?? [],
    [roleHintsQuery.data],
  );
  // What this project installed on this computer: a label for the role each
  // identity was installed as. Never eligibility.
  const { activeCommunity } = useCommunities();
  const installedRolesQuery = useProjectInstalledRolesQuery(
    activeCommunity?.relayUrl ?? null,
  );
  const installedRoles = React.useMemo(
    () => installedRolesForProject(installedRolesQuery.data, projectRef),
    [installedRolesQuery.data, projectRef],
  );
  const project = useCodingSessionProject(channelId, projectRef);
  const projectName = project?.name ?? null;
  const candidates = React.useMemo<NewCodingSessionLeadCandidate[]>(
    () =>
      applyCodingSessionInstalledRoles(
        managedAgents.map((agent) => ({
          pubkey: agent.pubkey,
          name: agent.name,
          // Read, never typed. A role record wins over the home role because it
          // names what the identity is *for* in this project.
          role:
            roleHintRecords
              .flatMap((team) => team.crew.seats)
              .find((seat) => seat.personaId === agent.personaId)?.role ??
            agent.homeRole ??
            null,
          model: agent.model,
          projectRef: agent.projectRef ?? null,
          ...(agent.hasRolePack === undefined
            ? {}
            : { hasRolePack: agent.hasRolePack }),
        })),
        installedRoles,
      ),
    [installedRoles, roleHintRecords, managedAgents],
  );

  const projectKnown = channelReader === "resolved";
  const { eligible, excluded } = React.useMemo(
    () =>
      projectKnown
        ? partitionCodingSessionCandidates({ candidates, projectRef })
        : { eligible: [], excluded: [] },
    [candidates, projectKnown, projectRef],
  );
  const leadGroups = React.useMemo(
    () =>
      groupCodingSessionCandidates({
        candidates: eligible,
        projectRef,
        projectName,
      }),
    [eligible, projectName, projectRef],
  );
  const leadEmptySentence = !projectKnown
    ? channelReader === "loading"
      ? "Which agents can lead depends on this session's project, which is still being read."
      : "This session's project could not be read, so no agent is offered: picking one would guess which project it works for."
    : codingSessionLeadEmptySentence({
        eligibleCount: eligible.length,
        projectRef,
        projectName,
      });
  const leadExclusionSentence = codingSessionCandidateExclusionSentence({
    excludedCount: excluded.length,
    projectRef,
    projectName,
    surface: "lead",
  });

  // Solo: you, whatever the Team picker last held. Team: the picked agent,
  // or `unset` — never "you" by fallback, which would seat a person as the
  // agent lead. An explicit pick is never overwritten; until there is one, the
  // project's single agent whose role is `lead` is preselected.
  const [leadSelection, setLeadSelection] = React.useState<{
    actor: string | null;
    explicit: boolean;
  }>({ actor: null, explicit: false });
  const setLeadActor = React.useCallback(
    (actor: string | null) => setLeadSelection({ actor, explicit: true }),
    [],
  );
  const leadActor = resolveCodingSessionLeadActor({
    selection: leadSelection,
    defaultActor: codingSessionProjectLeadDefault({
      candidates: eligible,
      projectRef,
    }),
    eligible,
  });
  const lead = React.useMemo<CodingSessionLaunchLead>(
    () =>
      mode === "solo"
        ? YOU_LEAD
        : resolveNewCodingSessionLead({
            actor: leadActor,
            candidates: eligible,
          }),
    [eligible, leadActor, mode],
  );

  // The bench is drawn from the same eligible set, less the lead.
  const benchIdentityOptions = React.useMemo<NewCodingSessionBenchOption[]>(
    () =>
      groupCodingSessionCandidates({
        candidates: eligible.filter(
          (candidate) =>
            !(lead.kind === "agent" && candidate.pubkey === lead.actor),
        ),
        projectRef,
        projectName,
      }).flatMap((group) =>
        group.candidates.map((candidate) => ({
          value: candidate.pubkey,
          label: candidate.name,
          detail: candidate.role,
          group: group.heading,
        })),
      ),
    [eligible, lead, projectName, projectRef],
  );
  const benchEmptySentence = !projectKnown
    ? "Who can be benched depends on this session's project, which has not been read."
    : projectRef === null
      ? "No other agent on this computer outside a project carries a role, so there is nobody to bench. The lead will work alone."
      : `No other ${projectName?.trim() || "project"} agent with a role is on this computer, so there is nobody to bench. The lead will work alone.`;
  const benchExclusionSentence = codingSessionCandidateExclusionSentence({
    excludedCount: excluded.length,
    projectRef,
    projectName,
    surface: "bench",
  });
  const eligiblePubkeys = React.useMemo(
    () => new Set(eligible.map((candidate) => candidate.pubkey)),
    [eligible],
  );

  const hireRoster = React.useMemo(
    () =>
      projectKnown
        ? codingSessionCrewHireRosterAgents({
            agents: managedAgents,
            projectRef,
          })
        : [],
    [managedAgents, projectKnown, projectRef],
  );

  return {
    candidates,
    eligiblePubkeys,
    leadGroups,
    leadEmptySentence,
    leadExclusionSentence,
    lead,
    leadActor,
    setLeadActor,
    benchIdentityOptions,
    benchEmptySentence,
    benchExclusionSentence,
    hireRoster,
    projectId: project?.id ?? null,
    projectName,
  };
}
