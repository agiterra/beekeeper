import * as React from "react";
import {
  Check,
  ChevronDown,
  Circle,
  CircleDot,
  CircleStop,
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
  formatCodingSessionCompletionOutcome,
  formatCodingSessionCost,
  formatCodingSessionDuration,
  type CodingSessionChangedFile,
  type CodingSessionTranscriptTurn,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import type {
  CodingSessionTask,
  CodingSessionTaskModel,
} from "@/features/coding-sessions/lib/codingSessionTaskModel";
import { cn } from "@/shared/lib/cn";

/**
 * The leaf presentation pieces of the coding-session transcript.
 *
 * Split out of `CodingSessionTranscript.tsx` purely for the file-size
 * discipline — the donor's single 992-line module was one import rename away
 * from the 1000-line ceiling, and the ceiling is never the thing that moves.
 */

export function CodingSessionActiveTool({
  disclosureId,
  item,
  onOpenChange,
  open,
}: {
  disclosureId: string;
  item: Extract<TranscriptItem, { type: "tool" }>;
  onOpenChange: (id: string, open: boolean) => void;
  open: boolean;
}) {
  const label = formatActiveToolLabel(item);
  const statusLabel = item.status === "pending" ? "Queued" : "Running";
  const hasDetails =
    Object.keys(item.args).length > 0 || item.result.trim().length > 0;
  const summary = (
    <>
      {item.status === "pending" ? (
        <Clock3 className="size-3.5 shrink-0" />
      ) : (
        <LoaderCircle className="size-3.5 shrink-0 animate-spin motion-reduce:animate-none" />
      )}
      <span className="min-w-0 truncate font-medium text-foreground/85">
        {label}
      </span>
      <span className="ml-auto shrink-0 text-xs">{statusLabel}</span>
      {hasDetails ? (
        <ChevronDown className="size-3.5 shrink-0 transition-transform group-open/active-tool:rotate-180" />
      ) : null}
    </>
  );

  if (!hasDetails) {
    return (
      <div
        className="flex min-h-7 items-center gap-2 px-0.5 text-sm text-muted-foreground"
        data-testid="coding-session-active-tool"
        data-tool-status={item.status}
        role="status"
      >
        {summary}
      </div>
    );
  }

  return (
    <details
      className="group/active-tool px-0.5 text-sm text-muted-foreground"
      data-testid="coding-session-active-tool"
      data-tool-status={item.status}
      onToggle={(event) => onOpenChange(disclosureId, event.currentTarget.open)}
      open={open}
    >
      <summary className="flex min-h-7 cursor-pointer list-none items-center gap-2">
        {summary}
      </summary>
      <div className="mt-1 ml-1 min-w-0 border-l border-border/60 pl-4">
        {Object.keys(item.args).length > 0 ? (
          <pre className="buzz-code-scrollbar max-h-48 min-w-0 max-w-full overflow-auto whitespace-pre-wrap wrap-anywhere rounded-md bg-muted/50 p-2 text-xs">
            {safeFormatToolArgs(item.args)}
          </pre>
        ) : null}
        {item.result.trim() ? (
          <pre className="buzz-code-scrollbar mt-2 max-h-48 min-w-0 max-w-full overflow-auto whitespace-pre-wrap wrap-anywhere rounded-md bg-muted/50 p-2 text-xs">
            {item.result}
          </pre>
        ) : null}
      </div>
    </details>
  );
}

export function CodingSessionWorking({
  startedAt,
}: {
  startedAt: string | null;
}) {
  const elapsed = useLiveCodingSessionDuration(startedAt);
  return (
    <div
      className="flex items-center gap-2 py-1 text-sm text-muted-foreground"
      data-testid="coding-session-working"
      role="status"
    >
      <span aria-hidden className="inline-flex items-center gap-[3px]">
        <span className="size-1 rounded-full bg-muted-foreground/40 animate-pulse" />
        <span className="size-1 rounded-full bg-muted-foreground/40 animate-pulse [animation-delay:200ms]" />
        <span className="size-1 rounded-full bg-muted-foreground/40 animate-pulse [animation-delay:400ms]" />
      </span>
      <span>{elapsed ? `Working for ${elapsed}` : "Working…"}</span>
    </div>
  );
}

function useLiveCodingSessionDuration(startedAt: string | null): string | null {
  const [now, setNow] = React.useState(() => Date.now());

  React.useEffect(() => {
    if (!startedAt) return;
    const interval = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(interval);
  }, [startedAt]);

  if (!startedAt) return null;
  const start = Date.parse(startedAt);
  if (!Number.isFinite(start)) return null;
  return formatCodingSessionDuration(Math.max(0, now - start));
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
      className="group/plan text-xs"
      data-plan-state={model.state}
      data-testid="coding-session-inline-plan"
      onToggle={(event) => onOpenChange(disclosureId, event.currentTarget.open)}
      open={open}
    >
      <summary className="flex min-h-7 cursor-pointer list-none items-center gap-2 rounded-md px-0.5 text-muted-foreground transition-colors hover:bg-muted/30 hover:text-foreground">
        <ChevronDown className="size-3.5 shrink-0 -rotate-90 transition-transform group-open/plan:rotate-0" />
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
            "min-w-0 truncate",
            model.state === "complete"
              ? "text-muted-foreground/70"
              : "font-medium text-foreground/85",
          )}
        >
          {label}
        </span>
        {model.tasks.length > 1 ? (
          <span className="ml-auto shrink-0 tabular-nums text-muted-foreground/70">
            {model.completedCount}/{model.tasks.length}
          </span>
        ) : null}
      </summary>
      {open && model.tasks.length > 0 ? (
        <div className="mt-1 ml-1 flex flex-col gap-0.5 border-l border-border/60 pl-4">
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
      <div className="mt-1 flex flex-col gap-1">
        {files.map((file) => (
          <CodingSessionChangedFileRow file={file} key={file.path} />
        ))}
      </div>
    </details>
  );
}

