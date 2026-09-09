import * as React from "react";
import {
  Activity,
  ChevronDown,
  ChevronRight,
  CircleCheck,
  EyeOff,
  Loader2,
  TriangleAlert,
} from "lucide-react";

import { cn } from "@/shared/lib/cn";

import type { PulseAuthorNames } from "../lib/pulseAuthors";
import { summarizePulseErrors } from "../lib/pulseErrorCopy";
import {
  branchChipLabel,
  countPulseBranchRows,
  formatPulseEntryCount,
  formatPulseReadAge,
  formatPulseSessionCount,
  groupPulseEntries,
  groupPulseSessions,
  matchesBranchFilter,
  pulseSessionDisplayGeneration,
  pulseSessionGroupHeading,
  pulseSessionMatchesBranch,
  sortPulseEntriesByConsequence,
} from "../lib/pulseFormat";
import {
  pulseDigestBranches,
  type ProjectPulseDigest,
  type PulseDigestEntry,
  type PulseDigestError,
  type PulseDigestSession,
} from "../lib/pulseFold.ts";
import type { PulseDeclaredWorkModel } from "../lib/pulseDeclaredWork";
import type { PulseDiskRow as PulseDiskRowModel } from "../lib/pulseDiskRow";
import type { PulseMissionRowsState } from "../lib/pulseQueries";
import { pulseMissionRowForSession } from "../lib/pulseMissionWire";
import { PulseDiskRow } from "./PulseDiskRow";
import {
  PulseDeclaredWorkSection,
  type PulseDeclaredWorkSectionState,
} from "./PulseDeclaredWorkSection";
import { PulseEntryRow } from "./PulseEntryRow";
import { PulseMissionsSection } from "./PulseMissionRow";
import { PulseOverlapCard } from "./PulseOverlapCard";
import { PulseRulingsWaitingCard } from "./PulseRulingsWaitingCard";
import { PulseSessionCard } from "./PulseSessionCard";
import {
  PulseStaleBeeCard,
  pulseStaleBeeReadingFromSessions,
} from "./PulseStaleBeeCard";
import { PulseWriteHint } from "./PulseWriteHint";

/** The Pulse header sentence. Fixed: it is what this screen actually shows. */
export const PROJECT_PULSE_HEADER =
  "Explicit updates and observed session state.";

/**
 * What the screen knows. `loading`, confirmed-empty, and `unavailable` are
 * three different answers and never share a card.
 *
 * There is no access-denied state here by design: an inadmissible Pulse read
 * returns an empty 200, so a 403 is not something this surface can observe.
 * `unavailable` is derived from the readability of the project head itself —
 * no head, no project to have a Pulse — never from a Pulse-query status code.
 *
 * `refreshing` marks a digest that is the *last complete read* rather than the
 * current one: a cached digest painted under a screen that otherwise means
 * "this is the current, complete answer" is a stale answer wearing a fresh
 * one's clothes.
 */
export type ProjectPulseViewState =
  | { kind: "loading" }
  | { kind: "unavailable" }
  | { kind: "ready"; digest: ProjectPulseDigest; refreshing?: boolean }
  | { kind: "partial"; digest: ProjectPulseDigest; refreshing?: boolean };

/**
 * The five honesty states, told apart before they are read.
 *
 * Each carries a glyph, a border tone, and a two-or-three word verdict ahead of
 * the explanatory sentence, because "which of five worlds am I in" should not
 * cost the reader a paragraph.
 */
function StateCard({
  children,
  icon,
  testId,
  tone = "neutral",
  verdict,
}: {
  children: React.ReactNode;
  icon: React.ReactNode;
  testId: string;
  tone?: "neutral" | "warning" | "muted";
  verdict: string;
}) {
  return (
    <div
      className={cn(
        "flex items-start gap-2 rounded-lg border p-4 text-sm text-muted-foreground",
        tone === "warning" && "border-amber-500/40 bg-amber-500/5",
        tone === "muted" && "border-border/60 bg-muted/30",
        tone === "neutral" && "border-border bg-card",
      )}
      data-testid={testId}
    >
      <span
        className={cn(
          "mt-0.5 shrink-0",
          tone === "warning"
            ? "text-amber-700 dark:text-amber-400"
            : "text-muted-foreground",
        )}
        aria-hidden
      >
        {icon}
      </span>
      <div className="min-w-0 flex-1">
        <span
          className={cn(
            "font-medium",
            tone === "warning"
              ? "text-amber-700 dark:text-amber-400"
              : "text-foreground",
          )}
        >
          {verdict}
        </span>{" "}
        {children}
      </div>
    </div>
  );
}

