import type { CodingSessionHistoryCompleteness } from "@/features/coding-sessions/lib/codingSessionTrustedIngressPaging";

/**
 * What a transcript says about the part of its history this client does not
 * hold yet (SV-116).
 *
 * The newest page renders the moment it lands, so for a long session the top
 * of the transcript is, for a while, not the start of the session. A
 * transcript that reads as whole while it is not is the same lie as one that
 * invents a turn — so the gap is named at the top, quietly while it is still
 * filling and plainly once paging has stopped short. Nothing is shown when the
 * history is complete or when the newest page has not answered (the
 * surface's own loading state covers that).
 */
export function codingSessionHistoryDisclosureText(
  completeness: CodingSessionHistoryCompleteness,
): string | null {
  switch (completeness.state) {
    case "pending":
    case "complete":
      return null;
    case "loading-earlier":
      return "Loading earlier events…";
    case "incomplete":
      switch (completeness.reason) {
        case "error":
          return "Earlier events could not be loaded, so this transcript starts partway through. They will be retried when the relay reconnects.";
        case "page-budget":
          return "This session is longer than this view loads; its earliest events are not shown.";
        case "crowded-second":
          return "Some earlier events could not be loaded: more events share one second than the relay returns at once.";
        case "not-paged":
          return "Only the most recent events are loaded here; earlier events are not shown.";
        default:
          return "Earlier events could not be loaded, so this transcript starts partway through.";
      }
  }
}

export function CodingSessionHistoryDisclosure({
  completeness,
}: {
  completeness: CodingSessionHistoryCompleteness;
}) {
  const text = codingSessionHistoryDisclosureText(completeness);
  if (text === null) return null;
  const loading = completeness.state === "loading-earlier";
  return (
    <div
      aria-live="polite"
      className="flex min-h-6 items-center justify-center gap-2 text-center text-xs text-muted-foreground"
      data-state={completeness.state}
      data-testid="coding-session-history-disclosure"
      role="status"
      title={completeness.message ?? undefined}
    >
      {loading ? (
        <span
          aria-hidden
          className="size-1.5 shrink-0 animate-pulse rounded-full bg-muted-foreground/60"
        />
      ) : null}
      <span>{text}</span>
    </div>
  );
}
