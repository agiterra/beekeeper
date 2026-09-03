import { OctagonAlert, TriangleAlert } from "lucide-react";

import { codingSessionFoldExclusionCopy } from "@/features/coding-sessions/lib/codingSessionMissionContracts";
import type {
  CodingSessionMissionDisclosureInput,
  CodingSessionMissionInspectorModel,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import {
  EmptyCopy,
  SignedSource,
} from "./CodingSessionMissionInspectorPrimitives";

/**
 * The Inspector's Integrity section.
 *
 * Split out of `CodingSessionMissionInspector.tsx` when that file reached the
 * repository's 1,000-line ceiling — split, never bump. The reviewer flagged it
 * at 996 with two other lanes still writing to it (REVIEW-L7 §5.7), and the
 * state-line wiring was the line that crossed it. Nothing here changed in the
 * move except its home.
 *
 * `EmptyCopy` and `SignedSource` came out of the same file in the same wave
 * and live in `CodingSessionMissionInspectorPrimitives`; two copies of a leaf
 * that renders an empty state is exactly how two panels drift apart.
 */

export function Integrity({
  model,
}: {
  model: CodingSessionMissionInspectorModel;
}) {
  const { integrity } = model;
  const clean =
    integrity.rejectedEventCount === 0 &&
    !integrity.rejectionsTruncated &&
    integrity.rejectedReasons.length === 0 &&
    integrity.conflicts.length === 0;
  if (clean) {
    return (
      <EmptyCopy>No rejected or conflicting transaction records.</EmptyCopy>
    );
  }
  return (
    <div className="space-y-3">
      {integrity.rejectedEventCount === null ||
      integrity.rejectedEventCount > 0 ||
      integrity.rejectedReasons.length > 0 ? (
        <div className="rounded-lg border border-amber-500/45 bg-amber-500/10 p-2.5">
          <p className="flex items-center gap-2 text-xs font-medium text-amber-700 dark:text-amber-300">
            <OctagonAlert aria-hidden className="size-3.5" />
            {integrity.rejectedEventCount === null
              ? "Rejected event total unavailable after the safety bound"
              : `${integrity.rejectedEventCount} rejected ${integrity.rejectedEventCount === 1 ? "event" : "events"}`}
          </p>
          {integrity.rejectionsTruncated ? (
            <p className="mt-1 text-2xs text-muted-foreground">
              Showing {integrity.rejectedReasons.length} rejected events;
              additional unique count unavailable after the safety bound.
            </p>
          ) : null}
          {integrity.rejectedReasons.length > 0 ? (
            <DisclosureList items={integrity.rejectedReasons} />
          ) : (
            <p className="mt-1 text-2xs text-muted-foreground">
              The trusted decoder reported no bounded reason detail.
            </p>
          )}
        </div>
      ) : null}
      {integrity.conflicts.length > 0 ? (
        <div className="rounded-lg border border-amber-500/45 bg-amber-500/10 p-2.5">
          <p className="flex items-center gap-2 text-xs font-medium text-amber-700 dark:text-amber-300">
            <TriangleAlert aria-hidden className="size-3.5" />
            Conflicting signed records
          </p>
          <DisclosureList items={integrity.conflicts} />
        </div>
      ) : null}
    </div>
  );
}

/** §1k copy when a code has it; the raw wire token when it does not, because
 * inventing a sentence for a code nobody wrote one for is a guess in the
 * costume of a fact. */
function DisclosureCode({ code }: { code: string }) {
  const copy = codingSessionFoldExclusionCopy[code];
  if (!copy) return <code className="text-2xs">{code}</code>;
  const label = `${copy.word} \u00b7 ${copy.detail}`;
  return <span data-testid={`exclusion-copy-${code}`}>{label}</span>;
}

function DisclosureList({
  items,
}: {
  items: readonly CodingSessionMissionDisclosureInput[];
}) {
  return (
    <ul className="mt-2 space-y-2">
      {items.map((item) => (
        <li
          className="text-xs"
          key={`${item.code}:${item.summary}:${item.eventIds.join(":")}`}
        >
          <p>
            <DisclosureCode code={item.code} /> · {item.summary}
          </p>
          {item.eventIds.map((eventId) => (
            <SignedSource eventId={eventId} key={eventId} />
          ))}
        </li>
      ))}
    </ul>
  );
}
