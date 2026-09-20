/**
 * The provenance facts of one run, in the order a reader asks for them.
 *
 * Every entry is a *record*, not a derivation: who signed the trigger, which
 * definition the run is bound to, how the tree was established and what it
 * was before, how the command ended, how long it took, and the event that
 * proves the result. An absent fact is a named absence, never a blank cell —
 * `checkout: null` on a result recorded before lane 184 means "this record
 * does not say", which is a different claim from "the tree was clean".
 */
import type {
  ProjectWorkflowHostStep,
  ProjectWorkflowRun,
} from "@/shared/api/tauriWorkflows";
import { truncatePubkey } from "@/shared/lib/pubkey";

export type RunProvenanceFact = {
  label: string;
  /** The value, or `null` when the records do not carry it. */
  value: string | null;
  /** Why it is absent, in words. `null` exactly when `value` is set. */
  reason: string | null;
  /** Rendered in a monospace run for shas, hashes and event ids. */
  mono?: boolean;
};

function shortHex(value: string): string {
  return value.length > 12 ? `${value.slice(0, 12)}…` : value;
}

function formatDuration(ms: number): string {
  if (ms < 1_000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1_000).toFixed(ms < 10_000 ? 1 : 0)} s`;
  const minutes = Math.floor(ms / 60_000);
  const seconds = Math.round((ms % 60_000) / 1_000);
  return `${minutes} m ${seconds} s`;
}

/** How the tree a step ran in was established, as one sentence. */
export function checkoutSentence(step: ProjectWorkflowHostStep): {
  value: string | null;
  reason: string | null;
} {
  const checkout = step.checkout;
  if (!checkout) {
    return {
      value: null,
      reason:
        "this result predates the checkout record, so it does not say how its tree was established",
    };
  }
  const parts: string[] = [];
  parts.push(checkout.mode ?? "mode not recorded");
  if (checkout.sha) parts.push(`at ${checkout.sha}`);
  if (checkout.headShaBefore)
    parts.push(`head before ${checkout.headShaBefore}`);
  parts.push(
    checkout.dirtyBefore === null
      ? "dirty before not recorded"
      : checkout.dirtyBefore
        ? "dirty before"
        : "clean before",
  );
  return { value: parts.join(" · "), reason: null };
}

/**
 * The provenance rows of one run, with the latest host step's facts.
 *
 * `hostStepsUnreadable` is the read's own failure and is reported as such:
 * an unreadable host-step listing must never render as a run with no host
 * step, which is what a plain empty array would say.
 */
export function runProvenanceFacts(
  run: ProjectWorkflowRun,
  hostSteps: readonly ProjectWorkflowHostStep[],
  hostStepsUnreadable: string | null,
): RunProvenanceFact[] {
  const facts: RunProvenanceFact[] = [
    {
      label: "Triggered by",
      value: run.triggerAuthor ? truncatePubkey(run.triggerAuthor) : null,
      reason: run.triggerAuthor
        ? null
        : "this run's record names no trigger author (a schedule, a ref update or a relay that predates the field)",
    },
    {
      label: "Definition",
      value: run.definitionHash ? shortHex(run.definitionHash) : null,
      reason: run.definitionHash
        ? null
        : "this run was created before runs were bound to a definition, so it names none",
      mono: true,
    },
  ];

  if (hostStepsUnreadable !== null) {
    facts.push({
      label: "Host step",
      value: null,
      reason: `the host-step record could not be read: ${hostStepsUnreadable}`,
    });
    return facts;
  }

  const step = [...hostSteps].sort(
    (a, b) =>
      b.stepIndex - a.stepIndex || b.createdAt.localeCompare(a.createdAt),
  )[0];
  if (!step) {
    facts.push({
      label: "Host step",
      value: null,
      reason: "this run recorded no host step",
    });
    return facts;
  }

  const checkout = checkoutSentence(step);
  facts.push(
    { label: "Checkout", ...checkout, mono: true },
    {
      label: "Exit code",
      value: step.exitCode === null ? null : String(step.exitCode),
      reason:
        step.exitCode === null
          ? `the step is ${step.status} and no exit code is recorded`
          : null,
    },
    {
      label: "Duration",
      value: step.durationMs === null ? null : formatDuration(step.durationMs),
      reason: step.durationMs === null ? "no duration is recorded" : null,
    },
    {
      label: "Result event",
      value: step.resultEventId ? shortHex(step.resultEventId) : null,
      reason: step.resultEventId
        ? null
        : "no result event has been recorded for this step",
      mono: true,
    },
  );
  return facts;
}
