/**
 * Declared work: what participants said they are doing, joined across the
 * sessions this read could see.
 *
 * Two kinds of declaration share the list. A **plan** is an active kind 44240
 * entry, rendered by the unchanged `PulseEntryRow` under a "Plan posted" chip —
 * regrouped from the Entries list above, never duplicated into a second plan
 * list. An **assignment** is one canonically included kind 44244 assignment,
 * with the source fields the fold proved: who is responsible, the paths they
 * declared, the branch and base they named, and the evidence — a report, a
 * disposition, a settlement, a terminal, a closed session — that the fold
 * actually established.
 *
 * This component renders a model it is handed. It fetches nothing, folds
 * nothing, and re-verifies nothing on hover, scroll or disclosure: every word
 * below was decided by `projectPulseDeclaredWork` from a native projection.
 *
 * Three honesty rules it exists to keep:
 *
 * - An unreadable read is disclosed as a failure. "No declared work" is a
 *   claim about a complete read and never stands in for one that broke.
 * - Closing a session settles nothing. An unresolved assignment in a closed
 *   session keeps its place in the current list, with "Session closed" as
 *   evidence next to it rather than as an outcome.
 * - A report is evidence of a report. The status word comes from the fold; no
 *   sentence here promotes a report into "done".
 *
 * Contract: `docs/DECLARED_WORK_PULSE_IMPL.md` §5 and §6.
 */
import type * as React from "react";
import { ClipboardList, ListTodo, Loader2, TriangleAlert } from "lucide-react";

import { cn } from "@/shared/lib/cn";
// Pubkeys are shortened by the app's one pubkey convention and commits by the
// model's `shortSha`: a founder key and a base sha must not look alike.
import { truncatePubkey } from "@/shared/lib/pubkey";

import { pulseAuthorLabel, type PulseAuthorNames } from "../lib/pulseAuthors";
import {
  type PulseDeclaredWorkModel,
  type PulseDeclaredWorkRow,
  shortSha,
} from "../lib/pulseDeclaredWork";
import { formatPulseAge } from "../lib/pulseFormat";
import type { PulseDigestEntry, PulseDigestSession } from "../lib/pulseFold.ts";
import { PulseEntryRow } from "./PulseEntryRow";

/** What the surface knows about the declared-work read right now. */
export type PulseDeclaredWorkSectionState = "loading" | "ready" | "unreadable";

/** Everything this section renders. Callbacks only; it fetches nothing. */
export type PulseDeclaredWorkSectionProps = {
  model: PulseDeclaredWorkModel | null;
  state: PulseDeclaredWorkSectionState;
  /** The read's own failure sentence, when it failed. */
  message: string | null;
  nowSeconds: number;
  authorNames?: PulseAuthorNames;
  entriesById?: ReadonlyMap<string, PulseDigestEntry>;
  sessionsByRef?: ReadonlyMap<string, PulseDigestSession>;
  /**
   * Entries whose refused cross-author supersession claim names a given entry,
   * keyed by the named entry's id — the screen's own
   * `crossAuthorClaimsByTarget` map.
   *
   * Threaded through because a regrouped plan is the *same row* the Entries
   * list drew: without it, "someone else says this is resolved — not applied"
   * silently disappears the moment a plan moves into this section, which is
   * exactly the disclosure a reader of a coordination screen needs most.
   */
  claimedBy?: ReadonlyMap<string, readonly PulseDigestEntry[]>;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  onLoadMore: () => void;
  /** Re-reads what is already loaded. It starts nothing. */
  onRecheck: () => void;
  refreshing: boolean;
  onOpenSession?: (sessionKey: string) => void;
  /** Whether an execution is recorded for this session key at all. */
  sessionOpenable: (sessionKey: string) => boolean;
  /**
   * What a filter above this section is hiding, in the caller's words.
   *
   * Present only when the screen's branch filter is on. It is rendered next to
   * the scan sentence and it changes what an empty list is allowed to mean:
   * rows hidden by a filter are not a project with no declared work.
   */
  filterNote?: string | null;
};

