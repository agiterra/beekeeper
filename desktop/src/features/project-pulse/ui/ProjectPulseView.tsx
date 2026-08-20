import * as React from "react";
import { Activity, ChevronDown, ChevronRight } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { truncatePubkey } from "@/shared/lib/pubkey";

import {
  branchChipLabel,
  groupPulseEntries,
  groupPulseSessions,
  matchesBranchFilter,
} from "../lib/pulseFormat";
import {
  pulseDigestBranches,
  type ProjectPulseDigest,
  type PulseDigestEntry,
} from "../lib/pulseFold.ts";
import { PulseEntryRow } from "./PulseEntryRow";
import { PulseSessionCard } from "./PulseSessionCard";

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
 */
export type ProjectPulseViewState =
  | { kind: "loading" }
  | { kind: "unavailable" }
  | { kind: "ready"; digest: ProjectPulseDigest }
  | { kind: "partial"; digest: ProjectPulseDigest };

function StateCard({
  children,
  testId,
}: {
  children: React.ReactNode;
  testId: string;
}) {
  return (
    <div
      className="rounded-lg border border-border bg-card p-4 text-sm text-muted-foreground"
      data-testid={testId}
    >
      {children}
    </div>
  );
}

function GroupHeading({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="mb-2 text-sm font-medium text-foreground">{children}</h2>
  );
}

/** Cross-author claims, indexed by the entry they name. */
function crossAuthorClaimsByTarget(
  digest: ProjectPulseDigest,
): ReadonlyMap<string, string[]> {
  const claims = new Map<string, string[]>();
  for (const entry of digest.entries) {
    for (const claim of entry.supersededBy) {
      if (claim.honored || claim.reason !== "cross-author") continue;
      const existing = claims.get(claim.eventId) ?? [];
      existing.push(entry.pubkey);
      claims.set(claim.eventId, existing);
    }
  }
  return claims;
}

/**
 * The Pulse of one project, rendered from a folded digest and nothing else.
 *
 * Presentation only: every fact here was decided by `foldProjectPulseDigest`,
 * and this component's whole job is to keep the distinctions that fold made —
 * Active work vs Last seen, claim vs observation, unknown vs false — visible
 * instead of flattening them into something friendlier and wrong.
 */
