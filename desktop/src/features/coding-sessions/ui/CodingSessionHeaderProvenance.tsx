import type { ReactNode } from "react";

import {
  renderCodingSessionContextLoad,
  type CodingSessionContextLoad,
} from "@/features/coding-sessions/lib/codingSessionContextLoad";

// Split out of `CodingSessionHeader.tsx` (1,000-line ceiling). Nothing about
// the popover body changed in the move; the header re-exports every name.

/**
 * One routed seat's line in the provenance popover.
 *
 * `line` is `describeCodingSessionRouting`'s sentence verbatim — `routed:
 * builder/standard → claude-primary/sonnet (medium) — <reason>`. Rendered as
 * one line, never a panel: a routing decision that needs its own screen to be
 * readable is a decision nobody reads. A seat nothing routed contributes no
 * row at all, because "not routed" and "routed to the default" are different
 * facts and only one of them happened.
 */
export type CodingSessionRoutedSeatRow = {
  key: string;
  line: string;
};

/** One execution's line in the provenance popover's `Context` section. */
export type CodingSessionContextRow = {
  key: string;
  /** How the seat names itself — `Actor · Role`, or runtime and model. */
  label: string;
  /** What the wire reported, or null when nothing has. */
  load: CodingSessionContextLoad | null;
};

/**
 * The provenance popover's body (SURFACES.md D7).
 *
 * The 2026-08-29 walk (finding 5) read the shipped popover in full — channel,
 * signed projection, verified source — and found it answered none of the
 * questions it exists to answer: no founder, though the 44226 genesis carries
 * one and **W6 says a founder is never unknown**, and no context, though the
 * 44225 usage items were on the wire and `bee sessions status` printed 27% of
 * a 1M window from exactly them. Both rows live here now, rendered the way
 * the CLI renders them so the two cannot drift apart.
 *
 * Exported so the copy can be asserted directly: Radix does not mount popover
 * content until it opens, so a test that renders the header alone sees none
 * of this.
 */
export function CodingSessionProvenanceDetails({
  channelName = null,
  contextLoads,
  founderDetails,
  generationLabel,
  projectName = null,
  providerAuthorityPubkey = null,
  routedSeats,
}: {
  channelName?: string | null;
  contextLoads?: readonly CodingSessionContextRow[];
  founderDetails?: ReactNode;
  generationLabel: string;
  projectName?: string | null;
  providerAuthorityPubkey?: string | null;
  /** The routed seats in this umbrella, one line each. */
  routedSeats?: readonly CodingSessionRoutedSeatRow[];
}) {
  return (
    <div data-testid="coding-session-provenance-details">
      <p className="text-sm font-medium">Shared session details</p>
      <dl className="mt-3 grid gap-2 text-xs">
        {projectName ? (
          <div>
            <dt className="text-muted-foreground">Project</dt>
            <dd className="mt-0.5 wrap-break-word">{projectName}</dd>
          </div>
        ) : null}
        {channelName ? (
          <div>
            <dt className="text-muted-foreground">Channel</dt>
            <dd className="mt-0.5 wrap-break-word">#{channelName}</dd>
          </div>
        ) : null}
        <div>
          <dt className="text-muted-foreground">Signed projection</dt>
          <dd className="mt-0.5 wrap-break-word">{generationLabel}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Founded by</dt>
          <dd className="mt-0.5 wrap-break-word">
            {founderDetails ?? (
              <span
                data-testid="coding-session-provenance-founder-unresolved"
                title="No genesis or create naming this session's founder has reached this client yet."
              >
                unresolved
              </span>
            )}
          </dd>
        </div>
        {providerAuthorityPubkey ? (
          <div>
            <dt className="text-muted-foreground">Verified source</dt>
            <dd className="mt-0.5 font-mono wrap-break-word">
              {shortPubkey(providerAuthorityPubkey)}
            </dd>
          </div>
        ) : null}
        {routedSeats && routedSeats.length > 0 ? (
          <div data-testid="coding-session-provenance-routing">
            <dt className="text-muted-foreground">Routing</dt>
            {routedSeats.map((row) => (
              <dd
                className="mt-0.5 truncate wrap-break-word"
                data-testid="coding-session-routed-line"
                key={row.key}
                title={row.line}
              >
                {row.line}
              </dd>
            ))}
          </div>
        ) : null}
        {contextLoads && contextLoads.length > 0 ? (
          <div data-testid="coding-session-provenance-context">
            <dt className="text-muted-foreground">Context</dt>
            {contextLoads.map((row) => (
              <dd
                className="mt-0.5 flex items-baseline justify-between gap-2 wrap-break-word"
                key={row.key}
              >
                <span className="min-w-0 truncate">{row.label}</span>
                {row.load === null ? (
                  <span
                    className="shrink-0 text-muted-foreground"
                    title="no usage reported"
                  >
                    {renderCodingSessionContextLoad(null)}
                  </span>
                ) : (
                  <span className="shrink-0 font-mono">
                    {renderCodingSessionContextLoad(row.load)}
                  </span>
                )}
              </dd>
            ))}
          </div>
        ) : null}
      </dl>
    </div>
  );
}

function shortPubkey(value: string): string {
  return value.length <= 20
    ? value
    : `${value.slice(0, 10)}…${value.slice(-8)}`;
}
