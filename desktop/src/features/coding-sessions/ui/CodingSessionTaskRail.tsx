import {
  Ban,
  Check,
  ChevronRight,
  Circle,
  CircleHelp,
  ClipboardCopy,
  ListChecks,
  LoaderCircle,
  OctagonAlert,
  TriangleAlert,
  X,
} from "lucide-react";

import * as React from "react";

import type {
  CodingSessionTask,
  CodingSessionTaskModel,
  CodingSessionTaskStatus,
} from "@/features/coding-sessions/lib/codingSessionTaskModel";
import { formatCodingSessionDuration } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { cn } from "@/shared/lib/cn";
import { copyTextToClipboard } from "@/shared/lib/clipboard";
import { Sheet, SheetContent, SheetTitle } from "@/shared/ui/sheet";

export const CODING_SESSION_TASK_RAIL_ID = "coding-session-task-rail";

type TaskRailLoadState = "ready" | "loading" | "error";

export function CodingSessionTaskRail({
  loadState = "ready",
  model,
  onClose,
  variant = "inline",
}: {
  loadState?: TaskRailLoadState;
  model: CodingSessionTaskModel | null;
  onClose?: () => void;
  /**
   * `surface` is the Plan surface's panel (SV-24): full width and height
   * inside the surface host, which owns the landmark and the tab chrome, so
   * it renders no `aside` of its own and opens on the 40px subheader every
   * surface shares. The dock above the composer is unchanged.
   */
  variant?: "dock" | "inline" | "sheet" | "surface";
}) {
  if (variant === "dock") {
    return (
      <CodingSessionTaskDock
        loadState={loadState}
        model={model}
        onClose={onClose}
      />
    );
  }

  const completedTasks =
    model?.tasks.filter((task) => task.status === "completed") ?? [];
  const openTasks =
    model?.tasks.filter((task) => task.status !== "completed") ?? [];

  const Container = variant === "surface" ? "div" : "aside";
  return (
    <Container
      aria-label={variant === "surface" ? undefined : "Session plan"}
      className={cn(
        "flex min-h-0 shrink-0 flex-col bg-muted/10",
        variant === "inline"
          ? "w-80 border-l border-border/60"
          : variant === "surface"
            ? "h-full w-full flex-1"
            : "h-full w-full",
      )}
      data-variant={variant}
      data-testid="coding-session-task-rail"
      id={variant === "surface" ? undefined : CODING_SESSION_TASK_RAIL_ID}
    >
      <div
        className={cn(
          "flex shrink-0 items-center justify-between border-b border-border/60",
          variant === "surface"
            ? "h-10 min-h-10 bg-background px-3"
            : "h-14 px-4",
          variant === "sheet" && "pr-14",
        )}
        data-surface-subheader={variant === "surface" ? "" : undefined}
      >
        <div className="flex min-w-0 items-center gap-2">
          <ListChecks
            aria-hidden
            className="h-4 w-4 shrink-0 text-muted-foreground"
          />
          <h2 className="truncate text-sm font-semibold">Plan</h2>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          {model?.copyText ? (
            <button
              aria-label="Copy session plan"
              className="inline-flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              onClick={() =>
                copyTextToClipboard(model.copyText ?? "", "Session plan copied")
              }
              title="Copy plan"
              type="button"
            >
              <ClipboardCopy aria-hidden className="h-3.5 w-3.5" />
            </button>
          ) : null}
          {model && model.tasks.length > 0 ? (
            <>
              <span
                aria-hidden
                className="rounded-full border border-border/60 bg-background/50 px-2 py-0.5 text-2xs font-medium tabular-nums text-muted-foreground"
              >
                {model.completedCount}/{model.tasks.length}
              </span>
              <span className="sr-only">
                {model.completedCount} of {model.tasks.length} tasks complete
              </span>
            </>
          ) : null}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
        {loadState === "loading" ? (
          <TaskRailLoadingState />
        ) : loadState === "error" ? (
          <TaskRailEmptyState
            description="Beekeeper could not read the latest signed plan. The session transcript is still available."
            icon="error"
            title="Plan unavailable"
          />
        ) : !model ? (
          <TaskRailEmptyState
            description="Plan and todo updates from this signed session will appear here."
            title="No plan published"
          />
        ) : model.state === "empty" ? (
          <TaskRailEmptyState
            description="The latest signed plan intentionally contains no tasks."
            title="Plan is empty"
          />
        ) : (
          <>
            <TaskRailStatusHeader model={model} />
            <div className="px-3 pb-4">
              {openTasks.length > 0 ? (
                <ol aria-label="Open session tasks" className="space-y-1.5">
                  {openTasks.map((task) => (
                    <CodingSessionTaskRow key={task.id} task={task} />
                  ))}
                </ol>
              ) : null}

              {completedTasks.length > 0 ? (
                <CompletedTaskDisclosure
                  className={cn(openTasks.length > 0 && "mt-3")}
                  tasks={completedTasks}
                />
              ) : null}
            </div>
          </>
        )}
      </div>

      {model && loadState === "ready" ? (
        <div className="flex shrink-0 items-center justify-between gap-3 border-t border-border/60 px-4 py-3 text-2xs text-muted-foreground">
          <span>
            {model.state === "complete"
              ? "All tasks complete"
              : model.state === "empty"
                ? "Latest signed plan"
                : "Live from signed session"}
          </span>
          <time dateTime={model.timestamp}>
            Updated {formatTaskTimestamp(model.timestamp)}
          </time>
        </div>
      ) : null}
    </Container>
  );
}

