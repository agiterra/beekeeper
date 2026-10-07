/**
 * H-08 — maps Beekeeper `TranscriptItem`s to export-bundle messages.
 *
 * The banked bundle law (`conformance/transcript-export/`) constrains the
 * envelope and the user_prompt attachment participation rule; the per-kind
 * shapes below are Beekeeper-defined for the Beekeeper viewer. Beekeeper transcript items
 * carry no file attachments today, so user_prompt messages map with an empty
 * attachments list — the engine's attachment matrix stays law-complete but
 * production-dormant.
 */
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { joinConsecutiveCodingSessionProse } from "@/features/coding-sessions/lib/codingSessionTranscriptModelText";

import type { TranscriptExportMessage } from "./transcriptExportEngine";

/**
 * A coding session's transcript as export messages: its prose pieces joined
 * into the messages the agent wrote first (SV-36 S5,
 * `conformance/transcript-prose-join/CONTRACT.md`), so one paragraph-streamed
 * answer exports as one `assistant_text`, keyed on its first piece. The join
 * runs over the whole transcript, so any other item — a status included —
 * ends a message, exactly as the contract says.
 */
export function mapCodingSessionTranscriptToExportMessages(
  items: readonly TranscriptItem[],
): TranscriptExportMessage[] {
  return mapTranscriptItemsToExportMessages(
    joinConsecutiveCodingSessionProse(items),
  );
}

/** Items to export messages, one to one. */
export function mapTranscriptItemsToExportMessages(
  items: readonly TranscriptItem[],
): TranscriptExportMessage[] {
  return items.map((item) => mapTranscriptItem(item));
}

function mapTranscriptItem(item: TranscriptItem): TranscriptExportMessage {
  switch (item.type) {
    case "message":
      return item.role === "user"
        ? {
            kind: "user_prompt",
            id: item.id,
            text: item.text,
            timestamp: item.timestamp,
            attachments: [],
          }
        : {
            kind: "assistant_text",
            id: item.id,
            text: item.text,
            timestamp: item.timestamp,
          };
    case "thought":
    case "plan":
    case "lifecycle":
      return {
        kind: item.type,
        id: item.id,
        title: item.title,
        text: item.text,
        timestamp: item.timestamp,
      };
    case "metadata":
      return {
        kind: "lifecycle",
        id: item.id,
        title: item.title,
        text: item.sections
          .map((section) => `${section.title}\n${section.body}`)
          .join("\n\n"),
        timestamp: item.timestamp,
      };
    case "tool":
      return {
        kind: "tool_call",
        id: item.id,
        title: item.title,
        toolName: item.toolName,
        status: item.status,
        isError: item.isError,
        text: item.result,
        timestamp: item.timestamp,
      };
  }
}
