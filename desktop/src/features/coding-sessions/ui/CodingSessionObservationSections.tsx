import type * as React from "react";

import {
  CODING_SESSION_OBSERVATION_EMPTY,
  CODING_SESSION_OBSERVATION_NOT_REPORTED,
  type CodingSessionObservationCheckpointView,
  type CodingSessionObservationFindingView,
  type CodingSessionObservationPhaseView,
  type CodingSessionObservationSeatBlock,
  type CodingSessionObservationView,
  gateSourceLabel,
  gateSourceTitle,
} from "@/features/coding-sessions/lib/codingSessionObservationView";
import { CodingSessionGateRows } from "./CodingSessionGateRows";

/**
 * The observer's screen: kind 44246, per author and per phase.
 *
 * Four sections — checkpoints, gate rows, findings, phase timing — inside each
 * author's own block, because the signature is the sole author of an
 * observation and nothing else on the wire says whose work a row is about.
 *
 * Within a block, everything that carries a phase word is in §1e's **declared**
 * order (`planning`, `red`, `green`, `gates`, `reporting`) — not arrival order,
 * and never author time (§8 I4). Gate rows and findings carry no phase, so
 * they keep the fold's own first-seen order with `observed` rows first.
 *
 * Every section states its own emptiness rather than rendering a blank, every
 * collection is bounded with the bound in words, and no state is carried by
 * colour alone (§8 I9, I10).
 */
export function CodingSessionObservationSections({
  view,
  loading = false,
  errorMessage = null,
}: {
  view: CodingSessionObservationView;
  loading?: boolean;
  errorMessage?: string | null;
}) {
  if (errorMessage !== null) {
    return (
      <div data-testid="coding-session-observations-error" role="alert">
        <p className="text-xs text-destructive">{errorMessage}</p>
        <p className="mt-1 text-2xs text-muted-foreground">
          Nothing here is empty — this read failed, so what this session
          observed is unknown.
        </p>
      </div>
    );
  }
  if (loading) {
    return (
      <p
        className="text-xs text-muted-foreground"
        data-testid="coding-session-observations-loading"
        role="status"
      >
        Reading this session's signed observations…
      </p>
    );
  }
  return (
    <div data-testid="coding-session-observations">
      {view.seats.length === 0 ? (
        <p className="text-xs text-muted-foreground">
          No observation has been published for this session.
          <span className="mt-1 block text-2xs">
            Kind 44246 carries checkpoints, gate rows, findings and phase
            timing. None has reached this view.
          </span>
        </p>
      ) : (
        <div className="space-y-4">
          {view.seats.map((seat) => (
            <SeatBlock key={seat.key} seat={seat} />
          ))}
        </div>
      )}

      {view.unresolved.length > 0 ? (
        <div
          className="mt-4"
          data-testid="coding-session-observations-unresolved"
        >
          <SectionHeading>Unresolved pointers</SectionHeading>
          <ul className="space-y-1">
            {view.unresolved.map((row) => (
              <li className="text-2xs text-muted-foreground" key={row.eventId}>
                Observation {row.shortEventId} names assignment{" "}
                {row.shortAssignmentRef}, which is not in this session's
                records.
              </li>
            ))}
          </ul>
          <p className="mt-1 text-2xs text-muted-foreground">
            Disclosed, never excluded: an observation cannot deny anything, its
            own pointer included.
          </p>
        </div>
      ) : null}

      {view.misclaimedObserved.length > 0 ? (
        <div
          className="mt-4"
          data-testid="coding-session-observations-misclaimed"
        >
          <SectionHeading>Claimed observed, not verified</SectionHeading>
          <ul className="space-y-1">
            {view.misclaimedObserved.map((row) => (
              <li className="text-2xs text-muted-foreground" key={row.eventId}>
                {row.shortEventId} — signed by {row.shortAuthor}, which is not a
                provider instance for this session. Shown as declared.
              </li>
            ))}
          </ul>
          <p className="mt-1 text-2xs text-muted-foreground">
            A record only counts as watched when the key that signed it is the
            mechanism doing the watching. The row still stands; its claim about
            its own provenance does not.
          </p>
        </div>
      ) : null}

      {view.seats.length > 0 && !view.provenanceChecked ? (
        // Unknown ≠ verified. A view that could not resolve this session's
        // provider instances has checked nothing, and says so rather than
        // letting every `observed` word read as a measurement.
        <p
          className="mt-2 text-2xs text-muted-foreground"
          data-testid="coding-session-observations-provenance-unchecked"
        >
          Provenance was not verified in this view: no provider instance was
          resolved for this session, so an `observed` word here is the author's
          own claim.
        </p>
      ) : null}

      {view.ignored.length > 0 ? (
        <div className="mt-4" data-testid="coding-session-observations-ignored">
          <SectionHeading>Could not be read</SectionHeading>
          <ul className="space-y-1">
            {view.ignored.map((row) => (
              <li className="text-2xs text-muted-foreground" key={row.eventId}>
                {row.shortEventId} — {row.reason}
              </li>
            ))}
          </ul>
        </div>
      ) : null}

      {view.truncations.map((truncation) => (
        <p
          className="mt-2 text-2xs text-muted-foreground"
          key={truncation.id}
          role="status"
        >
          {truncation.notice}
        </p>
      ))}

      {view.disclosure.length > 0 ? (
        <p className="mt-4 text-2xs text-muted-foreground">{view.disclosure}</p>
      ) : null}
    </div>
  );
}

