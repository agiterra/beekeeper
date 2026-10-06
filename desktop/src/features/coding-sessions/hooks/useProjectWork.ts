/**
 * Fetch one session's work inputs and hand them to the native fold.
 *
 * The division of labour is the team-transaction fold's: **this hook owns the
 * subscription, `buzz-core` owns the fold.** The query list below is the same
 * set `bee sessions work status` reads (`crates/beekeeper-cli/src/commands/
 * sessions/work.rs`), so the two surfaces establish coverage from the same
 * evidence.
 *
 * This hook starts no turn and publishes nothing. It is a read. It does not
 * pass a checkout path either: the native side resolves this host's own
 * agents-repository record for the project, so a seat's worktree can never
 * be the source of a contract.
 */
import { useQuery } from "@tanstack/react-query";

import { getRelaySelf } from "@/features/moderation/lib/relaySelf";
import { relayClient } from "@/shared/api/relayClient";
import {
  invokeProjectWorkCoverage,
  PROJECT_WORK_REQUEST_SCHEMA,
  toSignedWireEvent,
  type ProjectWorkGrantInput,
  type ProjectWorkResponse,
  type ProjectWorkSeatInput,
} from "@/shared/api/tauriProjectWork";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
  KIND_PROJECT_WORK_RECORD,
  KIND_REPO_STATE,
  KIND_HOST_STEP_RESULT,
  KIND_WORKFLOW_HOST_STEP_EXITED,
  KIND_WORKFLOW_HOST_STEP_REQUESTED,
} from "@/shared/constants/kinds";

/** How many rows one bounded read returns before it is truncated. */
export const PROJECT_WORK_READ_LIMIT = 500;

export type ProjectWorkScope = {
  channelRef: string;
  sessionRef: string;
  /** The session genesis event id; lane 213's fold is scoped by it. */
  genesisRef: string;
  projectRef: string;
  founderPubkey: string;
  activeSeats: readonly ProjectWorkSeatInput[];
  activeGrants: readonly ProjectWorkGrantInput[];
  /** Bare repository ids of the project's repositories, for kind:30618. */
  repositoryIds: readonly string[];
};

/**
 * The bare repository ids of the agents repositories a session's declarations
 * pin, from the kind:44249 records themselves.
 *
 * A malformed record contributes nothing rather than throwing: the fold
 * decides what a record means, and this only decides which `#d` values to
 * ask the relay for.
 */
function agentsRepositoryIds(
  workEvents: readonly { content: string }[],
): readonly string[] {
  const ids: string[] = [];
  for (const event of workEvents) {
    try {
      const repository = JSON.parse(event.content)?.body?.planRef?.repository;
      if (typeof repository === "string" && repository.length > 0) {
        ids.push(repository.slice(repository.lastIndexOf(":") + 1));
      }
    } catch {
      // Not a record this read can use; the fold says so, not this helper.
    }
  }
  return ids;
}

/** React Query key for one session's work coverage. */
export function projectWorkQueryKey(
  scope: Pick<ProjectWorkScope, "sessionRef">,
) {
  return ["project-work-coverage", scope.sessionRef] as const;
}

type FetchEvents = (
  filter: Parameters<typeof relayClient.fetchEventsBatch>[0][number],
) => Promise<RelayEvent[]>;

const defaultFetch: FetchEvents = (filter) =>
  relayClient.fetchEventsBatch([filter]);

/**
 * Read every input the fold needs and fold it.
 *
 * Exported so a test can drive it without React. A read that fails throws:
 * an empty coverage standing in for a refused read is the one answer this
 * surface must never give.
 */
export async function loadProjectWorkCoverage(
  scope: ProjectWorkScope,
  deps: {
    fetchEvents?: FetchEvents;
    relaySelf?: () => Promise<string | null>;
    fold?: typeof invokeProjectWorkCoverage;
  } = {},
): Promise<ProjectWorkResponse> {
  const fetchEvents = deps.fetchEvents ?? defaultFetch;
  const [workEvents, teamEvents, goalEvents, hostEvents, relaySelfKey] =
    await Promise.all([
      fetchEvents({
        kinds: [KIND_PROJECT_WORK_RECORD],
        "#h": [scope.channelRef],
        "#d": [scope.sessionRef],
        limit: PROJECT_WORK_READ_LIMIT,
      }),
      fetchEvents({
        kinds: [KIND_CODING_SESSION_TEAM_TRANSACTION],
        "#h": [scope.channelRef],
        "#d": [scope.sessionRef],
        limit: PROJECT_WORK_READ_LIMIT,
      }),
      fetchEvents({
        kinds: [KIND_CODING_SESSION_GOAL],
        "#h": [scope.channelRef],
        "#d": [scope.sessionRef],
        limit: PROJECT_WORK_READ_LIMIT,
      }),
      fetchEvents({
        kinds: [
          KIND_HOST_STEP_RESULT,
          KIND_WORKFLOW_HOST_STEP_EXITED,
          KIND_WORKFLOW_HOST_STEP_REQUESTED,
        ],
        "#h": [scope.channelRef],
        limit: PROJECT_WORK_READ_LIMIT,
      }),
      (deps.relaySelf ?? getRelaySelf)(),
    ]);
  // The agents repositories the declarations pinned, read out of the work
  // records themselves: `planDrift` is about the agents repository's own
  // branch tip (A10), and without its ref state every row honestly reads
  // `unknown`. The project's code repositories stay in the same query.
  const repositoryIds = [
    ...new Set([...scope.repositoryIds, ...agentsRepositoryIds(workEvents)]),
  ];
  const refStates =
    repositoryIds.length === 0
      ? []
      : await fetchEvents({
          kinds: [KIND_REPO_STATE],
          "#d": repositoryIds,
          limit: PROJECT_WORK_READ_LIMIT,
        });
  const fold = deps.fold ?? invokeProjectWorkCoverage;
  return fold({
    schema: PROJECT_WORK_REQUEST_SCHEMA,
    sessionRef: scope.sessionRef,
    projectRef: scope.projectRef,
    founderPubkey: scope.founderPubkey,
    relaySelfKey,
    activeSeats: scope.activeSeats,
    activeGrants: scope.activeGrants,
    channelRef: scope.channelRef,
    genesisRef: scope.genesisRef,
    workEvents,
    // With signatures: lane 213's assembler folds these with the canonical
    // 44244 fold, which verifies what it judges.
    teamEvents: teamEvents.map(toSignedWireEvent),
    goalEvents,
    hostEvents,
    refStates,
  });
}

/** One session's work coverage, folded natively. */
export function useProjectWork(scope: ProjectWorkScope | null) {
  return useQuery({
    queryKey: projectWorkQueryKey({ sessionRef: scope?.sessionRef ?? "" }),
    enabled: scope !== null,
    queryFn: () => loadProjectWorkCoverage(scope as ProjectWorkScope),
  });
}
