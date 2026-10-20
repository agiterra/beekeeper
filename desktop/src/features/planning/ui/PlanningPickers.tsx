import {
  Circle,
  CircleCheck,
  CircleDashed,
  CircleDot,
  CircleX,
  Minus,
  SignalHigh,
  SignalLow,
  SignalMedium,
  TriangleAlert,
} from "lucide-react";

import { cn } from "@/shared/lib/cn";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

import type {
  PlanningPriority,
  PlanningPriorityLevel,
  PlanningStatus,
  PlanningStatusCategory,
} from "../lib/planningTypes";

const STATUS_GLYPH: Record<
  PlanningStatusCategory,
  { icon: typeof Circle; className: string }
> = {
  triage: { icon: CircleDashed, className: "text-orange-500" },
  backlog: { icon: CircleDashed, className: "text-muted-foreground" },
  unstarted: { icon: Circle, className: "text-muted-foreground" },
  started: { icon: CircleDot, className: "text-amber-500" },
  completed: { icon: CircleCheck, className: "text-emerald-600" },
  canceled: { icon: CircleX, className: "text-muted-foreground" },
};

export function StatusGlyph({
  category,
  className,
}: {
  category: PlanningStatusCategory;
  className?: string;
}) {
  const glyph = STATUS_GLYPH[category];
  const Icon = glyph.icon;
  return (
    <Icon
      aria-hidden="true"
      className={cn("h-3.5 w-3.5 shrink-0", glyph.className, className)}
    />
  );
}

export function PriorityGlyph({
  level,
  className,
}: {
  level: PlanningPriorityLevel;
  className?: string;
}) {
  const Icon =
    level === "urgent"
      ? TriangleAlert
      : level === "high"
        ? SignalHigh
        : level === "medium"
          ? SignalMedium
          : level === "low"
            ? SignalLow
            : Minus;
  return (
    <Icon
      aria-hidden="true"
      className={cn(
        "h-3.5 w-3.5 shrink-0",
        level === "urgent" ? "text-red-500" : "text-muted-foreground",
        className,
      )}
    />
  );
}

const TRIGGER_CLASS =
  "inline-flex items-center gap-1.5 rounded-sm px-1 py-0.5 text-xs text-foreground hover:bg-accent focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-default disabled:hover:bg-transparent";

/**
 * A status control: the glyph (and the label when `showLabel`), and a menu
 * of the provider's statuses. Read-only viewers get the glyph alone.
 */
export function StatusPicker({
  status,
  statuses,
  canEdit,
  pending = false,
  showLabel = false,
  onChange,
  testId,
}: {
  status: PlanningStatus;
  statuses: readonly PlanningStatus[];
  canEdit: boolean;
  pending?: boolean;
  showLabel?: boolean;
  onChange: (statusId: string) => void;
  testId?: string;
}) {
  const face = (
    <>
      <StatusGlyph
        category={status.category}
        className={pending ? "animate-pulse" : undefined}
      />
      {showLabel ? <span>{status.label}</span> : null}
    </>
  );
  if (!canEdit) {
    return (
      <span
        className="inline-flex items-center gap-1.5 px-1 py-0.5 text-xs"
        data-testid={testId}
        title={status.label}
      >
        {face}
      </span>
    );
  }
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          aria-label={`Status: ${status.label}. Change status`}
          className={TRIGGER_CLASS}
          data-testid={testId}
          disabled={pending}
          onClick={(event) => event.stopPropagation()}
          type="button"
        >
          {face}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        className="w-48"
        onClick={(event) => event.stopPropagation()}
      >
        <DropdownMenuRadioGroup onValueChange={onChange} value={status.id}>
          {statuses.map((option) => (
            <DropdownMenuRadioItem
              data-testid={`planning-status-option-${option.id}`}
              key={option.id}
              value={option.id}
            >
              <span className="flex items-center gap-2">
                <StatusGlyph category={option.category} />
                {option.label}
              </span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export function PriorityPicker({
  priority,
  priorities,
  canEdit,
  pending = false,
  showLabel = false,
  onChange,
  testId,
}: {
  priority: PlanningPriority;
  priorities: readonly PlanningPriority[];
  canEdit: boolean;
  pending?: boolean;
  showLabel?: boolean;
  onChange: (priorityId: string) => void;
  testId?: string;
}) {
  const face = (
    <>
      <PriorityGlyph
        className={pending ? "animate-pulse" : undefined}
        level={priority.level}
      />
      {showLabel ? <span>{priority.label}</span> : null}
    </>
  );
  if (!canEdit) {
    return (
      <span
        className="inline-flex items-center gap-1.5 px-1 py-0.5 text-xs"
        data-testid={testId}
        title={priority.label}
      >
        {face}
      </span>
    );
  }
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          aria-label={`Priority: ${priority.label}. Change priority`}
          className={TRIGGER_CLASS}
          data-testid={testId}
          disabled={pending}
          onClick={(event) => event.stopPropagation()}
          type="button"
        >
          {face}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        className="w-44"
        onClick={(event) => event.stopPropagation()}
      >
        <DropdownMenuRadioGroup onValueChange={onChange} value={priority.id}>
          {priorities.map((option) => (
            <DropdownMenuRadioItem
              data-testid={`planning-priority-option-${option.id}`}
              key={option.id}
              value={option.id}
            >
              <span className="flex items-center gap-2">
                <PriorityGlyph level={option.level} />
                {option.label}
              </span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** A small "saving" mark for a row or field whose write is in flight. */
export function PendingMark({ label = "Saving" }: { label?: string }) {
  return (
    <span
      className="text-2xs text-muted-foreground"
      data-testid="planning-pending"
      role="status"
    >
      {label}…
    </span>
  );
}