/** The heading id the section is labelled by. */
const HEADING_ID = "pulse-declared-work-heading";

/** The label chip every row leads with — provenance in two or three words. */
function DeclaredLabel({
  children,
  icon,
  tone,
}: {
  children: React.ReactNode;
  icon: React.ReactNode;
  tone: "plan" | "assignment";
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-2xs font-medium",
        tone === "assignment"
          ? "bg-primary/15 text-primary"
          : "bg-muted text-foreground",
      )}
      data-testid="pulse-declared-label"
    >
      <span aria-hidden>{icon}</span>
      {children}
    </span>
  );
}

/**
 * The status word, always written out.
 *
 * The tint is decoration; the word is the fact. A settled assignment and an
 * unresolved one must be distinguishable to a reader who cannot see the
 * colour, which is why the word is never replaced by a dot.
 */
function DeclaredStatus({ status }: { status: string }) {
  const word =
    status === "settled"
      ? "Settled"
      : status === "reported"
        ? "Reported"
        : "Unresolved";
  return (
    <span
      className={cn(
        "rounded px-1.5 py-0.5 text-2xs font-medium",
        status === "settled"
          ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400"
          : "bg-muted text-muted-foreground",
      )}
      data-testid="pulse-declared-status"
    >
      {word}
    </span>
  );
}

