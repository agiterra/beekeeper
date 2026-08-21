import type { ReactNode } from "react";
import {
  ArrowRightLeft,
  Flag,
  GitBranch,
  Info,
  ListTodo,
  OctagonAlert,
  StickyNote,
} from "lucide-react";

import { cn } from "@/shared/lib/cn";

import {
  honoredSupersessionAuthors,
  pulseAuthorLabel,
  type PulseAuthorNames,
} from "../lib/pulseAuthors";
import {
  branchChipLabel,
  formatPulseAge,
  formatPulseEntryType,
  pulseEntryReference,
} from "../lib/pulseFormat";
import type { PulseEntryType } from "../lib/pulseEntry.ts";
import type { PulseDigestEntry, PulseDigestSession } from "../lib/pulseFold.ts";

/**
 * Type treatment. Salience tracks consequence: a blocker can stop somebody
 * else's work, so it carries a warning colour and a bordered card; a handoff
 * asks for a person, so it carries the accent; everything else recedes.
 */
const ENTRY_TREATMENT: Record<
  PulseEntryType,
  {
    badge: string;
    card: string;
    Icon: typeof OctagonAlert;
  }
> = {
  blocker: {
    badge: "bg-destructive/15 text-destructive",
    card: "border-destructive/50 bg-destructive/5",
    Icon: OctagonAlert,
  },
  handoff: {
    badge: "bg-primary/15 text-primary",
    card: "border-primary/40 bg-primary/5",
    Icon: ArrowRightLeft,
  },
  plan: { badge: "bg-muted text-foreground", card: "", Icon: ListTodo },
  milestone: { badge: "bg-muted text-foreground", card: "", Icon: Flag },
  note: { badge: "bg-muted text-foreground", card: "", Icon: StickyNote },
};

/** One qualifier sentence — the lines that keep this screen honest. */
function QualifierLine({
  children,
  testId,
  title,
  tone = "muted",
}: {
  children: ReactNode;
  testId: string;
  title?: string;
  tone?: "muted" | "warning";
}) {
  return (
    <p
      className={cn(
        "mt-2 flex items-start gap-1.5 text-xs",
        tone === "warning"
          ? "text-amber-700 dark:text-amber-400"
          : "text-muted-foreground",
      )}
      data-testid={testId}
      title={title}
    >
      <Info className="mt-0.5 size-3 shrink-0" aria-hidden />
      <span>{children}</span>
    </p>
  );
}

/**
 * One explicit claim (kind 44240) as its author wrote it.
 *
 * Everything on this row is a **claim**, never an observation: the code areas
 * are what the author said they would touch, not what a provider saw. Observed
 * worktree facts live on `PulseSessionCard` and are never mixed in here.
 *
 * Nothing here prints a hash where a name exists. Authors resolve through
 * `authorNames`, an entry a claim names is described by its own text, and a
 * `pu-session` reference resolves to the session's name — the raw hex survives
 * only in a `title`, for the reader who wants to copy it.
 *
 * A `pu-session` reference renders at project level and never inside that
 * session's card. The tag is author-controlled and unverified at ingest, and
 * Slice 1 does not resolve the 44226 founder / 44228 authority chain that would
 * license placing it inside the card — so any project writer could otherwise
 * have their claim render on another team's session.
 */
