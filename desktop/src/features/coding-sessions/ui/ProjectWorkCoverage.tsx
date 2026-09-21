import { cn } from "@/shared/lib/cn";
import type {
  ProjectWorkCriterion,
  ProjectWorkDeclaration,
  ProjectWorkResponse,
} from "@/shared/api/tauriProjectWork";

import {
  criterionStatusLabel,
  declarationStateLabel,
  nextStep,
  orderedDeclarations,
  owedBy,
  planLabel,
  shortCommit,
} from "../lib/projectWork";

const STATUS_CLASS: Record<string, string> = {
  covered: "text-emerald-600 dark:text-emerald-400",
  open: "text-foreground",
  stale: "text-amber-600 dark:text-amber-400",
  unknown: "text-muted-foreground",
};

/**
 * What proves this criterion — or that we cannot say.
 *
 * `proof` is `null` exactly when the plan blob was unreadable at its pinned
 * commit, and the criterion is then `unknown` with its own reason. Printing
 * "proved by undefined" (or throwing) would turn an honest gap into a lie
 * about the contract.
 */
function proofLabel(proof: ProjectWorkCriterion["proof"]): string {
  if (!proof) return "proof unknown: the plan could not be read at its commit";
  return proof.kind === "action"
    ? `proved by action ${proof.name} · step ${proof.step}`
    : `proved by ${proof.kind}`;
}

function Criterion({
  criterion,
  resolveActorName,
}: {
  criterion: ProjectWorkCriterion;
  resolveActorName?: (pubkey: string) => string;
}) {
  return (
    <li
      className="flex flex-col gap-0.5 border-t border-border/50 py-1.5"
      data-criterion-id={criterion.criterionId}
      data-status={criterion.status}
      data-testid="project-work-criterion"
    >
      <div className="flex flex-wrap items-baseline gap-2">
        <span className="font-mono text-xs">{criterion.criterionId}</span>
        <span className={cn("text-xs", STATUS_CLASS[criterion.status])}>
          {criterionStatusLabel(criterion.status)}
        </span>
        <span className="text-2xs text-muted-foreground">
          {proofLabel(criterion.proof)}
        </span>
      </div>
      {criterion.reason ? (
        <p className="text-2xs text-muted-foreground">{criterion.reason}</p>
      ) : null}
      <p className="text-2xs text-muted-foreground">
        {owedBy(criterion, resolveActorName)}
      </p>
      {criterion.evidence.length > 0 ? (
        <p className="flex flex-wrap gap-2 text-2xs text-muted-foreground">
          {criterion.evidence.map((item) => (
            <span
              className="font-mono"
              data-testid="project-work-evidence"
              key={`${item.kind}:${item.eventId}`}
              title={item.eventId}
            >
              {item.kind} {item.eventId.slice(0, 8)}…
            </span>
          ))}
        </p>
      ) : (
        <p className="text-2xs text-muted-foreground">no evidence is bound</p>
      )}
      {criterion.artifactCommit ? (
        <p className="font-mono text-2xs text-muted-foreground">
          at {shortCommit(criterion.artifactCommit)}
        </p>
      ) : null}
    </li>
  );
}

function Declaration({
  declaration,
  resolveActorName,
}: {
  declaration: ProjectWorkDeclaration;
  resolveActorName?: (pubkey: string) => string;
}) {
  return (
    <section
      className="rounded-lg border border-border/60 p-2"
      data-state={declaration.state}
      data-testid="project-work-declaration"
      data-work-id={declaration.workId}
    >
      <header className="flex flex-col gap-0.5">
        <span className="break-all font-mono text-xs">
          {planLabel(declaration)}
        </span>
        <span
          className={cn(
            "text-2xs",
            declaration.state === "head"
              ? "text-muted-foreground"
              : "text-amber-600 dark:text-amber-400",
          )}
          data-testid="project-work-declaration-state"
        >
          {declarationStateLabel(declaration)}
          {declaration.stateReason ? ` — ${declaration.stateReason}` : ""}
        </span>
      </header>
      {declaration.planResolved ? null : (
        <p
          className="mt-1 text-2xs text-muted-foreground"
          data-testid="project-work-plan-unresolved"
        >
          This contract&apos;s plan was not read at its commit, so its criteria
          are unknown — not open. Nothing here says work was or was not done.
        </p>
      )}
      {declaration.criteria.length > 0 ? (
        <ul className="mt-1">
          {declaration.criteria.map((criterion) => (
            <Criterion
              criterion={criterion}
              key={criterion.criterionId}
              resolveActorName={resolveActorName}
            />
          ))}
        </ul>
      ) : null}
      <footer className="mt-1 flex flex-col gap-0.5 border-t border-border/50 pt-1">
        <span className="text-2xs text-muted-foreground">
          candidate artifact:{" "}
          <span className="font-mono">
            {declaration.candidateArtifact
              ? shortCommit(declaration.candidateArtifact)
              : "none yet"}
          </span>
          {declaration.artifactCommits.length > 1
            ? ` · evidence names ${declaration.artifactCommits.length} different commits`
            : ""}
        </span>
        <span
          className={cn(
            "text-2xs",
            declaration.coverageComplete
              ? "text-emerald-600 dark:text-emerald-400"
              : "text-muted-foreground",
          )}
          data-testid="project-work-coverage-complete"
        >
          {declaration.coverageComplete
            ? "coverage complete"
            : `coverage incomplete${declaration.coverageReason ? ` — ${declaration.coverageReason}` : ""}`}
        </span>
      </footer>
    </section>
  );
}