/** One assignment, as the fold proved it. */
function DeclaredAssignmentRow({
  nowSeconds,
  onOpenSession,
  row,
  authorNames,
  sessionOpenable,
}: {
  nowSeconds: number;
  onOpenSession?: (sessionKey: string) => void;
  row: Extract<PulseDeclaredWorkRow, { kind: "assignment" }>;
  authorNames?: PulseAuthorNames;
  sessionOpenable: (sessionKey: string) => boolean;
}) {
  const assignment = row.assignment;
  // Every field below is on `PulseDeclaredWorkRowSession`, including the
  // umbrella's immutable `genesisRef`, so the type is the guard: a producer
  // that stopped sending one is a compile error in the projection, not a
  // sentence this component has to invent at runtime.
  const {
    channelId,
    genesisRef,
    founderPubkey,
    lifecycle,
    name: sessionName,
    sessionKey,
    sessionRef,
  } = row.session;
  const responsible = pulseAuthorLabel(row.responsible.pubkey, authorNames);
  const assigner = pulseAuthorLabel(row.assignedBy, authorNames);
  const openable = sessionOpenable(sessionKey);

  return (
    <li
      className="rounded-md border border-border/60 bg-background/40 p-3"
      data-kind="assignment"
      data-session-key={sessionKey}
      data-status={assignment.status}
      data-testid="pulse-declared-row"
    >
      <div className="flex flex-wrap items-center gap-2 text-2xs text-muted-foreground">
        <DeclaredLabel
          icon={<ClipboardList className="size-3" />}
          tone="assignment"
        >
          {row.label}
        </DeclaredLabel>
        <DeclaredStatus status={assignment.status} />
        <span
          data-testid="pulse-declared-age"
          title={new Date(assignment.createdAt * 1_000).toISOString()}
        >
          {formatPulseAge(nowSeconds - assignment.createdAt)} ago
        </span>
      </div>

      <p
        className="mt-1.5 whitespace-pre-wrap text-sm text-foreground"
        data-testid="pulse-declared-objective"
      >
        {assignment.objective}
      </p>

      <p className="mt-1 text-xs" data-testid="pulse-declared-responsible">
        <span className="text-foreground" title={row.responsible.pubkey}>
          Assigned to {responsible} as {row.responsible.role}
        </span>{" "}
        <span
          className="text-2xs text-muted-foreground"
          data-testid="pulse-declared-assigner"
          title={row.assignedBy}
        >
          by {assigner}
        </span>
      </p>

      <div className="mt-2 text-2xs" data-testid="pulse-declared-scope">
        {assignment.fileOwnership.length > 0 ? (
          <>
            <span className="text-muted-foreground">Declared paths</span>
            <ul className="mt-0.5 flex flex-wrap gap-1">
              {assignment.fileOwnership.map((path) => (
                <li
                  className="rounded bg-muted px-1.5 py-0.5 font-mono text-muted-foreground"
                  data-testid="pulse-declared-path"
                  key={path}
                  title="Declared by its author; nothing here was compared to another declaration or observed."
                >
                  {path}
                </li>
              ))}
            </ul>
          </>
        ) : (
          <span className="text-muted-foreground">No declared paths</span>
        )}
      </div>

      <p
        className="mt-2 text-2xs text-muted-foreground"
        data-testid="pulse-declared-branch"
      >
        {assignment.branch === null ? (
          "Branch not reported"
        ) : (
          <>
            Branch {assignment.branch}
            {assignment.baseSha === null
              ? ""
              : ` · base ${shortSha(assignment.baseSha)}`}
          </>
        )}
      </p>

      <p
        className="mt-1 text-2xs text-muted-foreground"
        data-testid="pulse-declared-session"
      >
        {sessionName ? `Session “${sessionName}”` : "Unnamed session"}
        {lifecycle === "closed" ? " · Session closed" : " · Session open"}
      </p>

      {row.evidence.length > 0 ? (
        <ul className="mt-2 flex flex-col gap-0.5">
          {row.evidence.map((evidence) => (
            <li
              className="text-xs text-muted-foreground"
              data-evidence-label={evidence.label}
              data-testid="pulse-declared-evidence"
              key={`${evidence.label}:${evidence.eventId ?? ""}:${evidence.detail}`}
              title={evidence.eventId ? `Event ${evidence.eventId}` : undefined}
            >
              <span className="font-medium text-foreground">
                {evidence.label}
              </span>{" "}
              {evidence.detail}
            </li>
          ))}
        </ul>
      ) : null}

      <details className="mt-2" data-testid="pulse-declared-details">
        <summary className="cursor-pointer text-2xs text-muted-foreground">
          Brief, acceptance steps and identifiers
        </summary>
        <div className="mt-1 flex flex-col gap-1 text-2xs text-muted-foreground">
          <p className="whitespace-pre-wrap" data-testid="pulse-declared-brief">
            {assignment.brief === "" ? "No brief recorded." : assignment.brief}
          </p>
          {assignment.acceptanceSteps.length > 0 ? (
            <ul className="list-disc pl-4">
              {assignment.acceptanceSteps.map((step) => (
                <li data-testid="pulse-declared-acceptance-step" key={step}>
                  {step}
                </li>
              ))}
            </ul>
          ) : (
            <p data-testid="pulse-declared-acceptance-empty">
              No acceptance steps recorded.
            </p>
          )}
          <p className="font-mono" data-testid="pulse-declared-source-id">
            Source event {assignment.sourceEventId}
          </p>
          {/* The identifiers a reader can carry to another tool. The genesis
              id is *not* one of them: the declared-work response does not
              carry it, and a founder pubkey is not a substitute — so the
              absence is stated rather than filled with the nearest field. */}
          <p className="font-mono" data-testid="pulse-declared-session-ids">
            Session {sessionRef} · channel {channelId} · founder{" "}
            {truncatePubkey(founderPubkey)}
          </p>
          {/* Full hex, not shortened: this is the id a reader carries to
              another tool to pull the umbrella's own record. */}
          <p className="font-mono" data-testid="pulse-declared-genesis">
            Genesis {genesisRef}
          </p>
        </div>
      </details>

      <div className="mt-2">
        {openable ? (
          <button
            className="rounded border border-border px-2 py-0.5 text-2xs text-foreground hover:bg-muted"
            data-testid="pulse-declared-open-session"
            onClick={() => onOpenSession?.(sessionKey)}
            type="button"
          >
            Open session
          </button>
        ) : (
          <p
            className="text-2xs text-muted-foreground"
            data-testid="pulse-declared-open-session-missing"
          >
            No execution recorded to open
          </p>
        )}
      </div>
    </li>
  );
}

