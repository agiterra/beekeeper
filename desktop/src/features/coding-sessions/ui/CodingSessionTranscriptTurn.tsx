import * as React from "react";
import { ChevronRight } from "lucide-react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  codingSessionTranscriptEntryKey,
  type CodingSessionChangedFile,
  type CodingSessionTranscriptEntry,
  type CodingSessionTranscriptTurn,
  type CodingSessionTurnRestingStatus,
  resolveCodingSessionTurnSettlement,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { deriveCodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import { CompactToolFailureToneContext } from "@/features/agents/ui/AgentSessionToolItem/CompactToolFailureToneContext";
import {
  ACTIVITY_ROW_ICON_CLASS,
  ACTIVITY_ROW_LABEL_CLASS,
  ACTIVITY_ROW_LINE_CLASS,
} from "@/features/agents/ui/AgentSessionToolItem/ToolItemRowClasses";
import { cn } from "@/shared/lib/cn";
import { CodingSessionSubagentEntry } from "./CodingSessionSubagentEntry";
import { useCodingSessionOpenAgentsSurface } from "./CodingSessionTranscriptAgentsSurface";
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
  CodingSessionTurnSettlementContext,
} from "./CodingSessionTranscriptItem";
import { CodingSessionChangedFilesCard } from "./CodingSessionTranscriptParts";
import {
  codingSessionEntryRowKind,
  codingSessionRowGap,
  codingSessionTurnTailGap,
  type CodingSessionRowKind,
} from "./CodingSessionTranscriptRhythm";
import { CodingSessionWorking } from "./CodingSessionTranscriptWorking";

/**
 * What the transcript's caller knows about its turns beyond the items.
 *
 * `restingStatus` is the session's status for a latest turn that has no
 * completion and is not being worked on — `unknown` unless the caller can
 * vouch otherwise (see `resolveCodingSessionTurnSettlement`).
 * `showWorkingRow: false` keeps the working line out of a transcript that is
 * a fragment of a turn whose working line is already on screen — Mission
 * Live's execution bundle under its block's narrative.
 *
 * A React context, not a module-level cache: nothing outlives the render
 * tree, so `resetCommunityState()` has nothing to reset.
 */
export type CodingSessionTranscriptTurnPolicy = {
  restingStatus: CodingSessionTurnRestingStatus;
  showWorkingRow: boolean;
};

export const CodingSessionTranscriptTurnPolicyContext =
  React.createContext<CodingSessionTranscriptTurnPolicy>({
    restingStatus: "unknown",
    showWorkingRow: true,
  });

/**
 * One turn of the conversation. Split out of `CodingSessionTranscript.tsx`
 * for the 1000-line ceiling.
 *
 * While the turn is live every entry is shown in order, with the working line
 * at the bottom where the work is happening. Once it settles, the model's
 * `fold` hides the work behind one "Worked for …" row at the first hidden
 * entry's place; opening that row restores every entry where it was. The
 * answer, changed files, and any stop or failure are never folded; a failed
 * step that folded reads quiet once the fold is opened (SV-02).
 *
 * Spacing is by what sits next to what (SV-08, `CodingSessionTranscriptRhythm`)
 * — no rule above each turn; the hairline under the fold row is the only one.
 * The answer, the changed files and the line under them form one hover block,
 * so its copy, time and cost appear while that block is pointed at (SV-07).
 */
