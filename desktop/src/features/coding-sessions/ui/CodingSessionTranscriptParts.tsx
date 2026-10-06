import * as React from "react";
import {
  Check,
  ChevronDown,
  Circle,
  CircleDashed,
  CircleDot,
  CircleHelp,
  Clock3,
  FileDiff,
  LoaderCircle,
  X,
} from "lucide-react";

import {
  FileEditDiffBlock,
  hasFileEditLineDiff,
} from "@/features/agents/ui/FileEditDiffView";
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  ACTIVITY_ROW_DETAIL_INSET_CLASS,
  ACTIVITY_ROW_ICON_CLASS,
  ACTIVITY_ROW_LABEL_CLASS,
  ACTIVITY_ROW_LINE_CLASS,
} from "@/features/agents/ui/AgentSessionToolItem/ToolItemRowClasses";
import {
  RevealedRedactionsMarker,
  RowRedactedText,
} from "@/features/agents/ui/AgentSessionToolItem/RowRedactedText";
import type {
  CodingSessionChangedFile,
  CodingSessionTurnSettlement,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import type {
  CodingSessionTask,
  CodingSessionTaskModel,
} from "@/features/coding-sessions/lib/codingSessionTaskModel";
import { cn } from "@/shared/lib/cn";
import { RedactedText } from "@/shared/ui/RedactedPill";
import { parseRedactionMarkers } from "@/shared/lib/redactionMarker";
import { useCodingSessionDisclosure } from "./CodingSessionTranscriptDisclosure";
import { CodingSessionLastTranscriptEventContext } from "./CodingSessionTranscriptWorking";
import {
  CodingSessionLiveShimmerText,
  useCodingSessionLiveShimmer,
} from "./CodingSessionTranscriptWorkingShimmer";

/**
 * The leaf presentation pieces of the coding-session transcript.
 *
 * Split out of `CodingSessionTranscript.tsx` purely for the file-size
 * discipline — the donor's single 992-line module was one import rename away
 * from the 1000-line ceiling, and the ceiling is never the thing that moves.
 *
 * Every disclosure here builds its body only while open: a closed row is a
 * summary line and nothing else, however much detail sits behind it.
 */

export function CodingSessionActiveTool({
  disclosureId,
  item,
  onOpenChange,
  open,
  settlement = "live",
}: {
  disclosureId: string;
  item: Extract<TranscriptItem, { type: "tool" }>;
  onOpenChange: (id: string, open: boolean) => void;
  open: boolean;
  /**
   * Where the turn this call belongs to stands. `settled`: the turn is over
   * while the call never reported an end, so it reads as a static, muted
   * "Did not finish" — never a spinner over work that stopped. `unknown`:
   * nothing says whether the turn is still going, so it reads "Status
   * unknown" — neither a spinner nor a verdict. `live` keeps the provider's
   * own "Running" or "Queued".
   */
  settlement?: CodingSessionTurnSettlement;
}) {
  const label = formatActiveToolLabel(item);
  const unfinished = settlement === "settled";
  const statusUnknown = settlement === "unknown";
  const statusLabel = unfinished
    ? "Did not finish"
    : statusUnknown
      ? "Status unknown"
      : item.status === "pending"
        ? "Queued"
        : "Running";
  const hasDetails =
    Object.keys(item.args).length > 0 || item.result.trim().length > 0;
  // SV-104: a call the provider still runs, in a live turn, shimmers while
  // the provider is fresh. A label carrying a redaction marker never does:
  // the shimmer's overlay copies the text, and a marker is not text.
  const lastEventAt = React.useContext(CodingSessionLastTranscriptEventContext);
  const shimmer = useCodingSessionLiveShimmer(
    settlement === "live" &&
      item.status === "executing" &&
      isPlainCodingSessionToolLabel(label),
    lastEventAt,
  );
  const summary = (
    <>
      {unfinished ? (
        <CircleDashed
          aria-hidden
          className={cn(ACTIVITY_ROW_ICON_CLASS, "opacity-70")}
        />
      ) : statusUnknown ? (
        <CircleHelp
          aria-hidden
          className={cn(ACTIVITY_ROW_ICON_CLASS, "opacity-70")}
        />
      ) : item.status === "pending" ? (
        <Clock3 className={ACTIVITY_ROW_ICON_CLASS} />
      ) : (
        <LoaderCircle
          className={cn(
            ACTIVITY_ROW_ICON_CLASS,
            "animate-spin motion-reduce:animate-none",
          )}
        />
      )}
      <span className={ACTIVITY_ROW_LABEL_CLASS}>
        {shimmer ? (
          <CodingSessionLiveShimmerText active text={label} />
        ) : (
          <RowRedactedText text={label} />
        )}
      </span>
      <RevealedRedactionsMarker texts={[label]} />
      <span className="ml-auto shrink-0 text-xs text-muted-foreground">
        {statusLabel}
      </span>
      {hasDetails ? (
        <ChevronDown className="size-3.5 shrink-0 transition-transform group-open/active-tool:rotate-180" />
      ) : null}
    </>
  );

  if (!hasDetails) {
    return (
      <div
        className="flex min-h-7 w-full items-center gap-1.5 px-0.5 text-sm text-muted-foreground"
        data-testid="coding-session-active-tool"
        data-tool-status={item.status}
        data-tool-status-unknown={statusUnknown ? "" : undefined}
        data-tool-unfinished={unfinished ? "" : undefined}
        role={settlement === "live" ? "status" : undefined}
      >
        {summary}
      </div>
    );
  }

  return (
    <details
      className="group/active-tool text-sm text-muted-foreground"
      data-testid="coding-session-active-tool"
      data-tool-status={item.status}
      data-tool-status-unknown={statusUnknown ? "" : undefined}
      data-tool-unfinished={unfinished ? "" : undefined}
      onToggle={(event) => onOpenChange(disclosureId, event.currentTarget.open)}
      open={open}
    >
      <summary
        className={cn("cursor-pointer list-none", ACTIVITY_ROW_LINE_CLASS)}
      >
        {summary}
      </summary>
      {open ? (
        <div className={cn(ACTIVITY_ROW_DETAIL_INSET_CLASS, "mt-1 min-w-0")}>
          {Object.keys(item.args).length > 0 ? (
            <pre className="buzz-code-scrollbar max-h-48 min-w-0 max-w-full overflow-auto whitespace-pre-wrap wrap-anywhere rounded-md bg-muted/50 p-2 text-xs">
              <RedactedText text={safeFormatToolArgs(item.args)} />
            </pre>
          ) : null}
          {item.result.trim() ? (
            <pre className="buzz-code-scrollbar mt-2 max-h-48 min-w-0 max-w-full overflow-auto whitespace-pre-wrap wrap-anywhere rounded-md bg-muted/50 p-2 text-xs">
              <RedactedText text={item.result} />
            </pre>
          ) : null}
        </div>
      ) : null}
    </details>
  );
}

export function CodingSessionInlinePlan({
  disclosureId,
  model,
  onOpenChange,
  open,
}: {
  disclosureId: string;
  model: CodingSessionTaskModel;
  onOpenChange: (id: string, open: boolean) => void;
  open: boolean;
}) {
  const currentTask =
    model.tasks.find((task) => task.status === "in_progress") ??
    model.tasks.find((task) => task.status !== "completed") ??
    model.tasks.at(-1);
  const label =
    model.state === "empty" ? "Plan cleared" : (currentTask?.text ?? "Plan");

  return (
    <details
      className="group/plan text-sm"
      data-plan-state={model.state}
      data-testid="coding-session-inline-plan"
      onToggle={(event) => onOpenChange(disclosureId, event.currentTarget.open)}
      open={open}
    >
      <summary
        className={cn(
          "cursor-pointer list-none text-muted-foreground",
          ACTIVITY_ROW_LINE_CLASS,
        )}
      >
        <ChevronDown
          className={cn(
            ACTIVITY_ROW_ICON_CLASS,
            "-rotate-90 transition-transform group-open/plan:rotate-0",
          )}
        />
        {model.tasks.length > 1 ? (
          <span
            aria-hidden
            className="flex max-w-28 shrink-0 items-center gap-0.5"
          >
            {model.tasks.slice(0, 8).map((task) => (
              <span
                className={cn(
                  "h-1 min-w-2 flex-1 rounded-full",
                  task.status === "completed"
                    ? "bg-emerald-500"
                    : task.status === "in_progress"
                      ? "bg-primary"
                      : task.status === "blocked" || task.status === "failed"
                        ? "bg-destructive"
                        : "bg-muted-foreground/25",
                )}
                key={task.id}
              />
            ))}
          </span>
        ) : null}
        <span
          className={cn(
            ACTIVITY_ROW_LABEL_CLASS,
            model.state === "complete" && "text-muted-foreground/70",
          )}
        >
          {label}
        </span>
        {model.tasks.length > 1 ? (
          <span className="ml-auto shrink-0 text-xs tabular-nums text-muted-foreground/70">
            {model.completedCount}/{model.tasks.length}
          </span>
        ) : null}
      </summary>
      {open && model.tasks.length > 0 ? (
        <div className="mt-1 ml-3 flex flex-col gap-0.5 border-l border-border/60 pl-4 text-xs">
          {model.explanation ? (
            <p className="mb-1 text-muted-foreground">{model.explanation}</p>
          ) : null}
          <ol aria-label="Plan steps" className="flex flex-col gap-0.5">
            {model.tasks.map((task) => (
              <CodingSessionInlinePlanStep key={task.id} task={task} />
            ))}
          </ol>
        </div>
      ) : null}
    </details>
  );
}

function CodingSessionInlinePlanStep({ task }: { task: CodingSessionTask }) {
  const Icon =
    task.status === "completed"
      ? Check
      : task.status === "in_progress"
        ? CircleDot
        : task.status === "failed" || task.status === "blocked"
          ? X
          : Circle;
  return (
    <li
      className={cn(
        "flex min-h-6 items-start gap-2 py-0.5",
        task.status === "completed" && "text-muted-foreground/65",
        (task.status === "failed" || task.status === "blocked") &&
          "text-destructive",
      )}
      data-plan-step-status={task.status}
    >
      <Icon className="mt-0.5 size-3.5 shrink-0" />
      <span className="min-w-0 wrap-break-word">{task.text}</span>
    </li>
  );
}

export function CodingSessionChangedFilesCard({
  disclosureId,
  files,
  onOpenChange,
  open,
}: {
  disclosureId: string;
  files: CodingSessionChangedFile[];
  onOpenChange: (id: string, open: boolean) => void;
  open: boolean;
}) {
  if (files.length === 0) return null;

  const completeStats = files.every(
    (file) => file.additions !== null && file.deletions !== null,
  );
  const additions = completeStats
    ? files.reduce((total, file) => total + (file.additions ?? 0), 0)
    : null;
  const deletions = completeStats
    ? files.reduce((total, file) => total + (file.deletions ?? 0), 0)
    : null;

  return (
    <details
      className="group/changed-files mt-1 rounded-2xl bg-muted/20 p-2"
      data-testid="coding-session-changed-files"
      onToggle={(event) => onOpenChange(disclosureId, event.currentTarget.open)}
      open={open}
    >
      <summary className="flex min-h-8 cursor-pointer list-none items-center gap-2 rounded-xl px-2 text-xs transition-colors hover:bg-muted/40">
        <ChevronDown className="size-3.5 shrink-0 text-muted-foreground transition-transform group-open/changed-files:rotate-180" />
        <span className="font-medium">
          {files.length} changed {files.length === 1 ? "file" : "files"}
        </span>
        {additions !== null && deletions !== null ? (
          <CodingSessionDiffStats additions={additions} deletions={deletions} />
        ) : null}
      </summary>
      {open ? (
        <div className="mt-1 flex flex-col gap-1">
          {files.map((file) => (
            <CodingSessionChangedFileRow
              disclosureId={`${disclosureId}:${file.path}`}
              file={file}
              key={file.path}
            />
          ))}
        </div>
      ) : null}
    </details>
  );
}

function CodingSessionChangedFileRow({
  disclosureId,
  file,
}: {
  disclosureId: string;
  file: CodingSessionChangedFile;
}) {
  const [open, setOpen] = useCodingSessionDisclosure(disclosureId);
  const inlineDiffs = file.diffs.filter(hasFileEditLineDiff);
  const label = (
    <>
      <FileDiff className="size-3.5 shrink-0 text-muted-foreground" />
      <span
        className="min-w-0 flex-1 truncate font-mono text-xs"
        title={file.path}
      >
        {file.path}
      </span>
      {file.additions !== null && file.deletions !== null ? (
        <CodingSessionDiffStats
          additions={file.additions}
          deletions={file.deletions}
        />
      ) : null}
    </>
  );

  if (inlineDiffs.length === 0) {
    return (
      <div
        className="flex min-h-7 items-center gap-2 rounded-lg px-2"
        data-testid="coding-session-changed-file"
      >
        {label}
      </div>
    );
  }

  return (
    <details
      className="group/changed-file rounded-lg"
      data-testid="coding-session-changed-file"
      onToggle={(event) => setOpen(event.currentTarget.open)}
      open={open}
    >
      <summary className="flex min-h-7 cursor-pointer list-none items-center gap-2 rounded-lg px-2 hover:bg-muted/30">
        {label}
        <ChevronDown className="size-3.5 shrink-0 text-muted-foreground transition-transform group-open/changed-file:rotate-180" />
      </summary>
      {open ? (
        <div className="mt-1 flex min-w-0 flex-col gap-2 overflow-hidden rounded-lg border border-border/50 bg-background/50">
          {inlineDiffs.map((diff) => (
            <FileEditDiffBlock diff={diff} key={diff.id} />
          ))}
        </div>
      ) : null}
    </details>
  );
}

export function CodingSessionDiffStats({
  additions,
  deletions,
}: {
  additions: number;
  deletions: number;
}) {
  return (
    <span className="inline-flex shrink-0 items-center gap-1 font-mono text-2xs">
      <span className="text-emerald-600 dark:text-emerald-400">
        +{additions}
      </span>
      <span className="text-rose-600 dark:text-rose-400">-{deletions}</span>
    </span>
  );
}

export const CodingSessionDiagnostics = React.memo(
  function CodingSessionDiagnostics({
    diagnostics,
    disclosureId,
    label,
    onOpenChange,
    open,
  }: {
    diagnostics: TranscriptItem[];
    disclosureId: string;
    label: string;
    onOpenChange: (id: string, open: boolean) => void;
    open: boolean;
  }) {
    if (diagnostics.length === 0) return null;

    return (
      <details
        className="group/diagnostics text-xs text-muted-foreground"
        data-testid="coding-session-diagnostics"
        onToggle={(event) =>
          onOpenChange(disclosureId, event.currentTarget.open)
        }
        open={open}
      >
        <summary className="flex w-fit cursor-pointer list-none items-center gap-1.5 py-1 transition-colors hover:text-foreground">
          <ChevronDown className="size-3 transition-transform group-open/diagnostics:rotate-180" />
          <span>
            {label} · {diagnostics.length}{" "}
            {diagnostics.length === 1 ? "event" : "events"}
          </span>
        </summary>
        {open ? (
          <CodingSessionDiagnosticRows diagnostics={diagnostics} />
        ) : null}
      </details>
    );
  },
);

/** The rows behind a diagnostics disclosure; rendered only while it is open. */
export function CodingSessionDiagnosticRows({
  diagnostics,
}: {
  diagnostics: TranscriptItem[];
}) {
  return (
    <div className="mt-1 ml-1 flex flex-col gap-2 border-l border-border/60 pl-3">
      {diagnostics.map((item) => (
        <div data-testid="coding-session-diagnostic-row" key={item.id}>
          <p className="font-medium text-foreground/75">
            <RedactedText text={item.title} />
          </p>
          {"text" in item && item.text ? (
            <p className="mt-0.5 line-clamp-3 whitespace-pre-wrap">
              <RedactedText text={item.text} />
            </p>
          ) : null}
        </div>
      ))}
    </div>
  );
}

/** True when `label` holds no redaction marker: plain text the shimmer may copy. */
export function isPlainCodingSessionToolLabel(label: string): boolean {
  const segments = parseRedactionMarkers(label);
  return segments.every((segment) => segment.kind === "text");
}

function formatActiveToolLabel(
  item: Extract<TranscriptItem, { type: "tool" }>,
): string {
  const action = item.descriptor.action;
  if (action) {
    return [toActiveToolVerb(action.verb), action.object]
      .filter(Boolean)
      .join(" ");
  }
  return item.descriptor.preview
    ? `${item.descriptor.label} · ${item.descriptor.preview}`
    : item.descriptor.label;
}

function toActiveToolVerb(verb: string): string {
  const activeVerbs: Record<string, string> = {
    Added: "Add",
    Archived: "Archive",
    Captured: "Capture",
    Checked: "Check",
    Compacted: "Compact",
    Created: "Create",
    Deleted: "Delete",
    Edited: "Edit",
    Ran: "Run",
    Read: "Read",
    Removed: "Remove",
    Searched: "Search",
    Sent: "Send",
    Unarchived: "Unarchive",
    Updated: "Update",
    Viewed: "View",
  };
  return activeVerbs[verb] ?? verb;
}

function safeFormatToolArgs(args: Record<string, unknown>): string {
  try {
    return JSON.stringify(args, null, 2);
  } catch {
    return "Tool input could not be formatted.";
  }
}
