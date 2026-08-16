import * as React from "react";
import { ChevronDown, CircleAlert, LoaderCircle, Wrench } from "lucide-react";

import { ToolItem } from "@/features/agents/ui/AgentSessionToolItem";
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { TranscriptActivityItem } from "@/features/agents/ui/activityRenderClasses/TranscriptActivityItem";
import {
  deriveCodingSessionTranscriptModel,
  formatCodingSessionDuration,
  isCodingSessionTranscriptError,
  stabilizeCodingSessionTranscriptModel,
  type CodingSessionTranscriptModel,
  type CodingSessionTranscriptEntry,
  type CodingSessionTranscriptTurn,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { Markdown } from "@/shared/ui/markdown";
import { VirtualizedList } from "@/shared/ui/VirtualizedList";
import {
  CodingSessionActiveTool,
  CodingSessionChangedFilesCard,
  CodingSessionDiagnostics,
  CodingSessionTurnCompletion,
  CodingSessionWorking,
} from "./CodingSessionTranscriptParts";

type CodingSessionTranscriptProps = {
  generationId: string;
  isWorking: boolean;
  items: TranscriptItem[];
  scrollRef?: React.RefObject<HTMLElement | null>;
};

const GENERIC_AGENT_IDENTITY = {
  agentAvatarUrl: null,
  agentName: "Coding session",
};

export const CODING_SESSION_VIRTUALIZATION_THRESHOLD = 40;

type CodingSessionTranscriptRow =
  | {
      kind: "block";
      block: CodingSessionTranscriptModel["blocks"][number];
      key: string;
    }
  | {
      kind: "diagnostics";
      diagnostics: TranscriptItem[];
      key: "session-diagnostics";
    };

export function CodingSessionTranscript({
  generationId,
  isWorking,
  items,
  scrollRef,
}: CodingSessionTranscriptProps) {
  const model = useStableCodingSessionTranscriptModel(items, isWorking);
  const rows = React.useMemo(
    () => buildCodingSessionTranscriptRows(model),
    [model],
  );
  const [openDisclosures, setOpenDisclosures] = React.useState<
    ReadonlySet<string>
  >(() => new Set());
  const setDisclosureOpen = React.useCallback((id: string, open: boolean) => {
    setOpenDisclosures((current) => {
      if (current.has(id) === open) return current;
      const next = new Set(current);
      if (open) next.add(id);
      else next.delete(id);
      return next;
    });
  }, []);
  const renderRow = React.useCallback(
    (row: CodingSessionTranscriptRow) => (
      <CodingSessionTranscriptRowContent
        generationId={generationId}
        key={row.key}
        onDisclosureOpenChange={setDisclosureOpen}
        openDisclosures={openDisclosures}
        row={row}
      />
    ),
    [generationId, openDisclosures, setDisclosureOpen],
  );
  const averageEstimatedRowSize = React.useMemo(
    () =>
      rows.length === 0
        ? 160
        : Math.round(
            rows.reduce(
              (total, row) =>
                total + estimateCodingSessionTranscriptRowSize(row),
              0,
            ) / rows.length,
          ),
    [rows],
  );

  if (rows.length === 0) {
    return (
      <div
        className="flex min-h-48 flex-col items-center justify-center text-center"
        data-testid="coding-session-transcript-empty"
      >
        {isWorking ? (
          <LoaderCircle className="size-4 animate-spin text-muted-foreground motion-reduce:animate-none" />
        ) : (
          <Wrench className="size-4 text-muted-foreground" />
        )}
        <p className="mt-3 text-sm font-medium">
          {isWorking ? "Session is working" : "No conversation yet"}
        </p>
        <p className="mt-1 text-sm text-muted-foreground">
          {isWorking
            ? "The first response will appear here as it streams."
            : "Send a prompt to begin this coding session."}
        </p>
      </div>
    );
  }

  const shouldVirtualize =
    rows.length > CODING_SESSION_VIRTUALIZATION_THRESHOLD && scrollRef;

  return (
    <div
      aria-label="Live coding-session conversation"
      aria-live="off"
      data-transcript-renderer={shouldVirtualize ? "virtualized" : "static"}
      data-testid="coding-session-transcript"
      role="log"
    >
      {shouldVirtualize ? (
        <VirtualizedList
          estimateSize={averageEstimatedRowSize}
          getItemKey={getCodingSessionTranscriptRowKey}
          innerClassName="w-full"
          items={rows}
          overscan={6}
          renderItem={(row) => <div className="pb-5">{renderRow(row)}</div>}
          scrollRef={scrollRef}
        />
      ) : (
        <div className="flex flex-col gap-5">{rows.map(renderRow)}</div>
      )}
      <span
        aria-atomic="true"
        aria-live="polite"
        className="sr-only"
        data-testid="coding-session-live-status"
        role="status"
      >
        {isWorking ? "Coding session working" : "Coding session idle"}
      </span>
    </div>
  );
}

export function buildCodingSessionTranscriptRows(
  model: CodingSessionTranscriptModel,
): CodingSessionTranscriptRow[] {
  const rows: CodingSessionTranscriptRow[] = model.blocks.map((block) => ({
    kind: "block",
    block,
    key: block.kind === "turn" ? `turn:${block.id}` : `item:${block.id}`,
  }));
  if (model.diagnostics.length > 0) {
    rows.push({
      kind: "diagnostics",
      diagnostics: model.diagnostics,
      key: "session-diagnostics",
    });
  }
  return rows;
}

export function getCodingSessionTranscriptRowKey(
  row: CodingSessionTranscriptRow,
): string {
  return row.key;
}

export function estimateCodingSessionTranscriptRowSize(
  row: CodingSessionTranscriptRow,
): number {
  if (row.kind === "diagnostics") return 48;
  if (row.block.kind === "standalone") return 96;
  const entryEstimate = row.block.entries.reduce((height, entry) => {
    if (entry.kind === "tool-group") return height + 36;
    if (entry.item.type === "message") {
      return height + Math.min(320, 52 + entry.item.text.length / 3);
    }
    return height + 40;
  }, 0);
  const stateEstimate =
    (row.block.isWorking ? 28 : 0) +
    (row.block.completion ? 24 : 0) +
    (row.block.changedFiles.length > 0 ? 48 : 0) +
    (row.block.diagnostics.length > 0 ? 32 : 0);
  return Math.max(88, Math.min(720, entryEstimate + stateEstimate + 20));
}

function CodingSessionTranscriptRowContent({
  generationId,
  onDisclosureOpenChange,
  openDisclosures,
  row,
}: {
  generationId: string;
  onDisclosureOpenChange: (id: string, open: boolean) => void;
  openDisclosures: ReadonlySet<string>;
  row: CodingSessionTranscriptRow;
}) {
  if (row.kind === "diagnostics") {
    const disclosureId = "session:diagnostics";
    return (
      <CodingSessionDiagnostics
        diagnostics={row.diagnostics}
        disclosureId={disclosureId}
        label="Session details"
        onOpenChange={onDisclosureOpenChange}
        open={openDisclosures.has(disclosureId)}
      />
    );
  }
  if (row.block.kind === "turn") {
    return (
      <CodingSessionTurn
        generationId={generationId}
        onDisclosureOpenChange={onDisclosureOpenChange}
        openDisclosures={openDisclosures}
        turn={row.block}
      />
    );
  }
  return (
    <CodingSessionStandalone
      block={row.block}
      generationId={generationId}
      onDisclosureOpenChange={onDisclosureOpenChange}
      openDisclosures={openDisclosures}
    />
  );
}

function useStableCodingSessionTranscriptModel(
  items: TranscriptItem[],
  isWorking: boolean,
): CodingSessionTranscriptModel {
  const previousRef = React.useRef<CodingSessionTranscriptModel | null>(null);
  const next = React.useMemo(
    () => deriveCodingSessionTranscriptModel(items, { isWorking }),
    [isWorking, items],
  );
  const stable = stabilizeCodingSessionTranscriptModel(
    previousRef.current,
    next,
  );
  previousRef.current = stable;
  return stable;
}

const CodingSessionStandalone = React.memo(function CodingSessionStandalone({
  block,
  generationId,
  onDisclosureOpenChange,
  openDisclosures,
}: {
  block: Extract<
    CodingSessionTranscriptModel["blocks"][number],
    { kind: "standalone" }
  >;
  generationId: string;
  onDisclosureOpenChange: (id: string, open: boolean) => void;
  openDisclosures: ReadonlySet<string>;
}) {
  return (
    <div
      className="content-visibility-auto"
      data-message-id={block.id}
      data-testid="coding-session-standalone"
    >
      <CodingSessionEntry
        disclosureScope={`standalone:${block.id}`}
        entry={block.entry}
        generationId={generationId}
        onDisclosureOpenChange={onDisclosureOpenChange}
        openDisclosures={openDisclosures}
      />
    </div>
  );
});

const CodingSessionTurn = React.memo(function CodingSessionTurn({
  generationId,
  onDisclosureOpenChange,
  openDisclosures,
  turn,
}: {
  generationId: string;
  onDisclosureOpenChange: (id: string, open: boolean) => void;
  openDisclosures: ReadonlySet<string>;
  turn: CodingSessionTranscriptTurn;
}) {
  const settledWork = React.useMemo(() => findSettledWorkRun(turn), [turn]);

  return (
    <section
      className="content-visibility-auto flex flex-col gap-2.5"
      data-message-id={`turn:${turn.id}`}
      data-testid="coding-session-turn"
      data-turn-id={turn.id}
    >
      {turn.entries.map((entry, index) => {
        if (settledWork && index === settledWork.startIndex) {
          return (
            <CodingSessionWorkedFold
              completion={turn.completion}
              disclosureId={`turn:${turn.id}:worked`}
              entries={settledWork.entries}
              generationId={generationId}
              key={`worked:${turn.id}`}
              onOpenChange={onDisclosureOpenChange}
              open={openDisclosures.has(`turn:${turn.id}:worked`)}
              openDisclosures={openDisclosures}
            />
          );
        }
        if (settledWork?.hiddenIndexes.has(index)) {
          return null;
        }
        return (
          <CodingSessionEntry
            disclosureScope={`turn:${turn.id}`}
            entry={entry}
            generationId={generationId}
            key={entry.kind === "item" ? entry.item.id : entry.id}
            onDisclosureOpenChange={onDisclosureOpenChange}
            openDisclosures={openDisclosures}
          />
        );
      })}
      {turn.isWorking ? (
        <CodingSessionWorking startedAt={turn.startedAt} />
      ) : null}
      {turn.completion ? (
        <CodingSessionTurnCompletion
          completion={turn.completion}
          durationShownInWorkFold={
            settledWork !== null && turn.completion.durationMs !== null
          }
        />
      ) : null}
      <CodingSessionChangedFilesCard
        disclosureId={`turn:${turn.id}:changed-files`}
        files={turn.changedFiles}
        onOpenChange={onDisclosureOpenChange}
        open={openDisclosures.has(`turn:${turn.id}:changed-files`)}
      />
      <CodingSessionDiagnostics
        diagnostics={turn.diagnostics}
        disclosureId={`turn:${turn.id}:diagnostics`}
        label="Turn details"
        onOpenChange={onDisclosureOpenChange}
        open={openDisclosures.has(`turn:${turn.id}:diagnostics`)}
      />
    </section>
  );
});

const CodingSessionEntry = React.memo(function CodingSessionEntry({
  disclosureScope,
  entry,
  generationId,
  onDisclosureOpenChange,
  openDisclosures,
}: {
  disclosureScope: string;
  entry: CodingSessionTranscriptEntry;
  generationId: string;
  onDisclosureOpenChange: (id: string, open: boolean) => void;
  openDisclosures: ReadonlySet<string>;
}) {
  if (entry.kind === "tool-group") {
    const disclosureId = `${disclosureScope}:tools:${entry.id}`;
    return (
      <details
        className="group/tools"
        data-testid="coding-session-tool-group"
        onToggle={(event) =>
          onDisclosureOpenChange(disclosureId, event.currentTarget.open)
        }
        open={openDisclosures.has(disclosureId)}
      >
        <summary
          aria-label={entry.label}
          className="flex min-h-7 w-fit cursor-pointer list-none items-center gap-1.5 rounded-md px-0.5 text-xs text-muted-foreground transition-colors hover:bg-muted/30 hover:text-foreground"
        >
          <ChevronDown className="size-3.5 transition-transform group-open/tools:rotate-180" />
          <span className="font-medium group-open/tools:hidden">
            +{entry.items.length} previous tool{" "}
            {entry.items.length === 1 ? "call" : "calls"}
          </span>
          <span className="hidden font-medium group-open/tools:inline">
            Show fewer tool calls
          </span>
        </summary>
        <div className="mt-1 ml-1 flex flex-col gap-0.5 border-l border-border/60 pl-4">
          {entry.items.map((item) => (
            <ToolItem
              {...GENERIC_AGENT_IDENTITY}
              agentPubkey={generationId}
              item={item}
              key={item.id}
            />
          ))}
        </div>
      </details>
    );
  }

  return (
    <CodingSessionItem
      disclosureId={`${disclosureScope}:item:${entry.item.id}`}
      generationId={generationId}
      item={entry.item}
      onDisclosureOpenChange={onDisclosureOpenChange}
      openDisclosures={openDisclosures}
    />
  );
});

const CodingSessionItem = React.memo(function CodingSessionItem({
  disclosureId,
  generationId,
  item,
  onDisclosureOpenChange,
  openDisclosures,
}: {
  disclosureId: string;
  generationId: string;
  item: TranscriptItem;
  onDisclosureOpenChange: (id: string, open: boolean) => void;
  openDisclosures: ReadonlySet<string>;
}) {
  if (item.type === "message") {
    if (item.role === "user") {
      return (
        <div
          className="group flex flex-col items-end gap-1"
          data-role="user-message"
          data-testid="coding-session-user-message"
        >
          <div className="max-w-[80%] rounded-2xl bg-muted px-4 py-3 text-base leading-6 text-foreground shadow-sm ring-1 ring-border/40">
            <Markdown content={item.text.trim() || " "} mediaInset />
          </div>
          <p className="pe-1 text-2xs text-muted-foreground">
            <span className="font-medium text-foreground/75">You</span>
            {formatCodingSessionMessageTimestamp(item.timestamp)}
          </p>
        </div>
      );
    }

    return (
      <article
        className="min-w-0 text-sm leading-6 text-foreground"
        data-role="assistant-message"
        data-testid="coding-session-assistant-message"
      >
        <Markdown className="leading-6" content={item.text.trim() || " "} />
      </article>
    );
  }

  if (item.type === "tool") {
    if (item.status === "executing" || item.status === "pending") {
      return (
        <CodingSessionActiveTool
          disclosureId={disclosureId}
          item={item}
          onOpenChange={onDisclosureOpenChange}
          open={openDisclosures.has(disclosureId)}
        />
      );
    }
    return (
      <ToolItem
        {...GENERIC_AGENT_IDENTITY}
        agentPubkey={generationId}
        item={item}
      />
    );
  }

  if (isCodingSessionTranscriptError(item)) {
    return (
      <div
        className="rounded-lg border border-destructive/25 bg-destructive/5 px-3 py-2 text-sm text-destructive"
        data-testid="coding-session-error"
        role="alert"
      >
        <div className="flex items-start gap-2">
          <CircleAlert className="mt-0.5 size-4 shrink-0" />
          <div className="min-w-0">
            <p className="font-medium">{item.title || "Session error"}</p>
            {"text" in item && item.text ? (
              <p className="mt-1 whitespace-pre-wrap text-xs opacity-85">
                {item.text}
              </p>
            ) : null}
          </div>
        </div>
      </div>
    );
  }

  return (
    <TranscriptActivityItem
      {...GENERIC_AGENT_IDENTITY}
      agentPubkey={generationId}
      item={item}
    />
  );
});

function formatCodingSessionMessageTimestamp(timestamp: string): string {
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime())) return "";
  return ` · ${date.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  })}`;
}

