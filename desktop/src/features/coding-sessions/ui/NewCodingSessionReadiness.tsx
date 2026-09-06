import { CircleAlert } from "lucide-react";

import type {
  CodingSessionLaunchPlanLine,
  CodingSessionLaunchReadiness,
} from "../lib/codingSessionLaunchForm";

/**
 * What is stopping this launch, and what nobody could check.
 *
 * Two lists with different jobs, and the split is the whole design. A
 * **blocker** is inline, always visible, and phrased as something to do: it is
 * the reason the button is off, and a disabled control with nothing under it
 * is the front door refusing in silence (item 79). An **unknown** is behind a
 * disclosure: it does not stop anything, but it is never dropped, because
 * "this computer could not check" and "it is fine" are different facts and
 * only the first one is ever an excuse.
 *
 * The launch plan remains available behind its own disclosure: it names the
 * events pressing the button will publish, with kind integers, without making
 * the primary goal-and-start path read like a wire trace.
 */
export function NewCodingSessionReadiness({
  plan,
  readiness,
}: {
  plan: readonly CodingSessionLaunchPlanLine[];
  readiness: CodingSessionLaunchReadiness;
}) {
  return (
    <div
      className="flex flex-col gap-2"
      data-testid="new-coding-session-readiness"
    >
      {readiness.blockers.map((blocker) => (
        <p
          className="flex items-start gap-2 text-sm text-destructive"
          data-testid={`new-coding-session-blocker-${blocker.id}`}
          key={blocker.id}
          role="alert"
        >
          <CircleAlert className="mt-0.5 size-4 shrink-0" />
          {blocker.sentence}
        </p>
      ))}

      {readiness.unknowns.length === 0 ? null : (
        <details data-testid="new-coding-session-readiness-details">
          <summary className="cursor-pointer text-2xs text-muted-foreground">
            Details —{" "}
            {readiness.unknowns.length === 1
              ? "1 thing this computer could not check or does not enforce"
              : `${readiness.unknowns.length} things this computer could not check or does not enforce`}
          </summary>
          <ul className="mt-2 flex flex-col gap-1">
            {readiness.unknowns.map((unknown) => (
              <li
                className="text-2xs text-muted-foreground"
                data-testid={`new-coding-session-unknown-${unknown.id}`}
                key={unknown.id}
              >
                {unknown.sentence}
              </li>
            ))}
          </ul>
        </details>
      )}

      {plan.length === 0 ? null : (
        <details data-testid="new-coding-session-launch-details">
          <summary className="cursor-pointer text-2xs text-muted-foreground">
            Launch details
          </summary>
          <ol
            className="mt-2 flex flex-col gap-1"
            data-testid="new-coding-session-launch-plan"
          >
            {plan.map((line) => (
              <li
                className="text-2xs text-muted-foreground"
                data-kind={line.kind}
                data-testid={`new-coding-session-plan-${line.id}`}
                key={line.id}
              >
                {line.sentence}
              </li>
            ))}
          </ol>
        </details>
      )}
    </div>
  );
}