/**
 * The plan's one persistent place: a rail docked above the composer.
 *
 * SESSION_VIEW_UX_PLAN L4. It used to open fully on every turn that published
 * a plan — up to 28 rem of task list pushed over the transcript the person was
 * reading, with the same plan also behind a header button and in the
 * transcript. Now it mounts as one line — progress and the task in hand — and
 * opens only when asked. Nothing is withheld: the line names the count and
 * the current task, and one click shows every task with its state.
 */
function CodingSessionTaskDock({
  loadState,
  model,
  onClose,
}: {
  loadState: TaskRailLoadState;
  model: CodingSessionTaskModel | null;
  onClose?: () => void;
}) {
  const [expanded, setExpanded] = React.useState(false);
  const listId = React.useId();
  const summary = codingSessionTaskDockSummary({ loadState, model });
  return (
    <aside
      aria-label="Session tasks"
      className={cn(
        "rounded-t-2xl border border-border/70 bg-background px-3 pt-1 shadow-sm",
        // The composer overlaps the rail's bottom edge by the wrapper's
        // negative margin; this padding keeps the last line clear of it.
        expanded ? "max-h-[min(48vh,28rem)] overflow-y-auto pb-8" : "pb-7",
      )}
      data-expanded={expanded ? "true" : "false"}
      data-testid="coding-session-task-dock"
      data-variant="dock"
      id={CODING_SESSION_TASK_RAIL_ID}
    >
      <div className="flex h-8 min-w-0 items-center gap-1">
        <button
          aria-controls={listId}
          aria-expanded={expanded}
          aria-label={
            expanded ? "Collapse session tasks" : "Expand session tasks"
          }
          className="flex h-7 min-w-0 flex-1 items-center gap-2 rounded-md px-1.5 text-left text-sm transition-colors hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          data-testid="coding-session-task-dock-toggle"
          onClick={() => setExpanded((value) => !value)}
          type="button"
        >
          <ChevronRight
            aria-hidden
            className={cn(
              "size-3.5 shrink-0 text-muted-foreground transition-transform",
              expanded && "rotate-90",
            )}
          />
          <ListChecks
            aria-hidden
            className="size-3.5 shrink-0 text-muted-foreground"
          />
          <span className="shrink-0 font-semibold">Tasks</span>
          {model && model.tasks.length > 0 ? (
            <span className="shrink-0 tabular-nums text-muted-foreground">
              {model.completedCount}/{model.tasks.length}
            </span>
          ) : null}
          {summary ? (
            <span
              className="min-w-0 truncate text-muted-foreground"
              data-testid="coding-session-task-dock-summary"
            >
              {summary}
            </span>
          ) : null}
        </button>
        {onClose ? (
          <button
            aria-label="Close session tasks"
            className="inline-flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onClick={onClose}
            title="Hide the plan for this turn; the header's Plan control brings it back"
            type="button"
          >
            <X aria-hidden className="size-3.5" />
          </button>
        ) : null}
      </div>
      {expanded ? (
        <div className="pt-1" id={listId}>
          {loadState === "loading" ? (
            <TaskRailLoadingState />
          ) : loadState === "error" ? (
            <TaskRailEmptyState
              description="Beekeeper could not read the latest signed plan."
              icon="error"
              title="Tasks unavailable"
            />
          ) : !model ? (
            <TaskRailEmptyState
              description="Signed plan updates will appear here."
              title="No tasks yet"
            />
          ) : model.tasks.length === 0 ? (
            <TaskRailEmptyState
              description="The latest signed plan contains no tasks."
              title="No tasks"
            />
          ) : (
            <ol aria-label="Session tasks" className="space-y-0.5">
              {model.tasks.map((task) => (
                <CodingSessionTaskDockRow key={task.id} task={task} />
              ))}
            </ol>
          )}
        </div>
      ) : null}
    </aside>
  );
}

