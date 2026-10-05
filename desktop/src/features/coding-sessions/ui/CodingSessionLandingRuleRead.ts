import * as React from "react";

import {
  type CodingSessionMissionLandRepository,
  readCodingSessionRepository,
  useCodingSessionMissionLand,
} from "@/features/coding-sessions/hooks/useCodingSessionMissionLand";
import { useCodingSessionSessionPolicy } from "@/features/coding-sessions/hooks/useCodingSessionSessionPolicy";
import {
  type CodingSessionLandingRuleRead,
  codingSessionLandingRuleUnavailableReason,
  codingSessionLandingVerdictClass,
} from "@/features/coding-sessions/lib/codingSessionLandingModel";
import {
  type CodingSessionLandNewestVerdict,
  invokeCodingSessionLand,
} from "@/features/coding-sessions/lib/codingSessionMissionLand";
import type { CodingSessionSurfaceLandingBadgeExtension } from "@/features/coding-sessions/lib/codingSessionSurfaceBadgeModel";
import { useCodingSessionMissionEvidence } from "@/features/coding-sessions/lib/useCodingSessionMissionEvidence";
import { useIdentityQuery } from "@/shared/api/hooks";
import { truncatePubkey } from "@/shared/lib/pubkey";

import type { CodingSessionSurfaceBaseCtx } from "./surfaces/codingSessionSurfaceContext";
import { codingSessionSurfaceEvidenceScope } from "./surfaces/useCodingSessionSurfaceTeamRead";

/**
 * The Landing surface's `readExtension` (`ctx.extensions.landing`): the land
 * rule's answer and its newest verdict, read once per view so that a refusing
 * verdict reaches Landing's badge and the header's right-panel dot while the
 * panel is closed (SV-22, DB5), and the panel reads the same answer rather
 * than asking twice.
 *
 * The rule is asked exactly as Mission asks it (`useCodingSessionMissionLand`
 * over the same evidence scope, policy and folded gate rows). It runs only
 * where there is a mission to ask about — a session with a genesis;
 * everywhere else every read is idle (null scope).
 *
 * No module cache: everything here is React state or React Query, so
 * `resetCommunityState()` has nothing to clear.
 */
export type CodingSessionLandingExtension =
  CodingSessionSurfaceLandingBadgeExtension & {
    rule: CodingSessionLandingRuleRead;
    /** Captured through the land hook's repository seam, or null. */
    repository: CodingSessionMissionLandRepository | null;
    /** The `repoRef` every execution agrees on, or null. */
    repoRef: string | null;
    /** Whether a genesis scopes the mission reads. */
    hasGenesis: boolean;
  };

/** The one name a mission's executions agree on, or null (Mission's rule). */
export function codingSessionLandingOneOrNone(
  values: readonly (string | null | undefined)[],
): string | null {
  const named = new Set(
    values.map((value) => value?.trim() ?? "").filter((value) => value !== ""),
  );
  return named.size === 1 ? [...named][0] : null;
}

/** `{Who}` for a pubkey: "the founder", a resolved name, or a short key. */
export function useCodingSessionLandingResolveWho(
  ctx: Pick<CodingSessionSurfaceBaseCtx, "umbrella" | "resolveActorName">,
): (pubkey: string) => string {
  const founder = ctx.umbrella.founderPubkey?.trim().toLowerCase() ?? "";
  const resolveActorName = ctx.resolveActorName;
  return React.useCallback(
    (pubkey: string) =>
      pubkey.trim().toLowerCase() === founder && founder !== ""
        ? "the founder"
        : (resolveActorName(pubkey) ?? truncatePubkey(pubkey)),
    [founder, resolveActorName],
  );
}

/**
 * The badge half of the contract: a refusing newest verdict, labelled with
 * its word and signer; null for any other verdict or an unread rule.
 */
export function codingSessionLandingRefusingVerdict(
  rule: CodingSessionLandingRuleRead,
  resolveWho: (pubkey: string) => string,
): { label: string } | null {
  if (rule.state !== "read" || rule.newestVerdict === null) return null;
  const verdict = rule.newestVerdict;
  if (codingSessionLandingVerdictClass(verdict.decision) !== "refuses") {
    return null;
  }
  return {
    label: `${verdict.decision} by ${resolveWho(verdict.authorPubkey)}`,
  };
}

/** Read `ctx.extensions.landing` back, or null when it is not this shape. */
export function readCodingSessionLandingExtension(
  value: unknown,
): CodingSessionLandingExtension | null {
  if (typeof value !== "object" || value === null) return null;
  const rule = (value as { rule?: unknown }).rule;
  if (typeof rule !== "object" || rule === null) return null;
  return value as CodingSessionLandingExtension;
}