function SeatBlock({ seat }: { seat: CodingSessionObservationSeatBlock }) {
  return (
    <section
      className="rounded-lg border border-border/60 p-2.5"
      data-author={seat.authorPubkey}
      data-testid="coding-session-observation-seat"
    >
      <h4 className="text-xs font-medium">{seat.label}</h4>
      {seat.watcherOnly ? (
        // The honest limit of `source: "observed"` as it stands: a 44246 names
        // the key that signed it and has no field for the subject it watched,
        // so this block says who wrote the rows and refuses to attribute them
        // to a seat it cannot name.
        <p className="mt-0.5 text-2xs text-muted-foreground">
          Watched and signed by this key. The record names the watcher, not the
          seat whose work it watched.
        </p>
      ) : null}

      <Section title="Checkpoints">
        {seat.checkpoints.length === 0 ? (
          <Empty>{CODING_SESSION_OBSERVATION_EMPTY.checkpoints}</Empty>
        ) : (
          <ul className="space-y-1.5" data-testid="coding-session-checkpoints">
            {seat.checkpoints.map((row) => (
              <CheckpointRow key={row.key} row={row} />
            ))}
          </ul>
        )}
      </Section>

      <Section title="Gate rows">
        <CodingSessionGateRows
          rows={seat.gates}
          testId="coding-session-seat-gates"
        />
      </Section>

      <Section title="Findings">
        {seat.findings.length === 0 ? (
          <Empty>{CODING_SESSION_OBSERVATION_EMPTY.findings}</Empty>
        ) : (
          <ul className="space-y-1.5" data-testid="coding-session-findings">
            {seat.findings.map((row) => (
              <FindingRow key={row.key} row={row} />
            ))}
          </ul>
        )}
      </Section>

      <Section title="Phase timing">
        {seat.phases.length === 0 ? (
          <Empty>{CODING_SESSION_OBSERVATION_EMPTY.phases}</Empty>
        ) : (
          <>
            <ul className="space-y-1" data-testid="coding-session-phases">
              {seat.phases.map((row) => (
                <PhaseRow key={row.key} row={row} />
              ))}
            </ul>
            {/* A duration list, never a bar: nothing here sets a scale, and a
                bar drawn against an invented one is a picture of a number
                nobody measured. */}
            <p className="mt-1 text-2xs text-muted-foreground">
              reported by {seat.label}
            </p>
          </>
        )}
      </Section>
    </section>
  );
}