function GroupHeading({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="mb-2 text-sm font-medium text-foreground">{children}</h2>
  );
}

/**
 * What a read lost, as sentences.
 *
 * The digest's `errors[]` are wire records — `{scope, message}` with raw
 * 64-hex ids, pinned by the fold conformance corpus and shared with the CLI.
 * They are translated here rather than printed, so the one card on this screen
 * whose whole job is to tell a reader what they are missing does not do it in
 * a vocabulary only the fold speaks. The verbatim record stays in the `title`.
 */
function PulseErrorNotes({
  errors,
  entriesById,
  authorNames,
  nowSeconds,
}: {
  errors: readonly PulseDigestError[];
  entriesById: ReadonlyMap<string, PulseDigestEntry>;
  authorNames?: PulseAuthorNames;
  nowSeconds: number;
}) {
  const notes = summarizePulseErrors(errors, {
    entriesById,
    authorNames,
    nowSeconds,
  });
  return (
    <ul className="mt-1 list-disc pl-4" data-testid="pulse-error-notes">
      {notes.map((note) => (
        <li data-testid="pulse-error-note" key={note.key} title={note.title}>
          {note.sentence}
        </li>
      ))}
    </ul>
  );
}

/** Cross-author claims, indexed by the entry they name. */
function crossAuthorClaimsByTarget(
  digest: ProjectPulseDigest,
): ReadonlyMap<string, PulseDigestEntry[]> {
  const claims = new Map<string, PulseDigestEntry[]>();
  for (const entry of digest.entries) {
    for (const claim of entry.supersededBy) {
      if (claim.honored || claim.reason !== "cross-author") continue;
      const existing = claims.get(claim.eventId) ?? [];
      existing.push(entry);
      claims.set(claim.eventId, existing);
    }
  }
  return claims;
}

/**
 * The declared-work read, when the host performed one.
 *
 * Exactly what `PulseDeclaredWorkSection` renders, minus the props this view
 * already owns (the clock, the author map, the entry and session indexes).
 * Optional for the same reason `missions` is: this component renders models,
 * it does not fetch them, and absent this one the screen is byte-for-byte the
 * Pulse it was before declared work existed.
 */
export type ProjectPulseDeclaredWork = {
  model: PulseDeclaredWorkModel | null;
  state: PulseDeclaredWorkSectionState;
  message: string | null;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  onLoadMore: () => void;
  onRecheck: () => void;
  refreshing: boolean;
};

/**
 * The default answer to "can this session be opened": no.
 *
 * Module-level so the reference is stable across renders — a fresh arrow here
 * would defeat the memoisation of every row below it.
 */
const NO_SESSION_OPENABLE = () => false;

/**
 * The Pulse of one project, rendered from a folded digest and nothing else.
 *
 * Presentation only: every fact here was decided by `foldProjectPulseDigest`,
 * and this component's whole job is to keep the distinctions that fold made —
 * recency vs unverified liveness vs closure, claim vs observation, and unknown
 * vs false — visible instead of flattening them into something friendlier and
 * wrong.
 */