export function ProjectPulseView({
  state,
  nowSeconds,
  onOpenSession,
}: {
  state: ProjectPulseViewState;
  nowSeconds: number;
  onOpenSession?: (targetKey: string) => void;
}) {
  const [branch, setBranch] = React.useState<string | null | undefined>(
    undefined,
  );
  const [showSuperseded, setShowSuperseded] = React.useState(false);

  const digest =
    state.kind === "ready" || state.kind === "partial" ? state.digest : null;
  const branches = React.useMemo(
    () => (digest ? pulseDigestBranches(digest) : []),
    [digest],
  );
  const claimedBy = React.useMemo(
    () => (digest ? crossAuthorClaimsByTarget(digest) : new Map()),
    [digest],
  );

  const header = (
    <header className="flex items-center gap-2">
      <Activity className="size-4 text-muted-foreground" aria-hidden />
      <h1 className="text-base font-semibold text-foreground">Pulse</h1>
      <p
        className="text-sm text-muted-foreground"
        data-testid="pulse-header-subtitle"
      >
        {PROJECT_PULSE_HEADER}
      </p>
    </header>
  );

  if (state.kind === "loading") {
    return (
      <div
        className="flex flex-col gap-4 p-4"
        data-testid="project-pulse-screen"
      >
        {header}
        <StateCard testId="pulse-loading">
          Reading this project's Pulse…
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
        <StateCard testId="pulse-unavailable">
          This project's head is not readable from this community, so its Pulse
          cannot be read either. This is not a claim that the project is empty.
        </StateCard>
      </div>
    );
  }

  const sessions = groupPulseSessions(digest as ProjectPulseDigest);
  const entries = groupPulseEntries(digest as ProjectPulseDigest);
  const visibleSessions = {
    activeWork: sessions.activeWork.filter((session) =>
      matchesBranchFilter(session.branch, branch),
    ),
    lastSeen: sessions.lastSeen.filter((session) =>
      matchesBranchFilter(session.branch, branch),
    ),
  };
  const visibleEntries = (rows: PulseDigestEntry[]) =>
    rows.filter((entry) => matchesBranchFilter(entry.branch, branch));
  // The disclosure count and the rows behind it read the same filtered list:
  // a count that outran the rows would promise history the branch chip hides.
  const visibleSupersededEntries = visibleEntries(entries.superseded);
  // A confirmed-empty verdict requires a read that lost nothing. An entry the
  // fold could not validate, or an event this client could not decode, is an
  // observation the surface does not have — saying "the project is quiet"
  // over it would render a null as a completed negative.
  const isConfirmedEmpty =
    state.kind === "ready" &&
    state.digest.entries.length === 0 &&
    state.digest.sessions.length === 0 &&
    state.digest.errors.length === 0;

  return (
    <div className="flex flex-col gap-4 p-4" data-testid="project-pulse-screen">
      {header}

      {state.kind === "partial" ? (
        <StateCard testId="pulse-partial">
          <span className="font-medium text-foreground">Partial read.</span>{" "}
          Some sources did not answer, so what follows is incomplete — not the
          whole project.
          <ul className="mt-1 list-disc pl-4">
            {state.digest.errors.map((error) => (
              <li key={`${error.scope}:${error.message}`}>
                {error.scope}: {error.message}
              </li>
            ))}
          </ul>
        </StateCard>
      ) : null}

      {isConfirmedEmpty ? (
        <StateCard testId="pulse-empty">
          No Pulse yet. Nobody has posted an entry for this project, and its
          channels carry no coding-session facts. This read completed — the
          project is quiet, not unreadable.
        </StateCard>
      ) : null}

      {/* A complete read can still have lost individual events (an entry that
          failed validation, an undecodable session fact). The partial card
          above already lists `errors[]`; this one carries them on an otherwise
          complete digest, so a non-empty `errors[]` can never sit silently
          behind a screen that looks exhaustive. */}
      {state.kind === "ready" && state.digest.errors.length > 0 ? (
        <StateCard testId="pulse-excluded">
          <span className="font-medium text-foreground">
            Some events were excluded.
          </span>{" "}
          This read completed, but what follows omits them.
          <ul className="mt-1 list-disc pl-4">
            {state.digest.errors.map((error) => (
              <li key={`${error.scope}:${error.message}`}>
                {error.scope}: {error.message}
              </li>
            ))}
          </ul>
        </StateCard>
      ) : null}

      {branches.length > 0 ? (
        <div
          className="flex flex-wrap items-center gap-1.5"
          data-testid="pulse-branch-chips"
        >
          <button
            className={cn(
              "rounded border border-border px-2 py-0.5 text-2xs",
              branch === undefined && "bg-muted text-foreground",
            )}
            onClick={() => setBranch(undefined)}
            type="button"
          >
            All branches
          </button>
          {branches.map((candidate) => (
            <button
              className={cn(
                "rounded border border-border px-2 py-0.5 text-2xs",
                branch === candidate && "bg-muted text-foreground",
              )}
              data-testid="pulse-branch-chip"
              key={
                candidate === null ? "no-branch-group" : `branch:${candidate}`
              }
              onClick={() => setBranch(candidate)}
              type="button"
            >
              {branchChipLabel(candidate)}
            </button>
          ))}
        </div>
      ) : null}

      {visibleSessions.activeWork.length > 0 ? (
        <section data-testid="pulse-active-work">
          <GroupHeading>Active work</GroupHeading>
          <ul className="flex flex-col gap-2">
            {visibleSessions.activeWork.map((session) => (
              <PulseSessionCard
                key={session.targetKey}
                nowSeconds={nowSeconds}
                onOpen={
                  onOpenSession
                    ? () => onOpenSession(session.targetKey)
                    : undefined
                }
                session={session}
              />
            ))}
          </ul>
        </section>
      ) : null}

      {visibleSessions.lastSeen.length > 0 ? (
        <section data-testid="pulse-last-seen">
          <GroupHeading>Last seen</GroupHeading>
          <ul className="flex flex-col gap-2">
            {visibleSessions.lastSeen.map((session) => (
              <PulseSessionCard
                key={session.targetKey}
                nowSeconds={nowSeconds}
                onOpen={
                  onOpenSession
                    ? () => onOpenSession(session.targetKey)
                    : undefined
                }
                session={session}
              />
            ))}
          </ul>
        </section>
      ) : null}

      {(digest as ProjectPulseDigest).sessions.length > 0 ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="pulse-sessions-scope"
        >
          Sessions are read from this project's channels only. A session running
          in a channel outside the project is not listed here.
        </p>
      ) : null}

      {visibleEntries(entries.active).length > 0 ? (
        <section data-testid="pulse-entries">
          <GroupHeading>Entries</GroupHeading>
          <ul className="flex flex-col gap-2">
            {visibleEntries(entries.active).map((entry) => (
              <PulseEntryRow
                entry={entry}
                key={entry.eventId}
                nowSeconds={nowSeconds}
                supersessionClaimedBy={claimedBy.get(entry.eventId) ?? []}
              />
            ))}
          </ul>
        </section>
      ) : null}

      {visibleSupersededEntries.length > 0 ? (
        <section data-testid="pulse-superseded">
          <button
            className="flex items-center gap-1 text-2xs text-muted-foreground hover:text-foreground"
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
              {visibleSupersededEntries.map((entry) => (
                <PulseEntryRow
                  entry={entry}
                  key={entry.eventId}
                  nowSeconds={nowSeconds}
                  supersessionClaimedBy={claimedBy.get(entry.eventId) ?? []}
                />
              ))}
            </ul>
          ) : null}
        </section>
      ) : null}
    </div>
  );
}

/** Authors of the honored claims that retired one entry, for a caller that
 * wants to name them without re-walking the digest. */
export function honoredSupersessionAuthors(entry: PulseDigestEntry): string[] {
  return entry.supersededBy
    .filter((claim) => claim.honored && claim.pubkey !== null)
    .map((claim) => truncatePubkey(claim.pubkey as string));
}