function CheckpointRow({
  row,
}: {
  row: CodingSessionObservationCheckpointView;
}) {
  return (
    <li data-phase={row.phase} data-testid="coding-session-checkpoint-row">
      <p className="flex items-baseline justify-between gap-2 text-xs">
        <span className="font-medium">{row.phase}</span>
        <span className="shrink-0 text-muted-foreground tabular-nums">
          {row.testsWritten} written · {row.testsRed} red · {row.testsGreen}{" "}
          green
        </span>
      </p>
      {row.lastCommand === null ? null : (
        <code className="mt-0.5 block break-all font-mono text-2xs text-muted-foreground">
          {row.lastCommand}
        </code>
      )}
      <p className="text-2xs text-muted-foreground">
        {row.lastSummary ?? "No summary published."}
        {" · "}
        <span title={gateSourceTitle(row.source)}>
          {gateSourceLabel(row.source)}
        </span>
      </p>
      {row.note === null ? null : (
        <p className="mt-0.5 text-2xs whitespace-pre-wrap">{row.note}</p>
      )}
      {row.assignmentUnresolved ? <UnresolvedNote /> : null}
    </li>
  );
}

function FindingRow({ row }: { row: CodingSessionObservationFindingView }) {
  return (
    <li
      data-disposition={row.disposition}
      data-testid="coding-session-finding-row"
    >
      <p className="flex items-baseline justify-between gap-2 text-xs">
        <span className="min-w-0 truncate">
          <span className="font-medium">{row.findingId}</span> {row.title}
        </span>
        <span className="shrink-0 rounded-md border border-border/60 px-1.5 py-0.5 text-2xs text-muted-foreground">
          {row.disposition}
        </span>
      </p>
      <p className="text-2xs text-muted-foreground">
        {row.refCount} reference{row.refCount === 1 ? "" : "s"} ·{" "}
        <span title={gateSourceTitle(row.source)}>
          {gateSourceLabel(row.source)}
        </span>
        {row.decisionShortRef === null
          ? null
          : row.decisionUnresolved
            ? ` · waiting on ruling ${row.decisionShortRef} — not in this session's records`
            : ` · waiting on ruling ${row.decisionShortRef}`}
      </p>
      {row.droppedEventIds > 0 ? (
        <p className="text-2xs text-muted-foreground">
          {row.droppedEventIds} older statement
          {row.droppedEventIds === 1 ? "" : "s"} about this finding are on the
          wire and not listed here.
        </p>
      ) : null}
      {row.assignmentUnresolved ? <UnresolvedNote /> : null}
    </li>
  );
}

function PhaseRow({ row }: { row: CodingSessionObservationPhaseView }) {
  return (
    <li
      className="flex items-baseline justify-between gap-2 text-2xs"
      data-phase={row.phase}
      data-testid="coding-session-phase-row"
    >
      <span className="min-w-0 truncate">{row.phase}</span>
      <span className="shrink-0 text-muted-foreground tabular-nums">
        {row.duration ?? CODING_SESSION_OBSERVATION_NOT_REPORTED}
        {row.running ? " · still running" : null}
        {/* REVIEW-L5 F7: every row says how it was produced. The block-level
            "watched and signed by this key" line only appears when *every* row
            in the block is observed, so in a mixed block a phase timing used to
            show no provenance at all. */}
        {" · "}
        <span title={gateSourceTitle(row.source)}>
          {gateSourceLabel(row.source)}
        </span>
      </span>
    </li>
  );
}

function UnresolvedNote() {
  return (
    <p className="text-2xs text-muted-foreground">
      Its assignment is not in this session's records. The observation stands.
    </p>
  );
}

function Section({
  children,
  title,
}: {
  children: React.ReactNode;
  title: string;
}) {
  return (
    <div className="mt-2.5">
      <SectionHeading>{title}</SectionHeading>
      {children}
    </div>
  );
}

function SectionHeading({ children }: { children: React.ReactNode }) {
  return (
    <h5 className="mb-1 text-2xs font-semibold tracking-wide text-muted-foreground uppercase">
      {children}
    </h5>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return <p className="text-xs text-muted-foreground">{children}</p>;
}