/**
 * The collapsed dock's one line after the count: the first failed task, else
 * the first blocked one, else the task in hand, else the honest state of the
 * plan. Never a
 * guess — a plan nobody published says so.
 */
export function codingSessionTaskDockSummary({
  loadState,
  model,
}: {
  loadState: TaskRailLoadState;
  model: CodingSessionTaskModel | null;
}): string | null {
  if (loadState === "loading") return "Loading plan…";
  if (loadState === "error") return "Plan unavailable";
  if (!model) return "No tasks yet";
  if (model.tasks.length === 0) return "No tasks";
  // A failure leads, then a block, then the task in hand: a one-line summary
  // that showed "working on X" over a failed task would be the comfortable
  // guess rather than the truth.
  const current =
    model.tasks.find((task) => task.status === "failed") ??
    model.tasks.find((task) => task.status === "blocked") ??
    model.tasks.find((task) => task.status === "in_progress");
  if (current) {
    return current.status === "in_progress"
      ? current.text
      : `${statusLabel(current.status)}: ${current.text}`;
  }
  if (model.completedCount === model.tasks.length) return "All complete";
  const next = model.tasks.find((task) => task.status === "pending");
  return next ? `Next: ${next.text}` : null;
}

function CodingSessionTaskDockRow({ task }: { task: CodingSessionTask }) {
  const Icon = taskStatusIcon(task.status);
  const active = task.status === "in_progress";
  return (
    <li
      className={cn(
        "flex min-h-9 items-start gap-3 rounded-lg px-1.5 py-1.5 text-sm",
        active ? "text-foreground" : "text-muted-foreground/65",
      )}
      data-status={task.status}
    >
      <Icon
        aria-label={statusLabel(task.status)}
        className={cn(
          "mt-0.5 size-3.5 shrink-0",
          active && "text-primary",
          task.status === "completed" && "text-emerald-500/70",
        )}
      />
      <span
        className={cn(
          "min-w-0 flex-1 wrap-break-word",
          active && "font-medium",
        )}
      >
        {task.text}
      </span>
      {active ? (
        <span className="shrink-0 text-2xs text-muted-foreground/60">now</span>
      ) : task.status === "completed" && task.elapsedMs !== undefined ? (
        <span className="shrink-0 text-2xs tabular-nums text-muted-foreground/55">
          {formatCodingSessionDuration(task.elapsedMs)}
        </span>
      ) : null}
    </li>
  );
}

