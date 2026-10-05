import * as React from "react";
import { useQuery } from "@tanstack/react-query";

import { useCodingSessionRunningGates } from "@/features/coding-sessions/hooks/useCodingSessionGateStartClock";
import { codingSessionObservationNotLiveOf } from "@/features/coding-sessions/lib/codingSessionObservationLiveness";
import {
  type CodingSessionMissionLandRepository,
  readCodingSessionRepository,
} from "@/features/coding-sessions/hooks/useCodingSessionMissionLand";
import {
  type CodingSessionLandingGateRead,
  type CodingSessionLandingMainRead,
  type CodingSessionLandingModel,
  type CodingSessionLandingRuleRead,
  codingSessionLandingModel,
  codingSessionLandingNewestGateHead,
} from "@/features/coding-sessions/lib/codingSessionLandingModel";
import { CODING_SESSION_LAND_REF } from "@/features/coding-sessions/lib/codingSessionMissionLand";
import { getProjectRepoSnapshot } from "@/shared/api/projectGit";

import {
  readCodingSessionLandingExtension,
  useCodingSessionLandingResolveWho,
} from "./CodingSessionLandingRuleRead";
import type { CodingSessionSurfaceCtx } from "./surfaces/codingSessionSurfaceContext";

/**
 * Everything the Landing surface reads, composed once while it is open.
 *
 * The land rule's answer and newest verdict come from Landing's own
 * `readExtension` (`ctx.extensions.landing`, `CodingSessionLandingRuleRead.ts`),
 * which the view runs once so the badge and this panel read the same answer.
 * The repository announcement (whose `clone` tag the Landed row reads `main`
 * through) is captured there on the way through the land hook. Wherever that
 * capture is missing once the rule has stopped asking — no genesis, so the
 * rule is never asked; a mission-evidence read that failed; a rule that never
 * reached its repository seam — the announcement is read here directly, so
 * an unrelated failure never hides a `main` this view can check on its own.
 *
 * The `main` read is one bounded `getProjectRepoSnapshot` call (at most 50
 * commits, `project_git.rs`), made only while this panel is mounted, never
 * polled, and stamped with when it answered.
 */

/** The announcement's first `clone` URL, or null. */
export function codingSessionLandingCloneUrl(
  repository: Pick<CodingSessionMissionLandRepository, "protectionTags"> | null,
): string | null {
  const tag = repository?.protectionTags.find(
    (candidate) => candidate[0] === "clone" && (candidate[1] ?? "") !== "",
  );
  return tag?.[1] ?? null;
}

/**
 * Whether Landing reads the repository announcement itself rather than
 * waiting on the land rule's capture: a repository is named, the rule
 * captured none, and the rule is not still asking. A genesis does not
 * matter — a failed mission-evidence read must not hide a `main` this view
 * can check on its own.
 */
export function codingSessionLandingReadsRepositoryDirectly(
  extension: Pick<
    NonNullable<ReturnType<typeof readCodingSessionLandingExtension>>,
    "repository" | "repoRef" | "rule"
  > | null,
): boolean {
  return (
    extension !== null &&
    extension.repository === null &&
    extension.repoRef !== null &&
    extension.rule.state !== "asking"
  );
}

function gateRead(
  ctx: Pick<CodingSessionSurfaceCtx, "observations">,
): CodingSessionLandingGateRead {
  const observations = ctx.observations;
  if (observations.state === "not-read") {
    return { state: "not-read", reason: observations.reason };
  }
  if (observations.result === null) {
    if (observations.errorMessage !== null) {
      return { state: "error", message: observations.errorMessage };
    }
    return { state: "loading" };
  }
  return {
    state: "read",
    rows: observations.view.gates,
    signedAt: observations.result.signedAt,
  };
}

function formatClock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

const RULE_NOT_SHARED: CodingSessionLandingRuleRead = {
  state: "not-read",
  reason: "This view did not share the land rule's answer with this panel.",
};

export type CodingSessionLandingRead = {
  model: CodingSessionLandingModel;
  /** Re-reads `main` (and nothing else); null while there is nothing to read. */
  refreshMain: (() => void) | null;
  refreshingMain: boolean;
};

