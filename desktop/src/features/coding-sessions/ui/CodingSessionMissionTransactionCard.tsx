import {
  ArrowRight,
  CheckCircle2,
  CircleDashed,
  Clock3,
  OctagonAlert,
  TriangleAlert,
} from "lucide-react";

import type {
  CodingSessionMissionCanonicalStep,
  CodingSessionMissionStateInput,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import { cn } from "@/shared/lib/cn";

export function CodingSessionMissionTransactionCard({
  state,
}: {
  state: CodingSessionMissionStateInput;
}) {
  const presentation = present(state);
  const Icon = presentation.icon;
  return (
    <article
      aria-label={`Mission transaction state: ${presentation.label}`}
      className={cn("mb-5 rounded-xl border p-3", presentation.classes)}
      data-mission-state={state.kind}
      data-testid="coding-session-mission-transaction-card"
    >
      <div className="flex items-start gap-2.5">
        <Icon aria-hidden className="mt-0.5 size-4 shrink-0" />
        <div className="min-w-0 flex-1">
          <p className="text-xs font-semibold">{presentation.label}</p>
          <p className="mt-1 text-xs text-current/80">{presentation.detail}</p>
          {state.kind === "blocked" ? (
            <>
              <ul className="mt-2 list-disc space-y-1 pl-4 text-xs">
                {state.blockers.map((blocker) => (
                  <li key={blocker}>{blocker}</li>
                ))}
              </ul>
              <p className="mt-2 text-xs font-medium">
                Required action: {state.requiredAction}
              </p>
            </>
          ) : state.kind === "waiting-on-person" ||
            state.kind === "acknowledgement-required" ? (
            <p className="mt-2 text-xs font-medium">
              Required action: {state.requiredAction}
            </p>
          ) : null}
          {"canonicalChain" in state && state.canonicalChain ? (
            <CanonicalChain steps={state.canonicalChain} />
          ) : null}
          {"sourceEventId" in state ? (
            <code className="mt-2 block truncate text-2xs text-current/70">
              Signed source {state.sourceEventId}
            </code>
          ) : null}
        </div>
      </div>
    </article>
  );
}

function present(state: CodingSessionMissionStateInput) {
  switch (state.kind) {
    case "completed":
      return {
        label: "Mission completed",
        detail: state.summary,
        icon: CheckCircle2,
        classes:
          "border-emerald-500/35 bg-emerald-500/10 text-emerald-800 dark:text-emerald-200",
      };
    case "blocked":
      return {
        label: "Mission blocked",
        detail: state.summary,
        icon: OctagonAlert,
        classes: "border-destructive/40 bg-destructive/10 text-destructive",
      };
    case "waiting-on-person":
      return {
        label: "Waiting on a person",
        detail: state.heldOn
          ? `Held on ${state.heldOn}.`
          : "A canonical transaction requires acknowledgement.",
        icon: Clock3,
        classes:
          "border-amber-500/40 bg-amber-500/10 text-amber-800 dark:text-amber-200",
      };
    case "acknowledgement-required":
      return {
        label: "Acknowledgement required",
        detail: state.heldOn
          ? `The approved disposition is held on the ${state.heldOn} seat.`
          : "The approved disposition is waiting for its assigned seat.",
        icon: Clock3,
        classes:
          "border-amber-500/40 bg-amber-500/10 text-amber-800 dark:text-amber-200",
      };
    case "stalled":
      return {
        label: "Mission stalled",
        detail: state.detail,
        icon: TriangleAlert,
        classes:
          "border-amber-500/40 bg-amber-500/10 text-amber-800 dark:text-amber-200",
      };
    case "running":
      return {
        label: "Mission running",
        detail: state.detail,
        icon: CircleDashed,
        classes: "border-border/70 bg-muted/25 text-foreground",
      };
    case "conflict":
      return {
        label: "Mission state conflict",
        detail: `${state.eventIds.length} signed terminal records conflict.`,
        icon: OctagonAlert,
        classes: "border-destructive/40 bg-destructive/10 text-destructive",
      };
    default:
      return {
        label: "Mission state unknown",
        detail:
          state.detail ??
          "No canonical typed transaction establishes the current state. Silence is not completion.",
        icon: CircleDashed,
        classes: "border-border/70 bg-muted/25 text-muted-foreground",
      };
  }
}

function CanonicalChain({
  steps,
}: {
  steps: readonly CodingSessionMissionCanonicalStep[];
}) {
  return (
    <div className="mt-3 border-t border-current/20 pt-2">
      <nav
        aria-label="Signed team handoff flow"
        className="mb-2 flex flex-wrap items-center gap-1.5"
        data-testid="coding-session-mission-handoff-flow"
      >
        {steps.map((step, index) => (
          <div className="contents" key={step.sourceEventId}>
            {index > 0 ? (
              <ArrowRight aria-hidden className="size-3 shrink-0 opacity-60" />
            ) : null}
            <span className="rounded-full border border-current/25 bg-background/35 px-2 py-1 text-2xs font-semibold capitalize">
              {step.type}
            </span>
          </div>
        ))}
      </nav>
      <ol
        aria-label="Canonical team transaction chronology"
        className="space-y-1.5"
        data-testid="coding-session-mission-canonical-chain"
      >
        {steps.map((step) => (
          <li className="min-w-0 text-2xs" key={step.sourceEventId}>
            <div className="flex items-baseline justify-between gap-3">
              <span className="font-semibold capitalize">{step.type}</span>
              <time className="shrink-0 text-current/65">
                {new Date(step.createdAt * 1000).toLocaleTimeString([], {
                  hour: "numeric",
                  minute: "2-digit",
                })}
              </time>
            </div>
            <p className="mt-0.5 line-clamp-2 text-current/80">
              {step.summary}
            </p>
            {step.decision ? (
              <p className="mt-0.5 text-current/80">
                Decision: {step.decision}
              </p>
            ) : null}
            {step.requiredAction ? (
              <p className="mt-0.5 font-medium text-current/90">
                Required action: {step.requiredAction}
              </p>
            ) : null}
            <code className="mt-0.5 block truncate text-current/65">
              Signed source {step.sourceEventId}
            </code>
          </li>
        ))}
      </ol>
    </div>
  );
}