/** One plan, under its chip: the same row the Entries list renders. */
function DeclaredPlanRow({
  authorNames,
  claimants,
  entriesById,
  nowSeconds,
  row,
  sessionsByRef,
}: {
  authorNames?: PulseAuthorNames;
  claimants?: readonly PulseDigestEntry[];
  entriesById?: ReadonlyMap<string, PulseDigestEntry>;
  nowSeconds: number;
  row: Extract<PulseDeclaredWorkRow, { kind: "plan" }>;
  sessionsByRef?: ReadonlyMap<string, PulseDigestSession>;
}) {
  return (
    <li
      data-kind="plan"
      data-event-id={row.entry.eventId}
      data-testid="pulse-declared-row"
    >
      <DeclaredLabel icon={<ListTodo className="size-3" />} tone="plan">
        {row.label}
      </DeclaredLabel>
      {/* `PulseEntryRow` renders an `<li>`; wrapping it keeps the markup valid
          without forking a second rendering of the same entry. */}
      <ul className="mt-1">
        <PulseEntryRow
          authorNames={authorNames}
          claimants={claimants ?? []}
          entriesById={entriesById}
          entry={row.entry}
          nowSeconds={nowSeconds}
          sessionsByRef={sessionsByRef}
        />
      </ul>
    </li>
  );
}

/**
 * The key one row is rendered under.
 *
 * The projection's own dedupe key, which already carries channel, session and
 * source event — a positional key would move one declaration's disclosure
 * state onto another when a refetch reorders the list.
 */
function rowKey(row: PulseDeclaredWorkRow): string {
  return row.dedupeKey;
}

/**
 * The declared-work section: plans and assignments, with what the read lost.
 */