export function ProjectPulseView({
  state,
  nowSeconds,
  onOpenSession,
  /** The project this Pulse describes; without it the screen names no project. */
  projectName,
  /** Back to the project home, when the caller can navigate there. */
  /** Resolved author names by lowercase pubkey. */
  authorNames,
  /**
   * The sibling mission read, when the host fetched one.
   *
   * Optional so this component keeps taking a digest and nothing else: it
   * renders, it does not fetch. Absent, the screen is exactly the Pulse it was
   * before missions existed.
   */
  missions,
  /**
   * This project's disk row, already composed by `pulseDiskRow.ts` from the
   * host's own worktree records (L11). Optional for the same reason
   * `missions` is: this component renders a model, it does not fetch one —
   * absent, the screen is exactly the Pulse it was before the row existed.
   */
  diskRow,
  /**
   * The sibling declared-work read. When present, active `plan` entries are
   * *regrouped* into its section and leave the Entries list below — one
   * declaration, rendered once. Absent, nothing on this screen moves.
   */
  declaredWork,
  /** Opens one declared-work session's newest execution. Publishes nothing. */
  onOpenDeclaredSession,
  /** Whether an execution is recorded for a session key at all. */
  declaredSessionOpenable,
}: {
  state: ProjectPulseViewState;
  nowSeconds: number;
  onOpenSession?: (targetKey: string) => void;
  projectName?: string | null;
  authorNames?: PulseAuthorNames;
  missions?: PulseMissionRowsState;
  diskRow?: PulseDiskRowModel;
  declaredWork?: ProjectPulseDeclaredWork;
  onOpenDeclaredSession?: (sessionKey: string) => void;
  declaredSessionOpenable?: (sessionKey: string) => boolean;
}) {
  const [branch, setBranch] = React.useState<string | null | undefined>(
    undefined,
  );
  const [showSuperseded, setShowSuperseded] = React.useState(false);

  const digest =
    state.kind === "ready" || state.kind === "partial" ? state.digest : null;
  const refreshing =
    (state.kind === "ready" || state.kind === "partial") &&
    state.refreshing === true;
  const branches = React.useMemo(
    () => (digest ? pulseDigestBranches(digest) : []),
    [digest],
  );
  const claimedBy = React.useMemo(
    () =>
      digest
        ? crossAuthorClaimsByTarget(digest)
        : new Map<string, PulseDigestEntry[]>(),
    [digest],
  );
  const entriesById = React.useMemo(
    () =>
      new Map((digest?.entries ?? []).map((entry) => [entry.eventId, entry])),
    [digest],
  );
  const sessionsByRef = React.useMemo(() => {
    const byRef = new Map<string, PulseDigestSession>();
    for (const session of digest?.sessions ?? []) {
      if (session.sessionRef) byRef.set(session.sessionRef, session);
    }
    return byRef;
  }, [digest]);

  const header = (
    <header className="flex flex-col gap-1">
      <div className="flex flex-wrap items-center gap-2">
        <Activity className="size-4 text-muted-foreground" aria-hidden />
        <h1
          className="text-base font-semibold text-foreground"
          data-testid="pulse-header-title"
        >
          {projectName ? `Pulse · ${projectName}` : "Pulse"}
        </h1>
        <p
          className="text-sm text-muted-foreground"
          data-testid="pulse-header-subtitle"
        >
          {PROJECT_PULSE_HEADER}
        </p>
      </div>
      {digest ? (
        <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          <span
            data-testid="pulse-read-age"
            title={new Date(digest.asOf * 1_000).toISOString()}
          >
            {formatPulseReadAge(nowSeconds - digest.asOf)}
          </span>
          {refreshing ? (
            <span
              className="inline-flex items-center gap-1 rounded border border-border/60 bg-muted/50 px-1.5 py-0.5"
              data-testid="pulse-stale-read"
            >
              <Loader2 className="size-3 animate-spin" aria-hidden />
              showing last complete read
            </span>
          ) : null}
        </div>
      ) : null}
    </header>
  );

  if (state.kind === "loading") {
    return (
      <div
        className="flex flex-col gap-4 p-4"
        data-testid="project-pulse-screen"
      >
        {header}
        <StateCard
          icon={<Loader2 className="size-4 animate-spin" />}
          testId="pulse-loading"
          tone="muted"
          verdict="Reading…"
        >
          Reading this project's Pulse. Nothing below is an answer yet.
          <span
            className="mt-3 flex flex-col gap-2"
            data-testid="pulse-loading-skeleton"
          >
            <span className="h-3 w-2/3 animate-pulse rounded bg-muted" />
            <span className="h-3 w-1/2 animate-pulse rounded bg-muted" />
            <span className="h-3 w-3/5 animate-pulse rounded bg-muted" />
          </span>
        </StateCard>
      </div>
    );
  }

  if (state.kind === "unavailable") {
    return (
      <div
        className="flex flex-col gap-4 p-4"
        data-testid="project-pulse-screen"
      >
        {header}
        <StateCard
          icon={<EyeOff className="size-4" />}
          testId="pulse-unavailable"
          tone="muted"
          verdict="Not readable."
        >
          This project's head is not readable from this community, so its Pulse
          cannot be read either. This is not a claim that the project is empty.
        </StateCard>
      </div>
    );
  }

  const readable = digest as ProjectPulseDigest;
  const sessions = groupPulseSessions(readable);
  const entries = groupPulseEntries(readable);
  // Sessions match on any generation's branch, and the chip counts call the
  // same predicate — see `pulseSessionMatchesBranch`. Keeping one definition is
  // what stops a chip's count and its rows from telling two different stories.
  const sessionMatchesBranch = (session: PulseDigestSession) =>
    pulseSessionMatchesBranch(session, branch);
  const visibleSessions = {
    providerReachable: sessions.providerReachable.filter(sessionMatchesBranch),
    openUnverified: sessions.openUnverified.filter(sessionMatchesBranch),
    closed: sessions.closed.filter(sessionMatchesBranch),
  };
  const visibleEntries = (rows: PulseDigestEntry[]) =>
    rows.filter((entry) => matchesBranchFilter(entry.branch, branch));
  // The disclosure count and the rows behind it read the same filtered list:
  // a count that outran the rows would promise history the branch chip hides.
  const visibleSupersededEntries = visibleEntries(entries.superseded);
  // Blockers and handoffs lead: the strongest wait-signal on a coordination
  // screen must not be row three of three in the same grey.
  const visibleActiveEntries = sortPulseEntriesByConsequence(
    visibleEntries(entries.active),
  );
  // Regrouping, not filtering: an active plan is rendered by the declared-work
  // section instead of here, so the same declaration never appears twice. With
  // no declared-work read there is nowhere else for it to be, and the list is
  // exactly the list it has always been.
  const regroupedPlans = declaredWork
    ? visibleActiveEntries.filter((entry) => entry.type === "plan")
    : [];
  const listedActiveEntries = declaredWork
    ? visibleActiveEntries.filter((entry) => entry.type !== "plan")
    : visibleActiveEntries;
  // The branch filter follows the plans it used to hide. Without this a chip
  // that promises "3 rows on this filter" would sit above a fourth, on another
  // branch, in the section the plans moved to. Assignments are *not* filtered:
  // an assignment that reported no branch would vanish under every chip, which
  // would hide work rather than scope it — so the section says so instead.
  const declaredBranchFilter = declaredWork && branch !== undefined;
  const declaredModel = declaredBranchFilter
    ? filterDeclaredPlansByBranch(declaredWork?.model ?? null, branch)
    : (declaredWork?.model ?? null);
  const declaredFilterNote = declaredBranchFilter
    ? `Filtered to ${branchChipLabel(branch)}: plans only. Assignments are listed on every branch.`
    : null;
  const sessionsHidden =
    readable.sessions.length > 0 &&
    visibleSessions.providerReachable.length === 0 &&
    visibleSessions.openUnverified.length === 0 &&
    visibleSessions.closed.length === 0;
  const countsAreLowerBounds =
    state.kind === "partial" ||
    readable.complete === false ||
    readable.errors.length > 0;
  const asFloor = (count: number, formatted: string, noun: string): string =>
    count === 0
      ? `no ${noun} in what this read returned`
      : `at least ${formatted}`;
  const entriesHidden =
    readable.entries.some((entry) => entry.active) &&
    visibleActiveEntries.length === 0;
  const sessionCount =
    visibleSessions.providerReachable.length +
    visibleSessions.openUnverified.length +
    visibleSessions.closed.length;
  // A confirmed-empty verdict requires a read that lost nothing. An entry the
  // fold could not validate, or an event this client could not decode, is an
  // observation the surface does not have, so absence cannot be promoted into
  // a liveness conclusion.
  const isConfirmedEmpty =
    state.kind === "ready" &&
    !refreshing &&
    state.digest.entries.length === 0 &&
    state.digest.sessions.length === 0 &&
    state.digest.errors.length === 0;

  const entryRow = (entry: PulseDigestEntry) => (
    <PulseEntryRow
      authorNames={authorNames}
      claimants={claimedBy.get(entry.eventId) ?? []}
      entriesById={entriesById}
      entry={entry}
      key={entry.eventId}
      nowSeconds={nowSeconds}
      sessionsByRef={sessionsByRef}
    />
  );
  const missionRows = missions?.rows ?? null;
  // Every mission painted inside a session card, so the section below can show
  // the rest exactly once — a mission dropped between two views of the same
  // project is the failure this bookkeeping exists to prevent.
  const missionSessionKeys = new Set<string>();
  const sessionCard = (session: PulseDigestSession) => {
    const targetKey = pulseSessionDisplayGeneration(session)?.targetKey;
    const missionRow = pulseMissionRowForSession(
      missionRows,
      session.sessionKey,
    );
    if (missionRow) missionSessionKeys.add(missionRow.sessionKey);
    return (
      <PulseSessionCard
        key={session.sessionKey}
        missionRow={missionRow}
        nowSeconds={nowSeconds}
        onOpen={
          onOpenSession && targetKey
            ? () => onOpenSession(targetKey)
            : undefined
        }
        onOpenExecution={onOpenSession}
        session={session}
      />
    );
  };

  return (
    <div className="flex flex-col gap-4 p-4" data-testid="project-pulse-screen">
      {header}

      {state.kind === "partial" ? (
        <StateCard
          icon={<TriangleAlert className="size-4" />}
          testId="pulse-partial"
          tone="warning"
          verdict="Partial read."
        >
          Some sources did not answer, so what follows is incomplete — not the
          whole project.
          <PulseErrorNotes
            authorNames={authorNames}
            entriesById={entriesById}
            errors={state.digest.errors}
            nowSeconds={nowSeconds}
          />
        </StateCard>
      ) : null}

      {isConfirmedEmpty ? (
        <StateCard
          icon={<CircleCheck className="size-4" />}
          testId="pulse-empty"
          verdict="No Pulse observations."
        >
          No sessions are currently verified live. This read completed and found
          no Pulse entries or coding-session observations in the project's
          channels; current liveness remains unverified.
          <PulseWriteHint />
        </StateCard>
      ) : null}

      {/* A complete read can still have lost individual events (an entry that
          failed validation, an undecodable session fact). The partial card
          above already lists `errors[]`; this one carries them on an otherwise
          complete digest, so a non-empty `errors[]` can never sit silently
          behind a screen that looks exhaustive. */}
      {state.kind === "ready" && state.digest.errors.length > 0 ? (
        <StateCard
          icon={<TriangleAlert className="size-4" />}
          testId="pulse-excluded"
          tone="warning"
          verdict="Some events were excluded."
        >
          This read completed, but what follows omits them.
          <PulseErrorNotes
            authorNames={authorNames}
            entriesById={entriesById}
            errors={state.digest.errors}
            nowSeconds={nowSeconds}
          />
        </StateCard>
      ) : null}

      {missionRows ? <PulseRulingsWaitingCard rows={missionRows} /> : null}
      {/* Which `bee` each live seat is running. Not wired to the wire yet:
          `CoordinatedGeneration` (shared/coordination/sessionCoordinationTypes.ts)
          carries no `beeStamp` — adding it is a two-fold contract change, not a
          field, because that type is the Pulse digest's frozen generation
          member — and no host command answers ancestry against the local
          checkout. See `pulseStaleBeeReadingFromSessions`. Until both land
          every live seat reads as uncompared, never as current. */}
      <PulseStaleBeeCard
        reading={pulseStaleBeeReadingFromSessions(readable.sessions)}
      />

      {branches.length > 0 ? (
        <div
          className="flex flex-wrap items-center gap-2"
          data-testid="pulse-branch-chips"
        >
          <span
            className="text-xs text-muted-foreground"
            id="pulse-branch-label"
          >
            Branch
          </span>
          {/* A plain container, labelled by the visible word next to it: the
              buttons carry their own names and pressed state, so a redundant
              ARIA role only adds a second, weaker source of truth. */}
          <div className="inline-flex flex-wrap items-center gap-0.5 rounded-md border border-border bg-muted/40 p-0.5">
            <BranchChip
              count={countPulseBranchRows(readable, undefined)}
              label="All branches"
              onSelect={() => setBranch(undefined)}
              selected={branch === undefined}
              testId="pulse-branch-chip-all"
            />
            {branches.map((candidate) => (
              <BranchChip
                count={countPulseBranchRows(readable, candidate)}
                key={
                  candidate === null ? "no-branch-group" : `branch:${candidate}`
                }
                label={branchChipLabel(candidate)}
                onSelect={() => setBranch(candidate)}
                selected={branch === candidate}
                testId="pulse-branch-chip"
              />
            ))}
          </div>
        </div>
      ) : null}

      {isConfirmedEmpty ? null : (
        <div
          className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground"
          data-testid="pulse-counts"
        >
          {refreshing ? <span>Last completed read:</span> : null}
          <span data-testid="pulse-count-entries">
            {countsAreLowerBounds
              ? asFloor(
                  visibleActiveEntries.length,
                  formatPulseEntryCount(visibleActiveEntries.length),
                  "entries",
                )
              : formatPulseEntryCount(visibleActiveEntries.length)}
          </span>
          <span data-testid="pulse-count-sessions">
            {countsAreLowerBounds
              ? asFloor(
                  sessionCount,
                  formatPulseSessionCount(sessionCount),
                  "sessions",
                )
              : formatPulseSessionCount(sessionCount)}
            {visibleSessions.providerReachable.length > 0
              ? ` · ${visibleSessions.providerReachable.length} provider-reachable`
              : ""}
          </span>
          {countsAreLowerBounds ? (
            <span data-testid="pulse-counts-incomplete">
              — this read lost events, so these are floors, not totals
            </span>
          ) : null}
        </div>
      )}

      {/* Explicit coordination claims lead the page. Observed executions are
          still fully present below, but cannot bury what people chose to say. */}
      {isConfirmedEmpty ? null : (
        <section data-testid="pulse-entries">
          <GroupHeading>Entries</GroupHeading>
          {listedActiveEntries.length > 0 ? (
            <ul className="flex flex-col gap-2">
              {listedActiveEntries.map(entryRow)}
            </ul>
          ) : regroupedPlans.length > 0 ? (
            // Not an empty project: the plans moved one section down, and
            // saying "no entries" over a screen that is showing them would be
            // the same lie in the other direction.
            <p
              className="text-sm text-muted-foreground"
              data-testid="pulse-entries-regrouped"
            >
              {regroupedPlans.length === 1
                ? "The one active entry here is a plan; it is listed under Declared work below."
                : `The ${regroupedPlans.length} active entries here are plans; they are listed under Declared work below.`}
            </p>
          ) : (
            <div data-testid="pulse-entries-empty">
              <p className="text-sm text-muted-foreground">
                {entriesHidden
                  ? "No entries on this branch. Other branches have entries — clear the filter to see them."
                  : readable.errors.length > 0
                    ? "No entries appeared in what this read returned; the read lost events, so that is not a project-wide answer."
                    : refreshing
                      ? "The last completed read contained no entries; refreshing now…"
                      : "No entries posted for this project yet."}
              </p>
              {entriesHidden ? null : <PulseWriteHint />}
            </div>
          )}
        </section>
      )}

      {declaredWork ? (
        <PulseDeclaredWorkSection
          authorNames={authorNames}
          // The same refused-claim map the Entries list reads: a plan that
          // moved into this section keeps every qualifier it had above it.
          claimedBy={claimedBy}
          entriesById={entriesById}
          hasNextPage={declaredWork.hasNextPage}
          isFetchingNextPage={declaredWork.isFetchingNextPage}
          filterNote={declaredFilterNote}
          message={declaredWork.message}
          model={declaredModel}
          nowSeconds={nowSeconds}
          onLoadMore={declaredWork.onLoadMore}
          onOpenSession={onOpenDeclaredSession}
          onRecheck={declaredWork.onRecheck}
          refreshing={declaredWork.refreshing}
          sessionOpenable={declaredSessionOpenable ?? NO_SESSION_OPENABLE}
          sessionsByRef={sessionsByRef}
          state={declaredWork.state}
        />
      ) : null}

      {visibleSupersededEntries.length > 0 ? (
        <section data-testid="pulse-superseded">
          <button
            className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
            data-testid="pulse-superseded-toggle"
            onClick={() => setShowSuperseded((open) => !open)}
            type="button"
          >
            {showSuperseded ? (
              <ChevronDown className="size-3" aria-hidden />
            ) : (
              <ChevronRight className="size-3" aria-hidden />
            )}
            {visibleSupersededEntries.length} superseded{" "}
            {visibleSupersededEntries.length === 1 ? "entry" : "entries"}
          </button>
          {showSuperseded ? (
            <ul className="mt-2 flex flex-col gap-2">
              {visibleSupersededEntries.map(entryRow)}
            </ul>
          ) : null}
        </section>
      ) : null}

      {/* Durable session observations follow claims and remain split by the
          fold's independent lifecycle/reachability facts. */}
      <section data-testid="pulse-sessions">
        {visibleSessions.providerReachable.length > 0 ? (
          <div className="mb-3" data-testid="pulse-provider-reachable">
            <GroupHeading>
              {pulseSessionGroupHeading("provider_reachable")}
            </GroupHeading>
            <ul className="flex flex-col gap-2">
              {visibleSessions.providerReachable.map(sessionCard)}
            </ul>
          </div>
        ) : null}

        {visibleSessions.openUnverified.length > 0 ? (
          <div className="mb-3" data-testid="pulse-open-unverified">
            <GroupHeading>
              {pulseSessionGroupHeading("open_unverified")}
            </GroupHeading>
            <ul className="flex flex-col gap-2">
              {visibleSessions.openUnverified.map(sessionCard)}
            </ul>
          </div>
        ) : null}

        {visibleSessions.closed.length > 0 ? (
          <div className="mb-3" data-testid="pulse-closed">
            <GroupHeading>{pulseSessionGroupHeading("closed")}</GroupHeading>
            <ul className="flex flex-col gap-2">
              {visibleSessions.closed.map(sessionCard)}
            </ul>
          </div>
        ) : null}

        {sessionCount === 0 ? (
          <p
            className="text-sm text-muted-foreground"
            data-testid="pulse-sessions-empty"
          >
            {refreshing
              ? "The last completed read contained no provider-reachable sessions; refreshing now…"
              : sessionsHidden
                ? "No coding sessions on this branch. Other branches have sessions — clear the filter to see them."
                : readable.errors.length > 0
                  ? "No sessions are currently verified live — but this read lost events, so that is not a confirmed answer."
                  : "No sessions are currently verified live."}
          </p>
        ) : null}

        <p
          className="mt-1 text-xs text-muted-foreground"
          data-testid="pulse-sessions-scope"
        >
          Sessions are read from this project's channels only. A session running
          in a channel outside the project is not listed here.
        </p>
      </section>

      {missions ? (
        <PulseMissionsSection
          renderedSessionKeys={missionSessionKeys}
          rows={missionRows}
          unreadable={missions.kind === "unreadable" ? missions.message : null}
        />
      ) : null}

      {missionRows ? (
        <PulseOverlapCard overlaps={missionRows.overlaps} />
      ) : null}

      {diskRow ? <PulseDiskRow row={diskRow} /> : null}
    </div>
  );
}