export function useCodingSessionLandingExtension(
  ctx: CodingSessionSurfaceBaseCtx,
): CodingSessionLandingExtension {
  const { umbrella } = ctx;
  const repoRef = React.useMemo(
    () =>
      codingSessionLandingOneOrNone(
        umbrella.executions.map(
          (execution) => execution.activeGeneration.repoRef,
        ),
      ),
    [umbrella.executions],
  );
  const projectRef = React.useMemo(
    () =>
      codingSessionLandingOneOrNone(
        umbrella.executions.map(
          (execution) => execution.activeGeneration.projectRef,
        ),
      ),
    [umbrella.executions],
  );
  // The rule is asked where there is a mission to ask about: a genesis
  // (without one the scope is null and every read idles). A session that
  // names no repository still has a verdict to show, and the land hook
  // infers the project's one repository where it can.
  const landingAvailable = ctx.repoRef !== null || umbrella.genesisRef !== null;
  const scope = React.useMemo(
    () =>
      landingAvailable
        ? codingSessionSurfaceEvidenceScope(ctx.channelId, {
            founderPubkey: umbrella.founderPubkey,
            genesisRef: umbrella.genesisRef,
            sessionRef: umbrella.sessionRef,
          })
        : null,
    [
      ctx.channelId,
      landingAvailable,
      umbrella.founderPubkey,
      umbrella.genesisRef,
      umbrella.sessionRef,
    ],
  );
  const policy = useCodingSessionSessionPolicy(scope);
  const policyRecordKnown = policy.fold?.selected != null;
  const verifierRequired =
    policy.fold?.selected?.record.gates?.verifierRequired ?? null;
  const requiredGates =
    policy.fold?.selected?.record.gates?.requiredGates ?? null;
  const evidence = useCodingSessionMissionEvidence(
    scope,
    undefined,
    verifierRequired,
  );
  const fold =
    ctx.observations.state === "read"
      ? (ctx.observations.result?.fold ?? null)
      : null;
  const observedGates = React.useMemo(
    () =>
      (fold?.gates ?? []).map((row) => ({
        authorPubkey: row.authorPubkey,
        source: row.source,
        gate: row.gate,
        outcome: row.outcome,
        headSha: row.headSha,
        dirty: row.dirty,
      })),
    [fold],
  );
  const gatePolicy = React.useMemo(
    () => (policyRecordKnown ? { verifierRequired, requiredGates } : null),
    [policyRecordKnown, requiredGates, verifierRequired],
  );
  const viewerPubkey = useIdentityQuery().data?.pubkey ?? null;
  const resolveWho = useCodingSessionLandingResolveWho(ctx);

  // Captured on the way through the hook's own seams, never re-read. The
  // hook asks again whenever its question changes, and drops a superseded
  // ask's answer only after it resolves — so each capture is tagged with
  // the ask it belongs to, and only the newest ask's answer is kept. An ask
  // is cancelled before it invokes when its question changes, so the newest
  // invoke started is the current question's.
  const [newestVerdict, setNewestVerdict] =
    React.useState<CodingSessionLandNewestVerdict | null>(null);
  const [repository, setRepository] =
    React.useState<CodingSessionMissionLandRepository | null>(null);
  const invokeSeq = React.useRef(0);
  const repositorySeq = React.useRef(0);
  const invoke = React.useCallback<typeof invokeCodingSessionLand>(
    async (input) => {
      invokeSeq.current += 1;
      const ask = invokeSeq.current;
      const result = await invokeCodingSessionLand(input);
      if (ask === invokeSeq.current) setNewestVerdict(result.newestVerdict);
      return result;
    },
    [],
  );
  const readRepository = React.useCallback<typeof readCodingSessionRepository>(
    async (ref, fetchEvents) => {
      repositorySeq.current += 1;
      const ask = repositorySeq.current;
      const read = await readCodingSessionRepository(ref, fetchEvents);
      if (ask === repositorySeq.current) setRepository(read);
      return read;
    },
    [],
  );
  // A different repository question drops the old announcement outright:
  // an ask that never reads one must not inherit the last session's.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset on the question's inputs
  React.useEffect(() => {
    repositorySeq.current += 1;
    invokeSeq.current += 1;
    setRepository(null);
    setNewestVerdict(null);
  }, [repoRef, projectRef, umbrella.sessionRef]);
  const { land, unavailableReason } = useCodingSessionMissionLand({
    founderPubkey: scope === null ? null : umbrella.founderPubkey,
    genesisRef: scope === null ? null : umbrella.genesisRef,
    landEvidence: evidence.inspectorInput.landEvidence,
    observedGates,
    gatePolicy,
    repoRef,
    projectRef,
    resolveWho,
    sessionRef: scope === null ? null : umbrella.sessionRef,
    viewerPubkey,
    invoke,
    readRepository,
  });

  const rule = React.useMemo<CodingSessionLandingRuleRead>(() => {
    if (scope === null) {
      return {
        state: "not-read",
        reason:
          "This session has no genesis, so there is no mission verdict or land rule to read.",
      };
    }
    if (land !== null) return { state: "read", land, newestVerdict };
    if (unavailableReason === "boundary-failed") return { state: "failed" };
    if (evidence.isLoading) return { state: "asking" };
    if (evidence.errorMessage !== null) {
      return {
        state: "not-read",
        reason: `This mission's evidence was not read: ${evidence.errorMessage}`,
      };
    }
    return codingSessionLandingRuleUnavailableReason(
      unavailableReason ?? "no-identity",
    );
  }, [
    evidence.errorMessage,
    evidence.isLoading,
    land,
    newestVerdict,
    scope,
    unavailableReason,
  ]);
  const refusingVerdict = React.useMemo(
    () => codingSessionLandingRefusingVerdict(rule, resolveWho),
    [resolveWho, rule],
  );
  return React.useMemo(
    () => ({
      refusingVerdict,
      rule,
      repository,
      repoRef,
      hasGenesis: scope !== null,
    }),
    [refusingVerdict, repoRef, repository, rule, scope],
  );
}
