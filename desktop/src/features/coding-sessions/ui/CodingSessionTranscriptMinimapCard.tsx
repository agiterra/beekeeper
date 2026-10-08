import {
  codingSessionMinimapPromptLine,
  codingSessionMinimapReplyPreview,
  type CodingSessionMinimapItem,
} from "@/features/coding-sessions/lib/codingSessionTranscriptMinimapItems";
import {
  codingSessionMinimapGateLine,
  type CodingSessionMinimapMarks,
} from "@/features/coding-sessions/lib/codingSessionTranscriptMinimapMarks";
import { formatCodingSessionDuration } from "@/features/coding-sessions/lib/codingSessionTranscriptModelFormat";
import { CODING_SESSION_LAST_SEEN_LABEL } from "@/features/coding-sessions/lib/codingSessionLastSeen";
import type { CodingSessionMinimapFacts } from "./CodingSessionTranscriptMinimapData";

/**
 * The minimap's hover card (SV-26): the prompt's first line and the start of
 * the reply, as T3 shows them. Beyond T3 (SV-27): who prompted the turn, its
 * duration, its changed files (count and the first three names), the gate
 * rows signed during it, its other marks, and — on the first turn after the
 * "since you were here" rule — that the rule is this device's alone.
 *
 * Every absent number says which absence it is: "duration not reported" is
 * not "0s", and an unread gate source is not "no gate row".
 */
export function CodingSessionTranscriptMinimapCard({
  authorLabel,
  facts,
  firstSinceLastSeen,
  item,
  marks,
}: {
  authorLabel: string;
  facts: CodingSessionMinimapFacts;
  firstSinceLastSeen: boolean;
  item: CodingSessionMinimapItem;
  marks: CodingSessionMinimapMarks | null;
}) {
  const prompt = codingSessionMinimapPromptLine(item.userText);
  const reply = codingSessionMinimapReplyPreview(item.assistantText);
  const hiddenFiles = item.changedFileCount - item.changedFileNames.length;
  return (
    <span
      className="block rounded-xl border border-border/70 bg-popover p-3 text-left text-popover-foreground shadow-xl shadow-black/25"
      data-testid="coding-session-minimap-card"
    >
      <span
        className="block max-w-full truncate text-sm font-medium leading-5"
        data-testid="coding-session-minimap-card-prompt"
      >
        {prompt ?? "Prompt with no text"}
      </span>
      {reply ? (
        <span
          className="mt-1 line-clamp-3 text-sm leading-5 text-muted-foreground"
          data-testid="coding-session-minimap-card-reply"
        >
          {reply}
        </span>
      ) : (
        <span className="mt-1 block text-xs text-muted-foreground">
          {item.working ? "Reply in progress" : "No reply text"}
        </span>
      )}
      <span
        className="mt-2 flex flex-col gap-0.5 border-t border-border/60 pt-2 text-xs text-muted-foreground"
        data-testid="coding-session-minimap-card-facts"
      >
        <span data-testid="coding-session-minimap-card-author">
          Prompted by {authorLabel}
        </span>
        <span data-testid="coding-session-minimap-card-duration">
          {item.working
            ? "Running now"
            : item.durationMs === null
              ? "Duration not reported"
              : `Took ${formatCodingSessionDuration(item.durationMs)}`}
        </span>
        <span data-testid="coding-session-minimap-card-files">
          {item.changedFileCount === 0
            ? "No file changes recorded"
            : `${item.changedFileCount} ${item.changedFileCount === 1 ? "file" : "files"} changed: ${item.changedFileNames.join(", ")}${hiddenFiles > 0 ? ` and ${hiddenFiles} more` : ""}`}
        </span>
        <span data-testid="coding-session-minimap-card-gates">
          {facts.gates.state === "read"
            ? codingSessionMinimapGateLine(marks?.gate ?? null)
            : facts.gates.reason}
        </span>
        {marks?.rewound ? (
          <span data-testid="coding-session-minimap-card-rewind">
            Rewound just before this turn · seeded from the record
          </span>
        ) : null}
        {marks?.failed ? (
          <span className="text-destructive">This turn failed</span>
        ) : null}
        {marks && marks.handovers > 0 ? (
          <span>
            {marks.handovers === 1
              ? "1 handover record signed during this turn"
              : `${marks.handovers} handover records signed during this turn`}
          </span>
        ) : null}
        {marks && marks.waitingRulings > 0 ? (
          <span
            className="text-amber-700 dark:text-amber-300"
            data-testid="coding-session-minimap-card-ruling"
          >
            {marks.waitingRulings === 1
              ? "Waiting on a ruling opened during this turn"
              : `Waiting on ${marks.waitingRulings} rulings opened during this turn`}
          </span>
        ) : null}
        {firstSinceLastSeen ? (
          <span data-testid="coding-session-minimap-card-since">
            New since you were here ({CODING_SESSION_LAST_SEEN_LABEL})
          </span>
        ) : null}
        {facts.notes.map((note) => (
          <span key={note}>{note}</span>
        ))}
      </span>
    </span>
  );
}