/**
 * What the project's adopted contract says remains, and what proves the rest.
 *
 * Every fact here is the native projection rendered **verbatim** — status,
 * reason, candidate artifact and `coverageComplete` are `buzz-core`'s answers,
 * not this component's. The only thing it decides is which single next step
 * to offer, and it names the fact that releases it.
 *
 * Two rules the layout enforces:
 *
 * - **Conflict, stale, superseded and unknown each render differently**, and
 *   none of them collapses into "not covered". A contract nobody can read and
 *   a contract nobody has worked on are opposite claims.
 * - **The 44244 mission state is a separate row and is never merged in.** A
 *   terminal mission over incomplete coverage is exactly what an older
 *   client's completion looks like, and it must be visible as a
 *   *disagreement* rather than reconciled into one number.
 */
export function ProjectWorkCoverage({
  response,
  errorMessage = null,
  loading = false,
  missionRow,
  resolveActorName,
}: {
  /** `null` means no fold has answered — unknown, never "nothing remains". */
  response: ProjectWorkResponse | null;
  errorMessage?: string | null;
  loading?: boolean;
  /** The 44244 mission state, in its own words, for the separate row. */
  missionRow?: string | null;
  resolveActorName?: (pubkey: string) => string;
}) {
  if (errorMessage) {
    return (
      <p
        className="text-xs text-destructive"
        data-testid="project-work-error"
        role="alert"
      >
        Work coverage could not be read: {errorMessage}
      </p>
    );
  }
  if (loading) {
    return (
      <p className="text-xs text-muted-foreground" role="status">
        Reading this session&apos;s signed work records…
      </p>
    );
  }
  if (!response) {
    return (
      <p
        className="text-xs text-muted-foreground"
        data-testid="project-work-unknown"
      >
        Work coverage unknown: no fold has answered for this session yet.
      </p>
    );
  }
  const step = nextStep(response);
  const declarations = orderedDeclarations(response.coverage);
  return (
    <div className="flex flex-col gap-2" data-testid="project-work-coverage">
      {declarations.length === 0 ? (
        <p className="text-xs text-muted-foreground">
          No plan has been adopted for this session, so there is no contract to
          measure against.
        </p>
      ) : (
        declarations.map((declaration) => (
          <Declaration
            declaration={declaration}
            key={declaration.declarationRef}
            resolveActorName={resolveActorName}
          />
        ))
      )}
      {response.coverage.conflicts.map((conflict) => (
        <p
          className="text-xs text-amber-600 dark:text-amber-400"
          data-testid="project-work-conflict"
          key={conflict.workId}
        >
          {conflict.message}
        </p>
      ))}
      {response.unreadablePlans.map((plan) => (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="project-work-unreadable-plan"
          key={`${plan.repository}@${plan.commit}:${plan.path}`}
        >
          {plan.path} at {shortCommit(plan.commit)} — {plan.reasonCode}:{" "}
          {plan.reason}
        </p>
      ))}
      {/*
        Two rows, never one. The contract is explicit that a disagreement
        between coverage and the mission terminal is a *disclosure*: nothing
        here merges them, lets one imply the other, or lets coverage publish
        or withhold a completion.
      */}
      <p
        className="border-t border-border/50 pt-1 text-2xs text-muted-foreground"
        data-testid="project-work-mission-row"
      >
        mission : {missionRow ?? "unknown — the 44244 fold has not answered"}
      </p>
      {step ? (
        <p className="text-xs text-foreground" data-testid="project-work-next">
          Next: {step.text}{" "}
          <span className="text-muted-foreground">
            (released by {step.releasedBy})
          </span>
        </p>
      ) : null}
    </div>
  );
}