function TaskRailStatusHeader({ model }: { model: CodingSessionTaskModel }) {
  const inProgressCount = countStatus(model.tasks, "in_progress");
  const attentionCount =
    countStatus(model.tasks, "blocked") + countStatus(model.tasks, "failed");
  const progress = Math.round(
    (model.completedCount / Math.max(1, model.tasks.length)) * 100,
  );
  const status =
    model.state === "complete"
      ? "Complete"
      : attentionCount > 0
        ? "Needs attention"
        : inProgressCount > 0
          ? "In progress"
          : "Queued";

  return (
    <section
      aria-label="Plan status"
      className="border-b border-border/50 px-4 py-4"
    >
      <div className="flex items-center justify-between gap-3">
        <div>
          <p className="text-3xs font-semibold tracking-[0.12em] text-muted-foreground uppercase">
            Tasks
          </p>
          <p className="mt-1 text-sm font-semibold text-foreground">{status}</p>
        </div>
        <span className="text-xs font-medium tabular-nums text-muted-foreground">
          {progress}%
        </span>
      </div>
      <div
        aria-label={`${progress}% complete`}
        aria-valuemax={100}
        aria-valuemin={0}
        aria-valuenow={progress}
        className="mt-3 h-1.5 overflow-hidden rounded-full bg-muted"
        role="progressbar"
      >
        <div
          className="h-full rounded-full bg-primary transition-[width] duration-300"
          style={{ width: `${progress}%` }}
        />
      </div>
      {model.explanation ? (
        <p className="mt-3 wrap-break-word text-xs leading-5 text-muted-foreground">
          {model.explanation}
        </p>
      ) : null}
    </section>
  );
}

function CompletedTaskDisclosure({
  className,
  tasks,
}: {
  className?: string;
  tasks: CodingSessionTask[];
}) {
  return (
    <details className={cn("group/completed", className)}>
      <summary className="flex cursor-pointer list-none items-center gap-2 rounded-md px-2 py-1.5 text-xs font-medium text-muted-foreground hover:bg-muted/50 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring [&::-webkit-details-marker]:hidden">
        <ChevronRight
          aria-hidden
          className="h-3.5 w-3.5 transition-transform group-open/completed:rotate-90"
        />
        <span>Completed</span>
        <span className="ml-auto tabular-nums">{tasks.length}</span>
      </summary>
      <ol aria-label="Completed session tasks" className="mt-1 space-y-1">
        {tasks.map((task) => (
          <CodingSessionTaskRow key={task.id} task={task} />
        ))}
      </ol>
    </details>
  );
}

function CodingSessionTaskRow({ task }: { task: CodingSessionTask }) {
  const Icon = taskStatusIcon(task.status);

  return (
    <li
      className={cn(
        "group/task flex min-w-0 items-start gap-2.5 rounded-lg border border-transparent px-2 py-2 text-sm transition-colors",
        task.status === "in_progress" &&
          "border-primary/20 bg-primary/8 text-foreground shadow-xs",
        task.status === "blocked" &&
          "border-amber-500/20 bg-amber-500/8 text-foreground",
        task.status === "failed" &&
          "border-destructive/20 bg-destructive/8 text-foreground",
        task.status === "completed" && "text-muted-foreground",
        task.status === "cancelled" && "text-muted-foreground/80",
      )}
      data-status={task.status}
      data-task-id={task.id}
    >
      <Icon
        aria-label={statusLabel(task.status)}
        className={cn(
          "mt-0.5 h-4 w-4 shrink-0",
          task.status === "in_progress" && "animate-spin text-primary",
          task.status === "completed" && "text-emerald-500",
          task.status === "pending" && "text-muted-foreground/60",
          task.status === "blocked" && "text-amber-500",
          task.status === "failed" && "text-destructive",
          task.status === "cancelled" && "text-muted-foreground/70",
          task.status === "unknown" && "text-amber-500",
        )}
      />
      <span
        className={cn(
          "min-w-0 flex-1 wrap-break-word leading-5",
          task.status === "in_progress" && "font-medium",
          task.status === "completed" &&
            "line-through decoration-border decoration-1",
        )}
      >
        {task.text}
      </span>
      <button
        aria-label={`Copy task: ${task.text}`}
        className="mt-px inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground opacity-0 transition-[opacity,color,background-color] hover:bg-muted hover:text-foreground focus:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring group-hover/task:opacity-100"
        onClick={() => copyTextToClipboard(task.text, "Task copied")}
        title="Copy task"
        type="button"
      >
        <ClipboardCopy aria-hidden className="h-3 w-3" />
      </button>
    </li>
  );
}

