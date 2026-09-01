import { ArrowRight } from "lucide-react";

import type { CodingSessionMissionTransactionRow as MissionTransactionRow } from "@/features/coding-sessions/lib/codingSessionMissionTransactionRows";
import {
  missionRowBodyClass,
  missionRowChatBodyClass,
  missionRowClass,
  missionRowMetaClass,
  missionRowTitleClass,
} from "@/features/coding-sessions/lib/codingSessionMissionRowGrammar";
import { cn } from "@/shared/lib/cn";
import {
  CodingSessionMissionDeliveryBadge,
  CodingSessionMissionUnseatedBadge,
} from "./CodingSessionMissionDeliveryBadge";

/**
 * One signed 44244 transaction, in the stream, where it happened.
 *
 * The arrow and the monograms are decorative: the row's accessible name is the
 * sentence "<Actor> to <Counterparty>: <type>", so a screen-reader user hears
 * the handoff rather than two letters and a glyph. Weight comes from the row
 * grammar, not from this component — a refutation and a blocked mission are
 * `attention` because of what they are, not because of how they are styled.
 */
export function CodingSessionMissionTransactionRow({
  row,
}: {
  row: MissionTransactionRow;
}) {
  const counts = [
    row.fileCount === null
      ? null
      : `${row.fileCount} ${row.fileCount === 1 ? "file" : "files"}`,
    row.testCount === null
      ? null
      : `${row.testCount} ${row.testCount === 1 ? "test" : "tests"}`,
  ].filter((value): value is string => value !== null);
  return (
    <article
      aria-label={row.accessibleLabel}
      className={missionRowClass(row.weight, {
        tone: row.tone ?? undefined,
        className: "flex min-w-0 gap-2.5",
      })}
      data-testid="coding-session-mission-transaction-row"
      data-transaction-type={row.type}
      data-weight={row.weight}
    >
      <span aria-hidden className="flex shrink-0 items-start gap-1 pt-0.5">
        <Monogram party={row.actor} />
        {row.counterparty ? (
          <>
            <ArrowRight className="mt-1.5 size-3 shrink-0 text-muted-foreground" />
            <Monogram party={row.counterparty} />
          </>
        ) : null}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-1">
          <p className={cn(missionRowTitleClass(), "flex-1 truncate")}>
            {row.title}
          </p>
          <time className={cn(missionRowMetaClass(), "shrink-0 tabular-nums")}>
            {row.meta.timeLabel}
          </time>
        </div>
        {row.body ? (
          <p className={cn(missionRowChatBodyClass(), "mt-1 wrap-break-word")}>
            {row.body}
          </p>
        ) : null}
        {row.requiredAction ? (
          <p className={cn(missionRowBodyClass(), "mt-1 font-medium")}>
            Required action: {row.requiredAction}
          </p>
        ) : null}
        {counts.length > 0 ? (
          <p className={cn(missionRowMetaClass(), "mt-1")}>
            {counts.join(" · ")}
          </p>
        ) : null}
        {row.delivery || row.unseated ? (
          <div className="mt-1.5 flex min-w-0 flex-wrap items-center gap-1.5">
            {row.delivery ? (
              <CodingSessionMissionDeliveryBadge delivery={row.delivery} />
            ) : null}
            {row.unseated ? <CodingSessionMissionUnseatedBadge /> : null}
          </div>
        ) : null}
        {/*
          On an attention row the delivery sentence is read, not hovered. A
          `title` is invisible to a reader scanning for what went wrong, and the
          §2a residual — "the lead dropped it and no Desktop was present to
          cover" — is the one sentence that explains a wake nobody can find.
        */}
        {row.weight === "attention" && row.delivery ? (
          <p
            className={cn(
              missionRowBodyClass(),
              "mt-1",
              row.tone === "critical"
                ? "text-destructive"
                : "text-amber-700 dark:text-amber-300",
            )}
            data-testid="coding-session-delivery-detail"
          >
            {row.delivery.detail}
          </p>
        ) : null}
        {row.showSignedSource ? (
          <details className={cn(missionRowMetaClass(), "mt-1")}>
            <summary className="w-fit cursor-pointer rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
              Signed source
            </summary>
            <code className="mt-1 block break-all">
              {row.meta.sourceEventId}
            </code>
          </details>
        ) : null}
      </div>
    </article>
  );
}

function Monogram({ party }: { party: MissionTransactionRow["actor"] }) {
  return (
    <span
      className={cn(
        // 24px: the design canvas puts the monogram pair at chat weight so the
        // `A → B` handoff reads as a row of people, not a footnote.
        "inline-flex size-6 shrink-0 items-center justify-center rounded-full border text-xs font-semibold",
        party.accent.border,
        party.accent.soft,
        party.accent.text,
      )}
    >
      {party.monogram}
    </span>
  );
}
