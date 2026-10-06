/**
 * One card grammar for every Mission row, in three weights.
 *
 * Before this module the Mission stream carried four competing card
 * vocabularies — the pinned transaction card's tinted `article`, the turn
 * block's left-accent rule, the conversation lane's `bg-muted/40` bubble and
 * the lifecycle line's centred caption — so a blocker and a "seat joined"
 * notice read at the same weight. Weight is now a deliberate, named choice:
 *
 * - `attention` — something a person must act on or is not allowed to miss.
 *   Full-width, bordered in a state hue, and always paired with an icon **and**
 *   a word by its caller; the colour is never the only carrier.
 * - `standard` — a signed fact worth its own card.
 * - `quiet` — chronology and scaffolding: lifecycle notices, truncation rows.
 *
 * Every class here is a Beekeeper theme token or a stock rem text token. There are
 * no hex colours and no arbitrary text sizes, so Cmd +/- zoom scales the whole
 * stream and both themes are covered by one definition.
 */
import { cn } from "@/shared/lib/cn";

/** How much of the reader's attention one Mission row is entitled to. */
export type CodingSessionMissionRowWeight = "attention" | "standard" | "quiet";

/**
 * Which state hue an `attention` row wears. `critical` is destructive — a fact
 * nobody is answering for; `caution` is amber — a fact somebody must answer.
 * Ignored for the other two weights, which never take a state hue.
 */
export type CodingSessionMissionRowTone = "critical" | "caution";

export type CodingSessionMissionRowOptions = {
  tone?: CodingSessionMissionRowTone;
  /** Extra classes merged last, so a caller can add layout without forking the grammar. */
  className?: string;
};

const ROW_BASE = "w-full min-w-0 rounded-xl";

/**
 * The single source of border, radius and padding for a Mission row.
 *
 * `attention` and `standard` are bordered cards; `quiet` is borderless and
 * muted. Callers add their own content classes through `opts.className`, which
 * is merged last so it wins on conflict.
 */
export function missionRowClass(
  weight: CodingSessionMissionRowWeight,
  opts: CodingSessionMissionRowOptions = {},
): string {
  if (weight === "quiet") {
    return cn(ROW_BASE, "px-3 py-1.5 text-muted-foreground", opts.className);
  }
  if (weight === "attention") {
    return cn(
      ROW_BASE,
      "border px-3 py-2.5",
      opts.tone === "critical"
        ? "border-destructive/40 bg-destructive/5"
        : "border-amber-500/45 bg-amber-500/5",
      opts.className,
    );
  }
  return cn(
    ROW_BASE,
    "border border-border/60 bg-background px-3 py-2.5",
    opts.className,
  );
}

/** The row title line: `text-sm`, the one step above body text. */
export function missionRowTitleClass(): string {
  return "min-w-0 text-sm font-semibold text-foreground";
}

/** The row body line: `text-xs`, the signed summary the title names. */
export function missionRowBodyClass(): string {
  return "min-w-0 text-xs text-foreground/85";
}

/**
 * A **stream** row's body: `text-sm`, one step above the rail's.
 *
 * The stream is read like chat — the design canvas (DESIGN-SPEC §3 Region C,
 * C-tx) puts the transaction row's signed summary at the same weight as a
 * message, so the causal spine reads at a glance rather than as fine print.
 * The rail stays on {@link missionRowBodyClass}: it is scanned, not read.
 */
export function missionRowChatBodyClass(): string {
  return "min-w-0 text-sm text-foreground/85";
}

/** Row metadata — time, counts, source disclosure — on the `text-2xs` step. */
export function missionRowMetaClass(): string {
  return "min-w-0 text-2xs text-muted-foreground";
}
