import * as React from "react";
import { ChevronRight } from "lucide-react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  codingSessionTranscriptEntryKey,
  type CodingSessionChangedFile,
  type CodingSessionTranscriptEntry,
  type CodingSessionTranscriptTurn,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { deriveCodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import { cn } from "@/shared/lib/cn";
import { CodingSessionSubagentEntry } from "./CodingSessionSubagentEntry";
import {
  CodingSessionTurnCompletion,
  CodingSessionWorkedFold,
} from "./CodingSessionTranscriptCompletion";
import {
  useCodingSessionDisclosure,
  useCodingSessionDisclosureProps,
} from "./CodingSessionTranscriptDisclosure";
import {
  CodingSessionItem,
  CodingSessionToolRow,
  CodingSessionTranscriptGenerationContext,
} from "./CodingSessionTranscriptItem";
import { CodingSessionChangedFilesCard } from "./CodingSessionTranscriptParts";
import { CodingSessionWorking } from "./CodingSessionTranscriptWorking";

/**
 * One turn of the conversation. Split out of `CodingSessionTranscript.tsx`
 * for the 1000-line ceiling.
 *
 * While the turn is live every entry is shown in order, with the working line
 * at the bottom where the work is happening. Once it settles, the model's
 * `fold` hides the work behind one "Worked for …" row at the first hidden
 * entry's place; opening that row restores every entry where it was. The
 * answer, changed files, and any stop or failure are never folded.
 */
export const CodingSessionTurn = React.memo(function CodingSessionTurn({
  turn,
}: {
  turn: CodingSessionTranscriptTurn;
}) {
  const [foldOpen, setFoldOpen] = useCodingSessionDisclosure(`fold:${turn.id}`);
  const fold = turn.fold;
  const hidden = React.useMemo(
    () => new Set(fold?.hiddenIndexes ?? []),
    [fold],
  );
  const toggleFold = React.useCallback(
    () => setFoldOpen(!foldOpen),
    [foldOpen, setFoldOpen],
  );
  const stepLabel = React.useMemo(
    () => (turn.isWorking ? findCurrentStep(turn.entries) : null),
    [turn.entries, turn.isWorking],
  );
  const answerText = React.useMemo(
    () => (turn.isWorking ? null : findAnswerText(turn.entries)),
    [turn.entries, turn.isWorking],
  );

  return (
    <section
      className="group/turn content-visibility-auto flex flex-col gap-2.5 border-t border-border/40 pt-5 first:border-t-0 first:pt-0"
      data-fold={fold ? (foldOpen ? "open" : "closed") : undefined}
      data-message-id={`turn:${turn.id}`}
      data-testid="coding-session-turn"
      data-turn-id={turn.id}
    >
      {turn.entries.map((entry, index) => {
        const isAnchor = fold !== null && index === fold.anchorIndex;
        const isHidden = fold !== null && !foldOpen && hidden.has(index);
        if (!isAnchor && isHidden) return null;
        return (
          <React.Fragment key={codingSessionTranscriptEntryKey(entry)}>
            {isAnchor && fold ? (
              <CodingSessionWorkedFold
                fold={fold}
                onToggle={toggleFold}
                open={foldOpen}
              />
            ) : null}
            {isHidden ? null : <CodingSessionEntry entry={entry} />}
          </React.Fragment>
        );
      })}
      {turn.isWorking ? (
        <CodingSessionWorking
          showThinking={turn.entries.every(isUserPromptEntry)}
          startedAt={turn.startedAt}
          stepLabel={stepLabel}
        />
      ) : null}
      <CodingSessionTurnChangedFiles
        files={turn.changedFiles}
        turnId={turn.id}
      />
      {turn.isWorking ? null : (
        <CodingSessionTurnCompletion
          answerText={answerText}
          completion={turn.completion}
          diagnostics={turn.diagnostics}
          durationShownInWorkFold={fold !== null && fold.durationMs !== null}
          turnId={turn.id}
        />
      )}
    </section>
  );
});

function CodingSessionTurnChangedFiles({
  files,
  turnId,
}: {
  files: CodingSessionChangedFile[];
  turnId: string;
}) {
  const disclosure = useCodingSessionDisclosureProps(`changed-files:${turnId}`);
  return <CodingSessionChangedFilesCard {...disclosure} files={files} />;
}

/** One entry: an item, a sentence row of tool calls, or a subagent batch. */
export const CodingSessionEntry = React.memo(function CodingSessionEntry({
  entry,
}: {
  entry: CodingSessionTranscriptEntry;
}) {
  if (entry.kind === "tool-group") {
    return <CodingSessionToolGroup entry={entry} />;
  }
  if (entry.kind === "subagents") {
    return <CodingSessionSubagents entry={entry} />;
  }
  return <CodingSessionItem item={entry.item} />;
});

/**
 * Consecutive settled tool calls as one sentence row. The calls are built
 * only while it is open, and each keeps its own open state inside it.
 */
function CodingSessionToolGroup({
  entry,
}: {
  entry: Extract<CodingSessionTranscriptEntry, { kind: "tool-group" }>;
}) {
  const generationId = React.useContext(
    CodingSessionTranscriptGenerationContext,
  );
  const [open, setOpen] = useCodingSessionDisclosure(`group:${entry.id}`);
  return (
    <div
      data-count={entry.items.length}
      data-testid="coding-session-tool-group"
    >
      <button
        aria-expanded={open}
        className="flex min-h-6 w-fit max-w-full items-center gap-1.5 rounded-md px-0.5 text-left text-sm text-muted-foreground transition-colors hover:text-foreground"
        onClick={() => setOpen(!open)}
        type="button"
      >
        <ChevronRight
          className={cn(
            "size-3.5 shrink-0 transition-transform",
            open && "rotate-90",
          )}
        />
        <span className="min-w-0 truncate font-medium">{entry.label}</span>
      </button>
      {open ? (
        <div className="mt-0.5 ml-1 flex flex-col gap-0.5 border-l border-border/60 pl-4">
          {entry.items.map((item) => (
            <CodingSessionToolRow
              generationId={generationId}
              item={item}
              key={item.id}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function CodingSessionSubagents({
  entry,
}: {
  entry: Extract<CodingSessionTranscriptEntry, { kind: "subagents" }>;
}) {
  const disclosure = useCodingSessionDisclosureProps(`subagents:${entry.id}`);
  return (
    <CodingSessionSubagentEntry
      {...disclosure}
      entry={entry}
      renderChild={renderSubagentChild}
    />
  );
}

function renderSubagentChild(item: TranscriptItem): React.ReactNode {
  return <CodingSessionItem item={item} />;
}

function isUserPromptEntry(entry: CodingSessionTranscriptEntry): boolean {
  return (
    entry.kind === "item" &&
    entry.item.type === "message" &&
    entry.item.role === "user"
  );
}

/** The live plan's current step, for the working line. */
function findCurrentStep(entries: CodingSessionTranscriptEntry[]) {
  let current: string | null = null;
  for (const entry of entries) {
    if (entry.kind !== "item") continue;
    const plan = deriveCodingSessionTaskModel([entry.item]);
    if (!plan) continue;
    const task =
      plan.tasks.find((candidate) => candidate.status === "in_progress") ??
      plan.tasks.find((candidate) => candidate.status !== "completed") ??
      null;
    current = task?.text ?? null;
  }
  return current;
}

/** The text the completion line's Copy copies: the turn's last prose. */
function findAnswerText(entries: CodingSessionTranscriptEntry[]) {
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const entry = entries[index];
    if (
      entry?.kind === "item" &&
      entry.item.type === "message" &&
      entry.item.role === "assistant" &&
      entry.item.text.trim()
    ) {
      return entry.item.text.trim();
    }
  }
  return null;
}