export function PulseDeclaredWorkSection({
  authorNames,
  claimedBy,
  entriesById,
  filterNote = null,
  hasNextPage,
  isFetchingNextPage,
  message,
  model,
  nowSeconds,
  onLoadMore,
  onOpenSession,
  onRecheck,
  refreshing,
  sessionOpenable,
  sessionsByRef,
  state,
}: PulseDeclaredWorkSectionProps) {
  const renderRow = (row: PulseDeclaredWorkRow) =>
    row.kind === "plan" ? (
      <DeclaredPlanRow
        authorNames={authorNames}
        claimants={claimedBy?.get(row.entry.eventId)}
        entriesById={entriesById}
        key={rowKey(row)}
        nowSeconds={nowSeconds}
        row={row}
        sessionsByRef={sessionsByRef}
      />
    ) : (
      <DeclaredAssignmentRow
        authorNames={authorNames}
        key={rowKey(row)}
        nowSeconds={nowSeconds}
        onOpenSession={onOpenSession}
        row={row}
        sessionOpenable={sessionOpenable}
      />
    );

  const current = model?.current ?? [];
  const settled = model?.settled ?? [];
  const limitations = model?.limitations ?? [];
  // Whether this read covered its stated scope. The projection decides it:
  // every visible session reached, its records readable, no page failed, no
  // cap hit. It is *not* the same question as "was anything declared" — a
  // complete read whose only work is settled is still complete, and saying
  // "the read is incomplete" over a section that is showing everything it
  // found is the failure this flag exists to prevent.
  const readIsComplete = model?.readIsComplete === true;
  const updateFailed = state === "ready" && message !== null;

  return (
    <section
      aria-labelledby={HEADING_ID}
      className="flex flex-col gap-2"
      data-state={state}
      data-testid="pulse-declared-work"
    >
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-sm font-medium text-foreground" id={HEADING_ID}>
          Declared work
        </h2>
        {refreshing ? (
          <span
            className="inline-flex items-center gap-1 text-2xs text-muted-foreground"
            data-testid="pulse-declared-refreshing"
          >
            <Loader2 className="size-3 animate-spin" aria-hidden />
            showing last complete read
          </span>
        ) : null}
        <button
          className="ml-auto rounded border border-border px-2 py-0.5 text-2xs text-foreground hover:bg-muted"
          data-testid="pulse-declared-recheck"
          onClick={onRecheck}
          title="Re-reads what is already loaded. It starts no model and no agent."
          type="button"
        >
          Check again
        </button>
      </div>

      {state === "unreadable" || updateFailed ? (
        <p
          className="flex items-start gap-1.5 rounded-md border border-destructive/40 bg-destructive/5 p-2 text-sm text-destructive"
          data-testid={
            updateFailed
              ? "pulse-declared-update-failed"
              : "pulse-declared-unreadable"
          }
          role="status"
        >
          <TriangleAlert className="mt-0.5 size-3 shrink-0" aria-hidden />
          <span>
            <span className="font-medium">
              {updateFailed
                ? "Declared work could not be updated."
                : "Declared work could not be read."}
            </span>{" "}
            {updateFailed
              ? "Showing previously read results; they may be out of date. "
              : null}
            {message ?? "The read failed without a message."}
          </span>
        </p>
      ) : null}

      {state === "loading" ? (
        <p
          className="flex items-center gap-1.5 text-sm text-muted-foreground"
          data-testid="pulse-declared-loading"
        >
          <Loader2 className="size-3 animate-spin" aria-hidden />
          Reading declared work. Nothing below is an answer yet.
        </p>
      ) : null}

      {model ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid="pulse-declared-scan"
        >
          {updateFailed ? "Previous read: " : null}
          {model.scan.sentence}
        </p>
      ) : null}

      {filterNote ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="pulse-declared-filter"
        >
          {filterNote}
        </p>
      ) : null}

      {limitations.length > 0 ? (
        <ul
          className="list-disc pl-4 text-2xs text-muted-foreground"
          data-testid="pulse-declared-limitations"
        >
          {limitations.map((limitation) => (
            <li data-testid="pulse-declared-limitation" key={limitation}>
              {limitation}
            </li>
          ))}
        </ul>
      ) : null}

      {current.length > 0 ? (
        <ul className="flex flex-col gap-2">{current.map(renderRow)}</ul>
      ) : null}

      {current.length === 0 && state === "ready" ? (
        <p
          className="text-sm text-muted-foreground"
          data-testid="pulse-declared-empty"
        >
          {updateFailed
            ? "The previous read found no unresolved declared work in this view; the latest read failed, so current work is unknown."
            : filterNote
              ? "No declared work matches this filter. Clear it to see the rest."
              : !readIsComplete
                ? "No declared work appeared in what this read returned; the read is incomplete, so that is not a project-wide answer."
                : settled.length > 0
                  ? `No unresolved declared work; ${settled.length} settled below.`
                  : "No declared work in this project's visible sessions."}
        </p>
      ) : null}

      {settled.length > 0 ? (
        <details data-testid="pulse-declared-settled">
          <summary className="cursor-pointer text-xs text-muted-foreground">
            Settled ({settled.length})
          </summary>
          <ul className="mt-2 flex flex-col gap-2">{settled.map(renderRow)}</ul>
        </details>
      ) : null}

      {hasNextPage ? (
        <div>
          <button
            className="rounded border border-border px-2 py-0.5 text-2xs text-foreground hover:bg-muted disabled:opacity-60"
            data-testid="pulse-declared-more"
            disabled={isFetchingNextPage}
            onClick={onLoadMore}
            type="button"
          >
            {isFetchingNextPage
              ? "Reading older sessions…"
              : "Show older sessions"}
          </button>
        </div>
      ) : null}
    </section>
  );
}
