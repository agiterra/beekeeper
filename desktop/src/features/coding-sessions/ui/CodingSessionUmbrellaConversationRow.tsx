import type { CodingSessionLaneMessage } from "@/features/coding-sessions/lib/codingSessionConversationLane";
import { resolveCodingSessionPromptAuthorLabel } from "@/features/coding-sessions/lib/codingSessionPromptAttribution";
import type { UserProfileLookup } from "@/features/profile/lib/identity";
import { RedactedText } from "@/shared/ui/RedactedPill";

/**
 * One lane message.
 *
 * The author is named, not stamped: the 2026-08-29 walk (finding 4) read the
 * founder's own message in the lane as a bare `a3945536…3cf2`, because this
 * row printed a key while the surrounding surface already held the profiles
 * that resolve it.
 *
 * The body goes through `RedactedText` for the same reason the rest of the
 * transcript does: a lane message can carry a privacy marker, and printing it
 * raw is ninety characters of hash the reader cannot reveal.
 */
export function UmbrellaConversationRow({
  currentUserPubkey,
  message,
  missionRowClassName,
  operatorProfiles,
}: {
  currentUserPubkey: string | null;
  message: CodingSessionLaneMessage;
  /**
   * Mission's shared card grammar. Conversation never passes it, so the
   * one-seat lens keeps the exact bubble it has always rendered.
   */
  missionRowClassName?: string;
  operatorProfiles: UserProfileLookup | undefined;
}) {
  const authorLabel = resolveCodingSessionPromptAuthorLabel({
    currentUserPubkey,
    operatorPubkey: message.authorPubkey,
    profiles: operatorProfiles,
  });
  return (
    <div
      className={missionRowClassName ?? "rounded-xl bg-muted/40 px-4 py-2"}
      data-testid="coding-session-umbrella-conversation"
    >
      <p className="text-2xs text-muted-foreground">
        <span
          className="font-medium text-foreground/75"
          data-testid="coding-session-umbrella-conversation-author"
        >
          {authorLabel}
        </span>{" "}
        · {formatLaneTimestamp(message.timestampMs)}
      </p>
      <p className="mt-0.5 text-base whitespace-pre-wrap wrap-break-word">
        <RedactedText text={message.content} />
      </p>
    </div>
  );
}

/** A lane message's wall-clock time, or "" when the stamp is not a real time. */
export function formatLaneTimestamp(timestampMs: number): string {
  const date = new Date(timestampMs);
  return Number.isFinite(date.getTime())
    ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
    : "";
}