export function PulseEntryRow({
  entry,
  nowSeconds,
  /** Resolved display names by lowercase pubkey; falls back to a short hash. */
  authorNames,
  /** Every entry in this digest, so a claim can be described by its content. */
  entriesById,
  /** Sessions by `sessionRef`, so `pu-session` resolves to a name. */
  sessionsByRef,
  /** Entries whose cross-author claim names this one — the other half of #2. */
  claimants = [],
}: {
  entry: PulseDigestEntry;
  nowSeconds: number;
  authorNames?: PulseAuthorNames;
  entriesById?: ReadonlyMap<string, PulseDigestEntry>;
  sessionsByRef?: ReadonlyMap<string, PulseDigestSession>;
  claimants?: readonly PulseDigestEntry[];
}) {
  const unhonored = entry.supersededBy.filter((claim) => !claim.honored);
  // Same predicate as `honoredSupersessionAuthors`, so the label at index `i`
  // belongs to the claim at index `i`.
  const honored = entry.supersededBy.filter(
    (claim) => claim.honored && claim.pubkey !== null,
  );
  const retiredBy = honoredSupersessionAuthors(entry, authorNames);
  const treatment = ENTRY_TREATMENT[entry.type];
  const TypeIcon = treatment.Icon;
  const author = pulseAuthorLabel(entry.pubkey, authorNames);
  const session = entry.sessionRef
    ? (sessionsByRef?.get(entry.sessionRef) ?? null)
    : null;

  return (
    <li
      className={cn(
        "rounded-md border border-border/60 bg-background/40 p-3",
        entry.active && treatment.card,
        // A retired entry is labelled, not just faded: the `Superseded` badge
        // and the "Replaced by" line below carry the fact, so the dimming is
        // decoration rather than the only signal (it survives dark mode).
        !entry.active && "opacity-75",
      )}
      data-testid="pulse-entry-row"
      data-entry-active={entry.active ? "true" : "false"}
      data-entry-type={entry.type}
    >
      <div className="flex flex-wrap items-center gap-2 text-2xs text-muted-foreground">
        <span
          className={cn(
            "inline-flex items-center gap-1 rounded px-1.5 py-0.5 font-medium",
            treatment.badge,
          )}
          data-testid="pulse-entry-type"
        >
          <TypeIcon className="size-3" aria-hidden />
          {formatPulseEntryType(entry.type)}
        </span>
        {entry.active ? null : (
          <span
            className="rounded bg-muted px-1.5 py-0.5 font-medium text-muted-foreground"
            data-testid="pulse-entry-superseded-badge"
          >
            Superseded
          </span>
        )}
        <span className="font-medium text-foreground" title={entry.pubkey}>
          {author}
        </span>
        <span>{formatPulseAge(nowSeconds - entry.createdAt)} ago</span>
        {entry.sessionRef ? (
          <span
            data-testid="pulse-entry-session-reference"
            title={`Session reference: ${entry.sessionRef}`}
          >
            {session?.name
              ? `references session “${session.name}”`
              : session
                ? "references an unnamed session in this project"
                : "references a session outside this project"}
          </span>
        ) : null}
      </div>

      <p className="mt-1.5 whitespace-pre-wrap text-sm text-foreground">
        {entry.text}
      </p>

      <div className="mt-2 flex flex-wrap items-center gap-1.5 text-2xs">
        <span
          className="inline-flex items-center gap-1 rounded bg-muted px-1.5 py-0.5 text-muted-foreground"
          data-testid="pulse-entry-branch"
        >
          <GitBranch className="size-3" aria-hidden />
          {branchChipLabel(entry.branch)}
        </span>
        {entry.claimedAreas.length > 0 ? (
          <span className="text-muted-foreground">Claimed areas:</span>
        ) : null}
        {entry.claimedAreas.map((area) => (
          <span
            className="rounded bg-muted px-1.5 py-0.5 font-mono text-muted-foreground"
            data-testid="pulse-entry-claimed-area"
            key={area}
            title={`${author} claims this area; nothing here was observed.`}
          >
            {area}
          </span>
        ))}
      </div>

      {/* This entry was retired by its own author's later entry — say who and
          when, rather than leaving a dimmed card to carry the whole fact. */}
      {honored.map((claim, index) => {
        const replacement = entriesById?.get(claim.eventId) ?? null;
        const by = retiredBy[index] ?? author;
        return (
          <QualifierLine
            key={claim.eventId}
            testId="pulse-entry-replaced-by"
            title={`Replaced by event ${claim.eventId}`}
          >
            Replaced by {by}
            {replacement
              ? `, ${formatPulseAge(nowSeconds - replacement.createdAt)} ago`
              : ""}
            {replacement
              ? `: ${pulseEntryReference(replacement, nowSeconds)}`
              : "."}
          </QualifierLine>
        );
      })}

      {/* The target's half of a refused cross-author claim. Both halves say the
          same thing in the same words — one relationship, one vocabulary. */}
      {claimants.map((claimant) => (
        <QualifierLine
          key={claimant.eventId}
          testId="pulse-entry-supersession-claimed"
          title={`Claimed by event ${claimant.eventId}`}
          tone="warning"
        >
          {pulseAuthorLabel(claimant.pubkey, authorNames)} says this is resolved
          — not applied: only an entry's own author can retire it, so this one
          stays active.
        </QualifierLine>
      ))}

      {/* The claimant's half. Never the word "supersession": it is a term of
          art from the plan, not a word a reader of this screen would use. */}
      {unhonored.map((claim) => {
        const target = entriesById?.get(claim.eventId) ?? null;
        const targetAuthor = claim.pubkey
          ? pulseAuthorLabel(claim.pubkey, authorNames)
          : null;
        const named = target
          ? pulseEntryReference(target, nowSeconds)
          : targetAuthor
            ? `${targetAuthor}'s entry`
            : "an entry that is not visible here";
        return (
          <QualifierLine
            key={claim.eventId}
            testId="pulse-entry-unhonored-claim"
            title={`Names event ${claim.eventId}`}
            tone="warning"
          >
            {claim.reason === "unresolved" ? (
              <>
                Says an entry that is not visible in this read is resolved —
                nothing was replaced.
              </>
            ) : claim.reason === "cross-author" ? (
              <>
                Says {named}
                {targetAuthor ? `, by ${targetAuthor},` : ""} is resolved — not
                applied: only an entry's own author can retire it, so the
                original stays active.
              </>
            ) : (
              <>
                Says {named} is resolved — not applied: this entry is older than
                the one it names, so the original stays active.
              </>
            )}
          </QualifierLine>
        );
      })}
    </li>
  );
}
