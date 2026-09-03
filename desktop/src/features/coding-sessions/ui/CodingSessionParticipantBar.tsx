import type { ReactNode } from "react";

import type {
  CodingSessionSeatAuthority,
  CodingSessionTeamWakeDelivery,
} from "@/features/coding-sessions/lib/codingSessionMissionContracts";
import type { CodingSessionParticipantPresence } from "@/features/coding-sessions/lib/codingSessionStreamPresence";
import { codingSessionParticipantAccent } from "@/features/coding-sessions/lib/codingSessionParticipantAccent";
import { codingSessionSeatBeeLine } from "@/features/coding-sessions/lib/codingSessionSeatBee";
import type { SeatBeeStamp } from "@/features/coding-sessions/lib/codingSessionSeatBee";
import { cn } from "@/shared/lib/cn";
import {
  CodingSessionMissionDeliveryBadge,
  CodingSessionSeatAuthorityBadge,
} from "./CodingSessionMissionDeliveryBadge";
import { CodingSessionSeatBeeLine } from "./CodingSessionSeatBeeLine";

/**
 * Singularity's always-readable roster: identity first, provider details second.
 *
 * The chip is **status only** — model, runtime, generation, seat authority
 * detail and last activity live in the Inspector's Team section, one home each.
 * Two exceptions earn their place here because they are the facts a reader
 * would otherwise never learn without opening a rail: a seat that was created
 * but never granted, and a wake for this seat's own report that has not started.
 *
 * A third joined them in L12: which `bee` this seat is actually running. Two
 * seats in one session can answer from two different binaries, and until the
 * host stamped it nothing on either surface said so.
 */
export function CodingSessionParticipantBar({
  className,
  deliveries,
  focusedExecutionKey,
  items,
  leading,
  onFocus,
  seatAuthorities,
  seatBeeStamps,
}: {
  /** Merged last — the finalizer owns the strip's border and background. */
  className?: string;
  /** Delivery evidence, matched to a chip through its seat's actor pubkey. */
  deliveries?: readonly CodingSessionTeamWakeDelivery[];
  focusedExecutionKey: string | null;
  items: readonly CodingSessionParticipantPresence[];
  /** Workflow controls that belong before the roster in the same strip. */
  leading?: ReactNode;
  onFocus: (executionKey: string | null) => void;
  /** Seat authority per execution; absent means the projection is not loaded. */
  seatAuthorities?: readonly CodingSessionSeatAuthority[];
  /**
   * The `bee` each seat is running, keyed by `executionKey`.
   *
   * A missing entry — and a `null` one — mean the seat's 44223 carried no
   * `beeStamp`, so the chip says nothing about its build rather than inventing
   * an unknown. An observed-but-unparsed `--version` arrives as a stamp with a
   * null `sha` and reads `bee build unknown`.
   */
  seatBeeStamps?: ReadonlyMap<string, SeatBeeStamp | null>;
}) {
  if (items.length === 0) return null;
  const authorityByExecution = new Map(
    (seatAuthorities ?? []).map((authority) => [
      authority.executionKey,
      authority,
    ]),
  );
  return (
    <nav
      aria-label="Session participants"
      className={cn(
        "flex min-h-14 shrink-0 items-center gap-2 overflow-x-auto px-4 py-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden",
        className,
      )}
      data-testid="coding-session-participant-bar"
    >
      {leading ? (
        <div className="mr-1 flex shrink-0 items-center border-r border-border/60 pr-3">
          {leading}
        </div>
      ) : null}
      {items.map((item) => {
        const selected = item.executionKey === focusedExecutionKey;
        const accent = codingSessionParticipantAccent(item.executionKey);
        const live = item.status.kind === "working";
        const attention =
          item.status.kind === "unknown" && item.status.attention !== undefined;
        const noProviderAnswering =
          item.status.kind === "unknown" &&
          item.status.label === "No provider answering";
        const authority = authorityByExecution.get(item.executionKey) ?? null;
        // `granted` is the uneventful case: the absence of a badge is the
        // honest default, and badging every seat would drown the one that
        // actually lacks its grant.
        const shownAuthority =
          authority !== null && authority.kind !== "granted" ? authority : null;
        const delivery = newestSeatDelivery(deliveries, authority);
        // A seat with no stamp on the wire adds no line, so its chip keeps the
        // exact height it had before this lane.
        const beeStamp = seatBeeStamps?.get(item.executionKey) ?? null;
        const hasBeeLine = codingSessionSeatBeeLine(beeStamp) !== null;
        return (
          <button
            aria-label={`${selected ? "Show all participants" : `Focus ${item.label}`} — ${item.disposition}`}
            aria-pressed={selected}
            className={cn(
              "group relative flex min-w-44 shrink-0 items-center gap-2.5 rounded-xl border px-3 py-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring motion-reduce:transition-none",
              selected
                ? cn(accent.border, accent.soft)
                : "border-border/60 bg-muted/15 hover:bg-muted/35",
              live && "coding-session-agent-breathe",
              noProviderAnswering && "border-destructive/45",
              attention && !noProviderAnswering && "border-amber-500/45",
            )}
            data-state={item.status.kind}
            data-testid="coding-session-participant-chip"
            key={item.executionKey}
            onClick={() => onFocus(selected ? null : item.executionKey)}
            title={
              item.secondaryLabel
                ? `${item.secondaryLabel} · ${item.lastTurnLabel}`
                : item.lastTurnLabel
            }
            type="button"
          >
            <span
              aria-hidden
              className={cn(
                "size-2.5 shrink-0 rounded-full",
                participantDot(item),
              )}
            />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-xs font-semibold text-foreground">
                {item.label}
              </span>
              <span
                className={cn(
                  "mt-0.5 block truncate text-2xs",
                  noProviderAnswering
                    ? "text-destructive"
                    : attention
                      ? "text-amber-700 dark:text-amber-300"
                      : "text-muted-foreground",
                )}
              >
                {item.disposition}
              </span>
              {/*
                The seat's activity phrase is deliberately absent. It has one
                home — `CodingSessionLiveActivityBar` — and rendering it here
                too made the chip a three-line card, pushed row 2 well past the
                wireframe's 56px, and clipped the phrase mid-word on narrow.
                The chip carries identity, the W1 word, and the two facts a
                reader could not otherwise learn (seat authority, delivery).
              */}
              {shownAuthority !== null || delivery !== null || hasBeeLine ? (
                <span className="mt-1 flex min-w-0 flex-wrap items-center gap-1">
                  {shownAuthority !== null ? (
                    <CodingSessionSeatAuthorityBadge
                      authority={shownAuthority}
                    />
                  ) : null}
                  {delivery !== null ? (
                    <CodingSessionMissionDeliveryBadge delivery={delivery} />
                  ) : null}
                  <CodingSessionSeatBeeLine stamp={beeStamp} />
                </span>
              ) : null}
            </span>
          </button>
        );
      })}
    </nav>
  );
}

