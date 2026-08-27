import { AlertTriangle, ShieldCheck, ShieldX } from "lucide-react";

import { Badge } from "@/shared/ui/badge";
import type { CodingSessionObserverView } from "./observer-contract.ts";

const NOTE_CLASS =
  "flex items-start gap-2 text-xs text-black/50 dark:text-white/50";

/**
 * The lines that keep the observer honest about what it read.
 *
 * `signaturesVerified` is true in every build shipped so far. It is printed
 * anyway: if it ever goes false, the reader has started rendering facts it
 * did not check, and that must be visible on the screen rather than buried in
 * a snapshot field nobody looks at.
 */
export function CodingSessionTrustNotes({
  view,
}: {
  view: CodingSessionObserverView;
}) {
  const { counts } = view;
  const droppedFacts = counts.malformed + counts.invalidSignature;
  return (
    <div className="mt-4 space-y-1.5" data-testid="coding-session-trust-notes">
      <p className={NOTE_CLASS}>
        {view.signaturesVerified ? (
          <ShieldCheck className="mt-0.5 h-3.5 w-3.5 shrink-0 text-emerald-600 dark:text-emerald-400" />
        ) : (
          <ShieldX className="mt-0.5 h-3.5 w-3.5 shrink-0 text-red-600 dark:text-red-400" />
        )}
        <span>
          Signatures verified on this device:{" "}
          {view.signaturesVerified ? "yes" : "no"}
        </span>
      </p>
      {view.truncatedAt1000 && (
        <p className={NOTE_CLASS}>
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-600 dark:text-amber-400" />
          <span>
            History truncated at 1000 events — older activity is not shown.
          </span>
        </p>
      )}
      {droppedFacts > 0 && (
        <p className={NOTE_CLASS}>
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-600 dark:text-amber-400" />
          <span>
            {droppedFacts} event{droppedFacts === 1 ? "" : "s"} dropped (
            {counts.malformed} malformed, {counts.invalidSignature} with an
            invalid signature).
          </span>
        </p>
      )}
      {counts.conflict > 0 && (
        <p className={NOTE_CLASS}>
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-600 dark:text-amber-400" />
          <span>
            {counts.conflict} conflicting payload
            {counts.conflict === 1 ? "" : "s"} could not be resolved and{" "}
            {counts.conflict === 1 ? "is" : "are"} not rendered.
          </span>
        </p>
      )}
    </div>
  );
}

/**
 * D5's fallback disclosure. A generation whose authority came from the
 * first-seen metadata signer instead of a readable 44221 create is labelled,
 * never silently trusted.
 */
export function CodingSessionAuthorityBadge() {
  return (
    <Badge
      variant="outline"
      className="border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300"
    >
      authority unverified
    </Badge>
  );
}

/** The read-only footer every session surface carries. */
export function CodingSessionReadOnlyFooter() {
  return (
    <p className="mt-6 border-t border-black/10 pt-4 text-xs text-black/50 dark:border-white/10 dark:text-white/50">
      Read-only in the browser — sessions are observed here, never driven.
    </p>
  );
}