export const CodingSessionTurn = React.memo(function CodingSessionTurn({
  turn,
}: {
  turn: CodingSessionTranscriptTurn;
}) {
  const [foldOpen, setFoldOpen] = useCodingSessionDisclosure(`fold:${turn.id}`);
  const policy = React.useContext(CodingSessionTranscriptTurnPolicyContext);
  const settlement = resolveCodingSessionTurnSettlement(
    turn,
    policy.restingStatus,
  );
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
  const answerIndex = React.useMemo(
    () => (turn.isWorking ? -1 : findCodingSessionAnswerIndex(turn.entries)),
    [turn.entries, turn.isWorking],
  );
  const answerEntry = answerIndex >= 0 ? turn.entries[answerIndex] : undefined;
  const answerText =
    answerEntry?.kind === "item" && answerEntry.item.type === "message"
      ? answerEntry.item.text.trim()
      : null;

  const lead: React.ReactNode[] = [];
  const answerBlock: React.ReactNode[] = [];
  let previous: CodingSessionRowKind | null = null;
  for (const [index, entry] of turn.entries.entries()) {
    const isAnchor = fold !== null && index === fold.anchorIndex;
    const isHidden = fold !== null && !foldOpen && hidden.has(index);
    // Everything from the answer on is the answer block, in order.
    const target =
      answerIndex >= 0 && index >= answerIndex ? answerBlock : lead;
    if (isAnchor && fold) {
      const gap = codingSessionRowGap(previous, "fold");
      target.push(
        <div className={gap.className || undefined} key={`fold:${turn.id}`}>
          <CodingSessionWorkedFold
            fold={fold}
            onToggle={toggleFold}
            open={foldOpen}
            startedAt={turn.startedAt}
          />
        </div>,
      );
      previous = "fold";
    }
    if (isHidden) continue;
    const kind = codingSessionEntryRowKind(entry);
    const gap = codingSessionRowGap(previous, kind);
    // SV-02/D2: a step the fold hides is shown by opening it as a step — a
    // failed call there reads quiet (dimmed icon, "· exit 2"). Kept failures
    // (after the answer, or in a turn that never answered) stay loud.
    const folded = fold !== null && hidden.has(index);
    target.push(
      <div
        className={gap.className || undefined}
        data-folded={folded ? "" : undefined}
        data-row-kind={kind}
        key={codingSessionTranscriptEntryKey(entry)}
      >
        <CompactToolFailureToneContext.Provider
          value={folded ? "quiet" : "alarm"}
        >
          <CodingSessionEntry entry={entry} />
        </CompactToolFailureToneContext.Provider>
      </div>,
    );
    previous = kind;
  }

  const tail = turn.isWorking ? lead : answerBlock;
  if (turn.isWorking && policy.showWorkingRow) {
    tail.push(
      <div
        className={
          codingSessionTurnTailGap(previous, "working").className || undefined
        }
        key="working"
      >
        <CodingSessionWorking
          showThinking={turn.entries.every(isUserPromptEntry)}
          startedAt={turn.startedAt}
          stepLabel={stepLabel}
        />
      </div>,
    );
  }
  if (turn.changedFiles.length > 0) {
    tail.push(
      <div
        className={
          codingSessionTurnTailGap(previous, "changed-files").className ||
          undefined
        }
        key="changed-files"
      >
        <CodingSessionTurnChangedFiles
          files={turn.changedFiles}
          turnId={turn.id}
        />
      </div>,
    );
  }
  // The line under the answer exists only when it has something to say.
  if (
    !turn.isWorking &&
    (turn.completion !== null || turn.diagnostics.length > 0)
  ) {
    answerBlock.push(
      <div
        className={
          codingSessionTurnTailGap(previous, "meta").className || undefined
        }
        key="completion"
      >
        <CodingSessionTurnCompletion
          answerText={answerText}
          completion={turn.completion}
          diagnostics={turn.diagnostics}
          durationShownInWorkFold={fold !== null && fold.durationMs !== null}
          turnId={turn.id}
        />
      </div>,
    );
  }

  return (
    <CodingSessionTurnSettlementContext.Provider value={settlement}>
      <section
        className="content-visibility-auto flex flex-col"
        data-fold={fold ? (foldOpen ? "open" : "closed") : undefined}
        data-message-id={`turn:${turn.id}`}
        data-testid="coding-session-turn"
        data-turn-id={turn.id}
      >
        {lead}
        {answerBlock.length > 0 ? (
          <div
            className="group/answer flex flex-col"
            data-testid="coding-session-answer-block"
          >
            {answerBlock}
          </div>
        ) : null}
      </section>
    </CodingSessionTurnSettlementContext.Provider>
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
      {/* SV-01/SV-06: the shared activity row; the chevron is its 16px icon. */}
      <button
        aria-expanded={open}
        className={cn("cursor-pointer", ACTIVITY_ROW_LINE_CLASS)}
        onClick={() => setOpen(!open)}
        type="button"
      >
        <ChevronRight
          aria-hidden
          className={cn(
            ACTIVITY_ROW_ICON_CLASS,
            "transition-transform",
            open && "rotate-90",
          )}
        />
        <span className={ACTIVITY_ROW_LABEL_CLASS}>{entry.label}</span>
      </button>
      {open ? (
        <div className="mt-0.5 ml-3 flex flex-col gap-0.5 border-l border-border/60 pl-3">
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
  const openAgentsSurface = useCodingSessionOpenAgentsSurface();
  return (
    <CodingSessionSubagentEntry
      {...disclosure}
      entry={entry}
      onOpenAgentsSurface={openAgentsSurface}
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

/**
 * Where the answer block starts: the turn's last prose, which the line under
 * it copies. `-1` when the turn wrote none.
 *
 * A synthesized Turn result body (`:assistant-result`) is the provider's
 * closing word, not the agent's answer — the fold model
 * (`deriveCodingSessionTurnFold`) excludes it from the terminal answer the
 * same way, so "Copy response" copies what the agent wrote and the agent's
 * answer stays inside the hover block. The result body still renders after it,
 * and is the answer only when the agent wrote no prose of its own.
 */
export function findCodingSessionAnswerIndex(
  entries: CodingSessionTranscriptEntry[],
): number {
  let resultBodyIndex = -1;
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const entry = entries[index];
    if (
      entry?.kind !== "item" ||
      entry.item.type !== "message" ||
      entry.item.role !== "assistant" ||
      !entry.item.text.trim()
    ) {
      continue;
    }
    if (!entry.item.id.endsWith(":assistant-result")) return index;
    if (resultBodyIndex < 0) resultBodyIndex = index;
  }
  // A turn whose only prose is the provider's result body: that is all
  // there is to copy.
  return resultBodyIndex;
}
