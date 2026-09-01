import {
  CheckCircle2,
  CircleDashed,
  Clock3,
  Flag,
  LifeBuoy,
  OctagonAlert,
} from "lucide-react";

import {
  codingSessionSeatAuthorityCopy,
  codingSessionTeamWakeDeliveryCopy,
  type CodingSessionSeatAuthority,
  type CodingSessionTeamWakeDelivery,
  type CodingSessionTeamWakeDeliveryKind,
} from "@/features/coding-sessions/lib/codingSessionMissionContracts";
import { cn } from "@/shared/lib/cn";

const DELIVERY_ICON: Readonly<
  Record<CodingSessionTeamWakeDeliveryKind, typeof Clock3>
> = {
  "provider-queued": Clock3,
  "provider-started": CheckCircle2,
  "fallback-grace": Clock3,
  "fallback-unconfirmed": LifeBuoy,
  "fallback-queued": LifeBuoy,
  "fallback-started": LifeBuoy,
  failed: OctagonAlert,
  unknown: CircleDashed,
};

const DELIVERY_TONE: Readonly<
  Record<CodingSessionTeamWakeDeliveryKind, string>
> = {
  "provider-queued": "border-border/60 text-muted-foreground",
  "provider-started": "border-border/60 text-muted-foreground",
  "fallback-grace": "border-border/60 text-muted-foreground",
  "fallback-unconfirmed":
    "border-amber-500/45 text-amber-700 dark:text-amber-300",
  "fallback-queued": "border-amber-500/45 text-amber-700 dark:text-amber-300",
  "fallback-started": "border-amber-500/45 text-amber-700 dark:text-amber-300",
  failed: "border-destructive/40 text-destructive",
  unknown: "border-border/60 text-muted-foreground",
};

const BADGE_CLASS =
  "inline-flex max-w-full shrink-0 items-center gap-1 rounded-md border px-1.5 py-0.5 text-2xs";

/**
 * How one operation wake reached the lead — as a word, not a colour.
 *
 * The icon is decorative; the badge word from the frozen copy table is the
 * carrier, and `title` holds the full sentence (including the §2a residual
 * clause when the delivery carries it). `provider-started` has no short badge
 * word by design, so the badge falls back to the full sentence rather than
 * rendering a wordless glyph.
 */
export function CodingSessionMissionDeliveryBadge({
  className,
  delivery,
}: {
  className?: string;
  delivery: CodingSessionTeamWakeDelivery;
}) {
  const copy = codingSessionTeamWakeDeliveryCopy[delivery.kind];
  const Icon = DELIVERY_ICON[delivery.kind];
  return (
    <span
      className={cn(BADGE_CLASS, DELIVERY_TONE[delivery.kind], className)}
      data-kind={delivery.kind}
      data-testid="coding-session-delivery-badge"
      title={delivery.detail}
    >
      <Icon aria-hidden className="size-3 shrink-0" />
      <span className="truncate">{copy.badge ?? copy.detail}</span>
    </span>
  );
}

/**
 * Whether a seated execution holds the governed seat its create claims.
 *
 * A `granted` seat renders nothing — the absence of a badge is the honest
 * default, and a "granted" chip on every seat would drown the one that is not.
 * `unknown` says so out loud rather than reading as granted.
 */
export function CodingSessionSeatAuthorityBadge({
  authority,
  className,
}: {
  authority: CodingSessionSeatAuthority;
  className?: string;
}) {
  const copy = codingSessionSeatAuthorityCopy[authority.kind];
  if (copy.badge === null) return null;
  return (
    <span
      className={cn(
        BADGE_CLASS,
        authority.kind === "created-ungranted"
          ? "border-amber-500/45 text-amber-700 dark:text-amber-300"
          : "border-border/60 text-muted-foreground",
        className,
      )}
      data-kind={authority.kind}
      data-testid="coding-session-seat-authority-badge"
      title={
        authority.remedy
          ? `${authority.detail} — ${authority.remedy}`
          : authority.detail
      }
    >
      <Flag aria-hidden className="size-3 shrink-0" />
      <span className="truncate">{copy.badge}</span>
    </span>
  );
}

/**
 * A report the Rust fold included whose author holds no active seat for the
 * assignment's role. Disclosure on the row that carries it, not a rejection:
 * the fold accepted the report, and hiding the gap would restate the CLI's own
 * false claim that an unauthoritative report is excluded.
 *
 * The sentence states **the fold's** fact and nothing more. It used to borrow
 * `Seat created, not granted` from the seat-authority table, which is §1c
 * vocabulary for a create receipt with no matching grant — but §1d's
 * `unseatedReports` also covers a seat that was never created at all, and one
 * held for a *different* role. Neither has a create receipt behind it, so the
 * badge may not claim one.
 */
export function CodingSessionMissionUnseatedBadge({
  className,
  role,
}: {
  className?: string;
  /** The assignment's `assigneeRole`, when the caller knows it. */
  role?: string | null;
}) {
  return (
    <span
      className={cn(
        BADGE_CLASS,
        "border-amber-500/45 text-amber-700 dark:text-amber-300",
        className,
      )}
      data-testid="coding-session-unseated-badge"
      title={codingSessionUnseatedReportDetail(role)}
    >
      <Flag aria-hidden className="size-3 shrink-0" />
      <span className="truncate">unseated</span>
    </span>
  );
}

/** The one sentence for a fold-listed unseated report. */
export function codingSessionUnseatedReportDetail(
  role?: string | null,
): string {
  return role
    ? `Report author holds no seat for ${role}`
    : "Report author holds no seat for the assigned role";
}