export function useCodingSessionLandingRead(
  ctx: CodingSessionSurfaceCtx,
): CodingSessionLandingRead {
  const extension = readCodingSessionLandingExtension(ctx.extensions.landing);
  const rule = extension?.rule ?? RULE_NOT_SHARED;
  const resolveWho = useCodingSessionLandingResolveWho(ctx);
  const fold =
    ctx.observations.state === "read"
      ? (ctx.observations.result?.fold ?? null)
      : null;
  // The land rule's repository seam is the first source of the announcement.
  // Where it captured none and the rule is no longer asking (no genesis, a
  // failed evidence read, a rule that stopped short of the seam), read the
  // announcement directly so Landed can still check main.
  const repoRef = extension?.repoRef ?? null;
  const readDirectly = codingSessionLandingReadsRepositoryDirectly(extension);
  // A NIP-34 coordinate names no relay, and the query client outlives a
  // community switch: the key carries the community so one relay's
  // announcement never answers for another's.
  const directRepository = useQuery({
    queryKey: [
      "coding-session-landing-repository",
      ctx.communityScope,
      repoRef,
    ],
    queryFn: () => readCodingSessionRepository(repoRef ?? ""),
    enabled: readDirectly,
    retry: false,
    staleTime: 60_000,
  });
  const repository =
    extension?.repository ??
    (readDirectly ? (directRepository.data ?? null) : null);
  const running = useCodingSessionRunningGates(
    fold,
    codingSessionObservationNotLiveOf(ctx.observations, formatClock),
  );
  const observationsRead = ctx.observations;
  const gates = React.useMemo(
    () => gateRead({ observations: observationsRead }),
    [observationsRead],
  );
  const gateHead =
    gates.state === "read"
      ? codingSessionLandingNewestGateHead(gates.rows, gates.signedAt)
      : null;
  const wantedHead =
    gateHead ??
    (rule.state === "read"
      ? (rule.newestVerdict?.headSha ?? rule.land.headSha)
      : null);
  const cloneUrl = codingSessionLandingCloneUrl(repository);
  const mainQuery = useQuery({
    queryKey: ["coding-session-landing-main", cloneUrl],
    queryFn: () =>
      getProjectRepoSnapshot({
        cloneUrl: cloneUrl ?? "",
        targetRef: CODING_SESSION_LAND_REF,
      }),
    enabled: cloneUrl !== null && wantedHead !== null,
    retry: false,
    staleTime: 60_000,
  });
  const main = React.useMemo<CodingSessionLandingMainRead>(() => {
    if (cloneUrl === null) {
      if (repository === null && readDirectly && directRepository.isError) {
        return {
          state: "not-read",
          reason: `main was not read: the repository's announcement could not be read (${errorText(directRepository.error)}).`,
        };
      }
      // Being read right now is not "has not read": say it is loading.
      if (repository === null && readDirectly && directRepository.isPending) {
        return { state: "loading" };
      }
      return {
        state: "not-read",
        reason:
          repository === null
            ? "main was not read: this view has not read the repository's announcement."
            : "main was not read: the repository's announcement names no clone URL.",
      };
    }
    if (mainQuery.data) {
      return {
        state: "read",
        commits: mainQuery.data.commits.map((commit) => commit.hash),
        checkedAtMs: mainQuery.dataUpdatedAt,
        // React Query keeps the old data when a refresh fails: say so.
        refreshError: mainQuery.isError ? errorText(mainQuery.error) : null,
      };
    }
    if (mainQuery.isError) {
      return { state: "error", message: errorText(mainQuery.error) };
    }
    return { state: "loading" };
  }, [
    cloneUrl,
    directRepository.error,
    directRepository.isError,
    directRepository.isPending,
    mainQuery.data,
    mainQuery.dataUpdatedAt,
    mainQuery.error,
    mainQuery.isError,
    readDirectly,
    repository,
  ]);

  // Nothing named and nothing inferred (or the rule still inferring): Land
  // and Landed say there is no repository rather than "not read".
  const noRepository =
    repoRef === null &&
    ctx.repoRef === null &&
    repository === null &&
    rule.state !== "asking";
  const model = React.useMemo(
    () =>
      codingSessionLandingModel({
        gates,
        running,
        // No fold read means nothing claimed provenance either way.
        provenanceChecked: fold?.provenanceChecked ?? true,
        rule,
        main,
        resolveWho,
        formatTime: formatClock,
        noRepository,
      }),
    [fold, gates, main, noRepository, resolveWho, rule, running],
  );
  const refetchMain = mainQuery.refetch;
  const refreshMain = React.useCallback(() => {
    void refetchMain();
  }, [refetchMain]);
  return {
    model,
    refreshMain: cloneUrl !== null && wantedHead !== null ? refreshMain : null,
    refreshingMain: mainQuery.isFetching,
  };
}
