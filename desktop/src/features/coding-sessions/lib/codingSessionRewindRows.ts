/**
 * SV-29: every sentence the rewind surfaces say — the dialog's choices and
 * why one is off, how a published rewind ended, and the collapsed row the
 * rewound turns sit under. Pure and dependency-light, so the transcript
 * item builder can read the status-row half without the wire half.
 *
 * Honesty rules (BRIEF § Acceptance SV-29):
 *
 * - the rewound turns stay in the record: a row says how many and who, it
 *   never removes them;
 * - "restarted with no memory" and "still remembers turns k–n (not
 *   restarted)" are distinct outcomes, each in the provider's words;
 * - the new conversation is "seeded from the record";
 * - the old native session was detached, never said to have forgotten.
 */
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type {
  SessionRewindFilesOutcome,
  SessionRewindReceiptFacts,
} from "@/shared/coordination/sessionCoordinationRewind";
import type {
  CodingSessionRewindBlock,
  CodingSessionRewindOutcome,
} from "./codingSessionRewind";

/** The status-item slug that opens a rewound generation (WIRE-C3b-FINAL). */
export const CODING_SESSION_REWOUND_STATUS = "session_rewound";
/** The transcript row title a `session_rewound` item is built with. */
export const CODING_SESSION_REWOUND_TITLE = "Rewound";
/** The opening words of that row's text. */
export const CODING_SESSION_REWOUND_TEXT = "Rewound to before this turn";

/**
 * The `status` lifecycle row for N+1's dedicated `session_rewound` item:
 * `{kind:"status", status:"session_rewound", commandId, checkpoint,
 * cutGeneration, cutAfterSeq, previousGeneration, files, memory}`. Every word
 * comes from a closed field the provider signed; an item without them is not
 * read as a rewind (the generic "Status" row then shows it).
 */
export function codingSessionRewoundStatusRow(
  item: Record<string, unknown>,
): { title: string; text: string } | undefined {
  if (
    item.status !== CODING_SESSION_REWOUND_STATUS ||
    !hasRewoundItemKeys(item) ||
    typeof item.commandId !== "string" ||
    typeof item.checkpoint !== "string" ||
    !HEX64.test(item.checkpoint) ||
    !Number.isSafeInteger(item.cutGeneration) ||
    !Number.isSafeInteger(item.cutAfterSeq) ||
    !Number.isSafeInteger(item.previousGeneration) ||
    (item.files !== "kept" && item.files !== "restored") ||
    (item.memory !== "seeded" && item.memory !== "none") ||
    (Object.hasOwn(item, "reason") &&
      (item.memory !== "none" || typeof item.reason !== "string"))
  ) {
    return undefined;
  }
  const memory =
    item.memory === "seeded"
      ? "new conversation seeded from the record"
      : "restarted with no memory";
  return {
    title: CODING_SESSION_REWOUND_TITLE,
    text: `${CODING_SESSION_REWOUND_TEXT} · files ${item.files} · ${memory}`,
  };
}

const HEX64 = /^[0-9a-f]{64}$/;
/** The item's exact keys (WIRE-C3b-FINAL § Transcript); `reason` is optional. */
const REWOUND_ITEM_KEYS: ReadonlySet<string> = new Set([
  "kind",
  "status",
  "commandId",
  "checkpoint",
  "cutGeneration",
  "cutAfterSeq",
  "previousGeneration",
  "files",
  "memory",
]);

function hasRewoundItemKeys(item: Record<string, unknown>): boolean {
  const keys = Object.keys(item).filter((key) => key !== "reason");
  return (
    item.kind === "status" &&
    keys.length === REWOUND_ITEM_KEYS.size &&
    keys.every((key) => REWOUND_ITEM_KEYS.has(key))
  );
}

/** True for the transcript row a `session_rewound` item became. */
export function isCodingSessionRewoundRow(item: TranscriptItem): boolean {
  return (
    item.type === "lifecycle" &&
    item.title === CODING_SESSION_REWOUND_TITLE &&
    item.text.startsWith(CODING_SESSION_REWOUND_TEXT)
  );
}

/** The hover action under a prompt, and the dialog's title. */
export const CODING_SESSION_REWIND_ACTION_LABEL = "Edit from here";

/** What the dialog says a rewind does, before either choice. */
export const CODING_SESSION_REWIND_DIALOG_BODY =
  "Rewind to before this prompt and send it again, edited. The turns from here on stay in the record; the agent starts a new conversation seeded from the record up to here.";