/**
 * The same model with its plan rows scoped to one branch.
 *
 * Only plans: they are the rows the branch chip already counted before they
 * were regrouped into the declared-work section, and `entry.branch` is the
 * field it filtered on. Assignments keep their own branch on the row and are
 * left alone, because a filter cannot tell "on another branch" from "reported
 * no branch" and would silently drop the second.
 */
function filterDeclaredPlansByBranch(
  model: PulseDeclaredWorkModel | null,
  branch: string | null | undefined,
): PulseDeclaredWorkModel | null {
  if (!model) return null;
  const keep = (row: PulseDeclaredWorkModel["current"][number]) =>
    row.kind !== "plan" || matchesBranchFilter(row.entry.branch, branch);
  return {
    ...model,
    current: model.current.filter(keep),
    settled: model.settled.filter(keep),
  };
}

/** One segment of the branch filter: a control that looks like a control. */
function BranchChip({
  count,
  label,
  onSelect,
  selected,
  testId,
}: {
  count: number;
  label: string;
  onSelect: () => void;
  selected: boolean;
  testId: string;
}) {
  return (
    <button
      aria-pressed={selected}
      className={cn(
        "rounded px-2 py-0.5 text-2xs text-muted-foreground",
        selected && "bg-background font-medium text-foreground shadow-sm",
      )}
      data-testid={testId}
      onClick={onSelect}
      title={`${count} row${count === 1 ? "" : "s"} on this filter: sessions and active entries`}
      type="button"
    >
      {label}
      <span className="ml-1 tabular-nums text-muted-foreground">{count}</span>
    </button>
  );
}