/**
 * The newest delivery this seat authored whose wake has not started.
 *
 * A started provider wake is the uneventful case and earns no chip; anything
 * else — queued, waiting, covered by Desktop, failed, unknown — is a fact the
 * roster owes the reader. Matching runs through the seat's actor pubkey, so a
 * chip with no seat-authority projection shows no delivery rather than
 * guessing which seat a delivery belongs to.
 */
function newestSeatDelivery(
  deliveries: readonly CodingSessionTeamWakeDelivery[] | undefined,
  authority: CodingSessionSeatAuthority | null,
): CodingSessionTeamWakeDelivery | null {
  const actor = authority?.actorPubkey ?? null;
  if (!deliveries || actor === null) return null;
  let newest: CodingSessionTeamWakeDelivery | null = null;
  for (const delivery of deliveries) {
    if (delivery.kind === "provider-started") continue;
    if (delivery.sourceActorPubkey?.toLowerCase() !== actor.toLowerCase()) {
      continue;
    }
    if (
      newest === null ||
      (delivery.observedAtMs ?? 0) >= (newest.observedAtMs ?? 0)
    ) {
      newest = delivery;
    }
  }
  return newest;
}

function participantDot(item: CodingSessionParticipantPresence): string {
  if (item.status.kind === "working") return "bg-emerald-500";
  if (item.status.kind === "waiting") return "bg-amber-500";
  if (
    item.status.kind === "unknown" &&
    item.status.label === "No provider answering"
  ) {
    return "bg-destructive";
  }
  if (item.status.kind === "unknown" && item.status.attention) {
    return "bg-amber-500";
  }
  if (item.status.kind === "ended") return "bg-muted-foreground/30";
  return "bg-muted-foreground/50";
}