function findSettledWorkRun(turn: CodingSessionTranscriptTurn) {
  if (!turn.completion) return null;
  const terminalAssistantIndex = turn.entries
    .map(
      (entry) =>
        entry.kind === "item" &&
        entry.item.type === "message" &&
        entry.item.role === "assistant",
    )
    .lastIndexOf(true);
  const hiddenIndexes = new Set<number>();
  turn.entries.forEach((entry, index) => {
    if (isFoldableWorkEntry(entry, index, terminalAssistantIndex)) {
      hiddenIndexes.add(index);
    }
  });
  const startIndex = hiddenIndexes.values().next().value ?? -1;
  if (startIndex < 0) return null;
  return {
    startIndex,
    hiddenIndexes,
    entries: turn.entries.filter((_, index) => hiddenIndexes.has(index)),
  };
}

function isFoldableWorkEntry(
  entry: CodingSessionTranscriptEntry,
  index: number,
  terminalAssistantIndex: number,
): boolean {
  if (entry.kind === "tool-group") return false;
  if (entry.item.type === "message") {
    return entry.item.role === "assistant" && index < terminalAssistantIndex;
  }
  return false;
}

function CodingSessionWorkedFold({
  completion,
  disclosureId,
  entries,
  generationId,
  onOpenChange,
  open,
  openDisclosures,
}: {
  completion: CodingSessionTranscriptTurn["completion"];
  disclosureId: string;
  entries: CodingSessionTranscriptEntry[];
  generationId: string;
  onOpenChange: (id: string, open: boolean) => void;
  open: boolean;
  openDisclosures: ReadonlySet<string>;
}) {
  const duration =
    completion?.durationMs !== null && completion?.durationMs !== undefined
      ? formatCodingSessionDuration(completion.durationMs)
      : null;

  return (
    <details
      className="group/worked"
      data-testid="coding-session-worked-fold"
      onToggle={(event) => onOpenChange(disclosureId, event.currentTarget.open)}
      open={open}
    >
      <summary className="flex min-h-7 cursor-pointer list-none items-center gap-2 text-sm text-muted-foreground transition-colors hover:text-foreground">
        <ChevronDown className="size-3.5 transition-transform group-open/worked:rotate-180" />
        <span className="font-medium">
          {duration ? `Worked for ${duration}` : "Worked"}
        </span>
      </summary>
      <div className="mt-1 ml-1 flex flex-col gap-1 border-l border-border/60 pl-4">
        {entries.map((entry) => (
          <CodingSessionEntry
            disclosureScope={disclosureId}
            entry={entry}
            generationId={generationId}
            key={entry.kind === "item" ? entry.item.id : entry.id}
            onDisclosureOpenChange={onOpenChange}
            openDisclosures={openDisclosures}
          />
        ))}
      </div>
    </details>
  );
}