function CodingSessionChangedFileRow({
  file,
}: {
  file: CodingSessionChangedFile;
}) {
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
    >
      <summary className="flex min-h-7 cursor-pointer list-none items-center gap-2 rounded-lg px-2 hover:bg-muted/30">
        {label}
        <ChevronDown className="size-3.5 shrink-0 text-muted-foreground transition-transform group-open/changed-file:rotate-180" />
      </summary>
      <div className="mt-1 flex min-w-0 flex-col gap-2 overflow-hidden rounded-lg border border-border/50 bg-background/50">
        {inlineDiffs.map((diff) => (
          <FileEditDiffBlock diff={diff} key={diff.id} />
        ))}
      </div>
    </details>
  );
}

function CodingSessionDiffStats({
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

export function CodingSessionTurnCompletion({
  completion,
  durationShownInWorkFold,
}: {
  completion: NonNullable<CodingSessionTranscriptTurn["completion"]>;
  durationShownInWorkFold: boolean;
}) {
  const workedDuration =
    completion.state === "completed" &&
    completion.durationMs !== null &&
    !durationShownInWorkFold
      ? formatCodingSessionDuration(completion.durationMs)
      : null;
  const details = [
    completion.durationMs !== null &&
    !durationShownInWorkFold &&
    workedDuration === null
      ? formatCodingSessionDuration(completion.durationMs)
      : null,
    formatCodingSessionCompletionOutcome(completion),
    completion.costUsd !== null
      ? formatCodingSessionCost(completion.costUsd)
      : null,
  ].filter((value): value is string => value !== null);

  return (
    <div
      className={cn(
        "flex items-center gap-1.5 pt-0.5 text-xs text-muted-foreground",
        completion.state === "interrupted" &&
          "text-amber-600 dark:text-amber-400",
        completion.state === "failed" && "text-destructive",
      )}
      data-testid="coding-session-turn-completion"
      data-turn-state={completion.state}
    >
      {completion.state === "interrupted" ? (
        <CircleStop className="size-3.5" />
      ) : completion.state === "failed" ? (
        <X className="size-3.5" />
      ) : (
        <Check className="size-3.5" />
      )}
      <span>
        {completion.state === "interrupted"
          ? "Stopped"
          : completion.state === "failed"
            ? "Failed"
            : workedDuration
              ? `Worked for ${workedDuration}`
              : "Completed"}
      </span>
      {details.length > 0 ? <span>· {details.join(" · ")}</span> : null}
    </div>
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
        <div className="mt-1 ml-1 flex flex-col gap-2 border-l border-border/60 pl-3">
          {diagnostics.map((item) => (
            <div data-testid="coding-session-diagnostic-row" key={item.id}>
              <p className="font-medium text-foreground/75">{item.title}</p>
              {"text" in item && item.text ? (
                <p className="mt-0.5 line-clamp-3 whitespace-pre-wrap">
                  {item.text}
                </p>
              ) : null}
            </div>
          ))}
        </div>
      </details>
    );
  },
);

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
