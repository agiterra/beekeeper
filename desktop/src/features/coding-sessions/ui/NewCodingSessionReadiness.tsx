import { CircleAlert } from "lucide-react";

import type {
  CodingSessionLaunchPlanLine,
  CodingSessionLaunchReadiness,
} from "../lib/codingSessionLaunchForm";

/**
 * What is stopping this launch, and — where a surface wants them — what
 * nobody could check and what pressing the button will publish.
 *
 * A **blocker** is inline, always visible, and phrased as something to do:
 * it is the reason the button is off, and a disabled control with nothing
 * under it is the front door refusing in silence (item 79). Callers hand in
 * only the blockers that belong under the button; one surfaced on press
 * (the blank initial prompt) is rendered by the field it belongs to.
 *
 * The **unknowns** disclosure and the **plan** are optional. The founded
 * page's setup card — the one launch form since 2026-09-10, the create
 * dialog being gone — shows both, because its Start signs a name, a prompt,
 * a policy, a seat, grants and a turn, and names what it will not check. The
 * "no Details" decision (Andy, 2026-09-10: "they don't add any value to the
 * dialog") was about the dialog.
 */
export function NewCodingSessionReadiness({
  plan = [],
  readiness,
  unknownsDisclosed = true,
}: {
  plan?: readonly CodingSessionLaunchPlanLine[];
  readiness: CodingSessionLaunchReadiness;
  /** False hides the "Details" disclosure of unknowns entirely. */
  unknownsDisclosed?: boolean;
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

      {!unknownsDisclosed || readiness.unknowns.length === 0 ? null : (
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
                data-kind={line.kind ?? undefined}
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
