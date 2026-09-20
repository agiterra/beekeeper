import type { ReactNode } from "react";

import type {
  CodingSessionMissionInspectorModel,
  CodingSessionMissionStateInput,
} from "../lib/codingSessionMissionInspectorModel";

/**
 * One titled band of the Mission inspector, with its own truncation notices.
 *
 * Lives beside the inspector rather than inside it because that file is at
 * the repository's 1,000-line ceiling: a panel is split, never the limit
 * raised.
 */
export function InspectorSection({
  children,
  title,
  truncations = [],
}: {
  children: ReactNode;
  title: string;
  truncations?: readonly CodingSessionMissionInspectorModel["truncations"][number][];
}) {
  return (
    <section className="border-b border-border/50 py-4 last:border-b-0">
      <h3 className="mb-2 text-2xs font-semibold tracking-wide text-muted-foreground uppercase">
        {title}
      </h3>
      {children}
      {truncations.map((truncation) => (
        <p
          className="mt-2 text-2xs text-amber-700 dark:text-amber-300"
          key={truncation.id}
          role="status"
        >
          {truncation.notice}
        </p>
      ))}
    </section>
  );
}

/**
 * The 44244 mission state in one line, for the coverage child's second row.
 *
 * Deliberately terse and deliberately *separate*: the panel above already
 * renders the mission state in full, and this exists only so a reader can see
 * the two answers side by side and notice when they disagree.
 */
export function missionStateRow(state: CodingSessionMissionStateInput): string {
  switch (state.kind) {
    case "unknown":
      return `unknown${state.detail ? ` — ${state.detail}` : ""}`;
    case "running":
      return `running — ${state.phase}`;
    case "acknowledgement-required":
      return `acknowledgement required — ${state.requiredAction}`;
    case "waiting-on-person":
      return `waiting on a person — ${state.requiredAction}`;
    default:
      return state.kind;
  }
}
