import { CalendarDays, X } from "lucide-react";
import * as React from "react";

import { Button } from "@/shared/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";
import { cn } from "@/shared/lib/cn";

import { dueDateError } from "../lib/todoOp";
import { formatDue, isOverdue } from "../lib/todoPeople";

/**
 * The due-date chip on a to-do row: shows the date (red when overdue and the
 * item is open), opens a native date input to change it, and clears it. A
 * read-only viewer gets the chip with no trigger.
 */
export function DueDatePicker({
  due,
  done,
  canEdit,
  onChange,
  testId,
}: {
  due: string | null;
  done: boolean;
  canEdit: boolean;
  onChange: (due: string | null) => void;
  testId?: string;
}) {
  const [open, setOpen] = React.useState(false);
  const [draft, setDraft] = React.useState(due ?? "");
  React.useEffect(() => {
    if (open) setDraft(due ?? "");
  }, [due, open]);
  const overdue = due !== null && !done && isOverdue(due);
  const label = due === null ? "Due" : formatDue(due);
  const draftError = draft.length > 0 ? dueDateError(draft) : null;

  const chip = (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-sm px-1.5 py-0.5 text-2xs",
        due === null
          ? "text-muted-foreground"
          : overdue
            ? "bg-destructive/15 text-destructive"
            : "bg-muted text-foreground",
        canEdit && "hover:bg-accent",
      )}
      data-overdue={overdue ? "true" : undefined}
      data-testid={testId}
    >
      <CalendarDays aria-hidden="true" className="h-3 w-3" />
      {label}
    </span>
  );

  if (!canEdit) return chip;

  return (
    <Popover onOpenChange={setOpen} open={open}>
      <PopoverTrigger asChild>
        <button
          aria-label={due === null ? "Set due date" : `Due ${label}; change`}
          className="rounded-sm focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring"
          type="button"
        >
          {chip}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-56 p-3">
        <form
          className="flex flex-col gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            if (draft.length === 0) {
              onChange(null);
              setOpen(false);
              return;
            }
            if (draftError) return;
            onChange(draft);
            setOpen(false);
          }}
        >
          <label
            className="text-xs text-muted-foreground"
            htmlFor="todo-due-input"
          >
            Due date
          </label>
          <input
            className="h-8 rounded-md border border-input bg-background px-2 text-sm"
            data-testid="todo-due-input"
            id="todo-due-input"
            onChange={(event) => setDraft(event.target.value)}
            type="date"
            value={draft}
          />
          {draftError ? (
            <p className="text-2xs text-destructive">{draftError}</p>
          ) : null}
          <div className="flex items-center justify-between gap-2">
            <Button
              disabled={due === null}
              onClick={() => {
                onChange(null);
                setOpen(false);
              }}
              size="xs"
              type="button"
              variant="ghost"
            >
              <X aria-hidden="true" className="mr-1 h-3 w-3" />
              Clear
            </Button>
            <Button disabled={draftError !== null} size="xs" type="submit">
              Save
            </Button>
          </div>
        </form>
      </PopoverContent>
    </Popover>
  );
}
