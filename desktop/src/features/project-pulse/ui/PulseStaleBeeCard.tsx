import { TriangleAlert } from "lucide-react";

import {
  buildPulseStaleBeeReading,
  type PulseStaleBeeReading,
} from "@/features/coding-sessions/lib/codingSessionSeatBee";

import type { PulseDigestSession } from "../lib/pulseFoldTypes";

/**
 * Pulse's "what is owed" row for seats running an older `bee`.
 *
 * Every number here was decided by the Desktop host against the local
 * checkout: this component compares nothing and infers nothing. A seat only
 * appears once the host said, in commits, how far behind `main` its build is.
 * Everything else — no `beeStamp` on the seat's 44223, an unparsed
 * `--version`, an ancestry the host could not decide — is disclosed as a count
 * of what could not be compared, because a build nobody could place is not
 * evidence that the build is current.
 *
 * The card renders nothing when the reading holds neither rows nor uncompared
 * seats: no observation is not the same claim as "every seat is up to date".
 */
export function PulseStaleBeeCard({
  reading,
}: {
  /** The folded reading; see `buildPulseStaleBeeReading`. */
  reading: PulseStaleBeeReading;
}) {
  const { rows, uncomparedCount, truncatedCount } = reading;
  if (rows.length === 0 && uncomparedCount === 0) return null;
  return (
    <div
      className="flex items-start gap-2 rounded-lg border border-border bg-card p-4 text-sm text-muted-foreground"
      data-testid="pulse-stale-bee"
    >
      <span className="mt-0.5 shrink-0 text-muted-foreground" aria-hidden>
        <TriangleAlert className="size-4" />
      </span>
      <div className="min-w-0 flex-1">
        <h2 className="font-medium text-foreground">
          seats running an older bee
        </h2>
        {rows.length === 0 ? (
          <p className="mt-1" data-testid="pulse-stale-bee-empty">
            no seat's build could be compared
          </p>
        ) : (
          <ul className="mt-1 list-disc pl-4">
            {rows.map((row) => (
              <li data-testid="pulse-stale-bee-row" key={row.seatKey}>
                {`${row.label} · bee ${row.sha7} — ${row.behind} ${
                  row.behind === 1 ? "commit" : "commits"
                } behind main`}
              </li>
            ))}
          </ul>
        )}
        {rows.length > 0 && uncomparedCount > 0 ? (
          <p className="mt-1" data-testid="pulse-stale-bee-uncompared">
            {uncomparedCount === 1
              ? "1 more seat's build could not be compared"
              : `${uncomparedCount} more seats' builds could not be compared`}
          </p>
        ) : null}
        {truncatedCount > 0 ? (
          <p className="mt-1" data-testid="pulse-stale-bee-truncated">
            {truncatedCount === 1
              ? "1 more row not shown"
              : `${truncatedCount} more rows not shown`}
          </p>
        ) : null}
      </div>
    </div>
  );
}

/**
 * The reading for the seats this digest can see — today, all of them uncompared.
 *
 * **What is missing, named exactly.** Two pieces have not landed:
 *
 * 1. `CoordinatedGeneration` (`shared/coordination/sessionCoordinationTypes.ts`)
 *    carries no `beeStamp` field, so the digest never conveys the seat's
 *    observed binary even though the seat's own kind:44223 now does. It is not
 *    the one-line addition it looks like: that type is the Project Pulse
 *    digest's own generation member, frozen byte-for-byte by
 *    `conformance/project-pulse-fold` and bound by the Rust fold
 *    (`crates/buzz-core/src/pulse_fold.rs`) and a relay-side kind 39011
 *    projection as well. Adding the key means both folds, the CONTRACT's key
 *    order, and the banked vectors — measured here: adding it to TypeScript
 *    alone turned `every banked conformance vector folds byte-identically`
 *    red.
 * 2. No Desktop host command answers ancestry — "how many commits behind
 *    `main` is this sha, in the local checkout" — so there is nothing to build
 *    an `ancestryBySha` map from.
 *
 * Until both exist, every live generation is honestly *uncompared*: the card
 * says `no seat's build could be compared` rather than implying every seat is
 * current. When they land, pass the real stamps and the host's ancestry map
 * here and the rows appear with no other change.
 */
export function pulseStaleBeeReadingFromSessions(
  sessions: readonly PulseDigestSession[],
): PulseStaleBeeReading {
  const seats = sessions
    .filter((session) => session.coordinationState === "provider_reachable")
    .flatMap((session) =>
      session.generations
        .filter((generation) => generation.current)
        .map((generation) => ({
          seatKey: generation.targetKey,
          label: `${session.name ?? session.sessionKey} · ${generation.executionKey}`,
          stamp: null,
        })),
    );
  return buildPulseStaleBeeReading(seats, new Map());
}
