import type { CodingSessionLaneMessage } from "@/features/coding-sessions/lib/codingSessionConversationLane";
import {
  resolveCodingSessionPromptAuthor,
  type CodingSessionPromptSeatResolver,
} from "@/features/coding-sessions/lib/codingSessionPromptAttribution";
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
  resolveSeat,
}: {
  currentUserPubkey: string | null;
  message: CodingSessionLaneMessage;
  /**
   * Mission's shared card grammar. Conversation never passes it, so the
   * one-seat lens keeps the exact bubble it has always rendered.
   */
  missionRowClassName?: string;
  operatorProfiles: UserProfileLookup | undefined;
  /**
   * Names a seat from its actor pubkey. A seat can post into the lane too, and
   * without this its message read as a bare key — or, when the founder's own
   * Desktop signed it, as `You`.
   */
  resolveSeat?: CodingSessionPromptSeatResolver;
}) {
  const author = resolveCodingSessionPromptAuthor({
    currentUserPubkey,
    operatorPubkey: message.authorPubkey,
    profiles: operatorProfiles,
    resolveSeat,
  });
  return (
    <div
      className={missionRowClassName ?? "rounded-xl bg-muted/40 px-4 py-2"}
      data-testid="coding-session-umbrella-conversation"
    >
      <p className="text-2xs text-muted-foreground">
        <span
          className={
            author.kind === "unrecorded"
              ? "font-medium text-muted-foreground"
              : "font-medium text-foreground/75"
          }
          data-author-kind={author.kind}
          data-testid="coding-session-umbrella-conversation-author"
        >
          {author.label}
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