function TaskRailLoadingState() {
  return (
    <div
      aria-label="Loading session plan"
      aria-live="polite"
      className="px-4 py-5"
      role="status"
    >
      <div className="h-3 w-16 animate-pulse rounded bg-muted" />
      <div className="mt-3 h-1.5 animate-pulse rounded-full bg-muted" />
      <div className="mt-6 space-y-3">
        {[0, 1, 2].map((index) => (
          <div
            className="flex items-center gap-3"
            key={`task-rail-loading-${index}`}
          >
            <div className="h-4 w-4 animate-pulse rounded-full bg-muted" />
            <div
              className={cn(
                "h-3 animate-pulse rounded bg-muted",
                index === 1 ? "w-2/3" : "w-5/6",
              )}
            />
          </div>
        ))}
      </div>
      <span className="sr-only">Loading session plan</span>
    </div>
  );
}

function TaskRailEmptyState({
  description,
  icon = "empty",
  title,
}: {
  description: string;
  icon?: "empty" | "error";
  title: string;
}) {
  const Icon = icon === "error" ? TriangleAlert : ListChecks;
  return (
    <div className="flex min-h-48 flex-col items-center justify-center px-4 text-center">
      <div className="flex h-9 w-9 items-center justify-center rounded-xl border border-border/60 bg-background/50">
        <Icon
          aria-hidden
          className={cn(
            "h-4 w-4",
            icon === "error" ? "text-amber-500" : "text-muted-foreground/70",
          )}
        />
      </div>
      <p className="mt-3 text-sm font-medium">{title}</p>
      <p className="mt-1 max-w-56 text-xs leading-5 text-muted-foreground">
        {description}
      </p>
    </div>
  );
}

function taskStatusIcon(status: CodingSessionTaskStatus) {
  switch (status) {
    case "completed":
      return Check;
    case "in_progress":
      return LoaderCircle;
    case "pending":
      return Circle;
    case "blocked":
      return TriangleAlert;
    case "failed":
      return OctagonAlert;
    case "cancelled":
      return Ban;
    default:
      return CircleHelp;
  }
}

function statusLabel(status: CodingSessionTaskStatus): string {
  switch (status) {
    case "completed":
      return "Completed";
    case "in_progress":
      return "In progress";
    case "pending":
      return "Pending";
    case "blocked":
      return "Blocked";
    case "failed":
      return "Failed";
    case "cancelled":
      return "Cancelled";
    default:
      return "Status unknown";
  }
}

function countStatus(
  tasks: CodingSessionTask[],
  status: CodingSessionTaskStatus,
): number {
  return tasks.filter((task) => task.status === status).length;
}

function formatTaskTimestamp(timestamp: string): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return "recently";
  return new Intl.DateTimeFormat(undefined, {
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}

/**
 * The plan rail as a right-side sheet, for a workspace too narrow to dock it.
 *
 * Split out of `CodingSessionUmbrellaWorkspace.tsx`, which sits at the
 * repository's 1,000-line ceiling: the rule is to split the file, never to
 * raise the limit. Nothing about the sheet changed in the move.
 */
export function CodingSessionTaskRailSheet({
  dock,
}: {
  dock: {
    activeModel: CodingSessionTaskModel | null;
    close: () => void;
    open: boolean;
    show: () => void;
  };
}) {
  return (
    <Sheet
      onOpenChange={(open) => (open ? dock.show() : dock.close())}
      open={dock.open}
    >
      <SheetContent
        aria-describedby={undefined}
        className="w-[min(90vw,22rem)] max-w-none p-0"
        side="right"
      >
        <SheetTitle className="sr-only">Session plan</SheetTitle>
        <CodingSessionTaskRail model={dock.activeModel} variant="sheet" />
      </SheetContent>
    </Sheet>
  );
}
