import { GitBranch } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { truncatePubkey } from "@/shared/lib/pubkey";

import {
  branchChipLabel,
  formatPulseAge,
  formatPulseEntryType,
} from "../lib/pulseFormat";
import type { PulseDigestEntry } from "../lib/pulseFold.ts";

/**
 * One explicit claim (kind 44240) as its author wrote it.
 *
 * Everything on this row is a **claim**, never an observation: the code areas
 * are what the author said they would touch, not what a provider saw. Observed
 * worktree facts live on `PulseSessionCard` and are never mixed in here.
 *
 * A `pu-session` reference renders at project level as
 * `references session <ref>` and never inside that session's card. The tag is
 * author-controlled and unverified at ingest, and Slice 1 does not resolve the
 * 44226 founder / 44228 authority chain that would license placing it inside
 * the card — so any project writer could otherwise have their claim render on
 * another team's session.
 */
export function PulseEntryRow({
  entry,
  nowSeconds,
  /** Authors whose cross-author supersession claim names this entry. */
  supersessionClaimedBy = [],
}: {
  entry: PulseDigestEntry;
  nowSeconds: number;
  supersessionClaimedBy?: readonly string[];
}) {
  const unhonored = entry.supersededBy.filter((claim) => !claim.honored);
  return (
    <li
      className={cn(
        "rounded-md border border-border/60 bg-background/40 p-3",
        !entry.active && "opacity-75",
      )}
      data-testid="pulse-entry-row"
      data-entry-active={entry.active ? "true" : "false"}
      data-entry-type={entry.type}
    >
      <div className="flex flex-wrap items-center gap-2 text-2xs text-muted-foreground">
        <span className="rounded bg-muted px-1.5 py-0.5 font-medium text-foreground">
          {formatPulseEntryType(entry.type)}
        </span>
        <span className="font-mono">{truncatePubkey(entry.pubkey)}</span>
        <span>{formatPulseAge(nowSeconds - entry.createdAt)} ago</span>
        {entry.sessionRef ? (
          <span data-testid="pulse-entry-session-reference">
            references session {entry.sessionRef}
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
            title={`${truncatePubkey(entry.pubkey)} claims this area; nothing here was observed.`}
          >
            {area}
          </span>
        ))}
      </div>

      {supersessionClaimedBy.length > 0 ? (
        <p
          className="mt-2 text-2xs text-muted-foreground"
          data-testid="pulse-entry-supersession-claimed"
        >
          {supersessionClaimedBy.map((pubkey) => (
            <span className="mr-2 block" key={pubkey}>
              supersession claimed by {truncatePubkey(pubkey)}
            </span>
          ))}
        </p>
      ) : null}

      {unhonored.map((claim) => (
        <p
          className="mt-2 text-2xs text-muted-foreground"
          data-testid="pulse-entry-unhonored-claim"
          key={claim.eventId}
        >
          {claim.reason === "unresolved"
            ? `supersedes ${claim.eventId} (not visible)`
            : `supersedes ${claim.eventId} — not honored (${
                claim.reason === "cross-author"
                  ? "different author"
                  : "older than its target"
              }), the original stays active`}
        </p>
      ))}
    </li>
  );
}