/** The line that keeps the dialog from implying the old session forgot. */
export const CODING_SESSION_REWIND_DETACHED_NOTE =
  "The earlier native session is detached, not erased.";

export const CODING_SESSION_REWIND_CHOICES = {
  keep: {
    label: "Chat only",
    detail: "Files on disk stay as they are.",
  },
  restore: {
    label: "Chat and files",
    detail:
      "Files go back to how they were before this turn. HEAD, branches, the index and ignored files are not touched.",
  },
} as const;

/** Why a choice is off, in a person's words. */
export function codingSessionRewindBlockText(
  block: CodingSessionRewindBlock,
): string {
  switch (block) {
    case "outside-generation":
      return "Only this run's turns can be rewound";
    case "session-closed":
      return "This session is closed";
    case "authority-unresolved":
      return "Session authority is still loading";
    case "not-controller":
      return "Only people who can control this session can rewind it";
    case "turn-running":
      return "A turn is running — let it finish or stop it first";
    case "no-checkpoint":
      return "This provider build cannot rewind: it published no checkpoint for this turn";
    case "provider-cannot-rewind":
      return "This provider build cannot rewind";
    case "no-git":
      return "No git checkpoint for this turn";
  }
}

function shortOid(oid: string): string {
  return oid.slice(0, 7);
}

/** What happened to the files, from the provider's signed `files`. */
export function codingSessionRewindFilesText(
  files: SessionRewindFilesOutcome,
  head: string | null,
): string {
  const headClause = head ? ` · HEAD unchanged at ${shortOid(head)}` : "";
  switch (files) {
    case "restored":
      return `Files restored to before this turn${headClause}`;
    case "kept":
      return `Files kept as they are${headClause}`;
    case "restore_failed":
      return "Restoring files failed partway — check the working tree";
  }
}

/** The dialog's lines for one outcome (at most three), in reading order. */
export function codingSessionRewindOutcomeRows(
  outcome: CodingSessionRewindOutcome,
): { tone: "muted" | "warning"; lines: string[] } | null {
  switch (outcome.kind) {
    case "pending":
      return {
        tone: "muted",
        lines: ["Rewinding — waiting for the provider's answer"],
      };
    case "conflict":
      return {
        tone: "warning",
        lines: ["The provider's answers disagree, so nothing is claimed"],
      };
    case "refused":
      return {
        tone: "warning",
        lines: [
          outcome.code === "TREE_BUSY"
            ? `Not rewound — the working tree is busy: ${outcome.message}`
            : `Not rewound: ${outcome.message}`,
          "Nothing was changed.",
        ],
      };
    case "rewound": {
      const lines =
        outcome.memory === "seeded"
          ? ["Rewound — the new conversation is seeded from the record"]
          : // The provider's message itself begins "restarted with no memory".
            ["Rewound, but restarted with no memory", outcome.message ?? ""];
      if (outcome.rewind) {
        lines.push(
          codingSessionRewindFilesText(
            outcome.rewind.files,
            outcome.rewind.head,
          ),
        );
      }
      lines.push(CODING_SESSION_REWIND_DETACHED_NOTE);
      return {
        tone: outcome.memory === "seeded" ? "muted" : "warning",
        lines: lines.filter((line) => line.length > 0),
      };
    }
    case "not-restarted": {
      const lines = [`Not restarted: ${outcome.message}`];
      if (outcome.files) {
        lines.push(codingSessionRewindFilesText(outcome.files, null));
      }
      return { tone: "warning", lines };
    }
  }
}

/**
 * The collapsed row's label: "3 turns rewound by Brian · files restored".
 * A count the view cannot establish is left out rather than guessed.
 */
export function codingSessionRewoundRowLabel(input: {
  count: number | null;
  signer: string;
  rewind: Pick<SessionRewindReceiptFacts, "files">;
  memory: "seeded" | "none";
}): string {
  const turns =
    input.count === null
      ? "Turns"
      : `${input.count} ${input.count === 1 ? "turn" : "turns"}`;
  const files =
    input.rewind.files === "restored"
      ? "files restored"
      : input.rewind.files === "kept"
        ? "files kept"
        : "file restore failed";
  const memory = input.memory === "none" ? " · restarted with no memory" : "";
  return `${turns} rewound by ${input.signer} · ${files}${memory}`;
}
