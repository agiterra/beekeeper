import { Lock, Users } from "lucide-react";
import * as React from "react";

import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Switch } from "@/shared/ui/switch";

import { type TodoVisibility, todoTextError } from "../lib/todoOp";

export type CreateTodoListInput = {
  title: string;
  visibility: TodoVisibility;
  pinned: boolean;
};

/**
 * "New to-do list": a title, who can see it, and whether it sits in the
 * project sidebar. Visibility is fixed for the list's life, which the copy
 * says plainly — a personal list cannot later be shared, nor a project list
 * made private, because every op already on the relay carries the choice.
 */
export function CreateTodoListDialog({
  open,
  projectName,
  isCreating,
  onOpenChange,
  onCreate,
}: {
  open: boolean;
  projectName: string;
  isCreating: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (input: CreateTodoListInput) => Promise<void>;
}) {
  const [title, setTitle] = React.useState("");
  const [visibility, setVisibility] = React.useState<TodoVisibility>("project");
  const [pinned, setPinned] = React.useState(true);
  const [error, setError] = React.useState<string | null>(null);
  React.useEffect(() => {
    if (open) {
      setTitle("");
      setVisibility("project");
      setPinned(true);
      setError(null);
    }
  }, [open]);
  const trimmed = title.trim();
  const titleError =
    trimmed.length > 0 ? todoTextError("title", trimmed) : null;

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (trimmed.length === 0 || titleError) return;
    setError(null);
    try {
      await onCreate({ title: trimmed, visibility, pinned });
      onOpenChange(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  const choice = (
    value: TodoVisibility,
    icon: React.ReactNode,
    label: string,
    detail: string,
  ) => (
    <button
      aria-pressed={visibility === value}
      className={cn(
        "flex flex-1 flex-col items-start gap-1 rounded-md border px-3 py-2 text-left",
        visibility === value
          ? "border-primary bg-primary/10"
          : "border-border hover:bg-muted/60",
      )}
      data-testid={`todo-list-visibility-${value}`}
      onClick={() => setVisibility(value)}
      type="button"
    >
      <span className="flex items-center gap-1.5 text-sm font-medium">
        {icon}
        {label}
      </span>
      <span className="text-2xs text-muted-foreground">{detail}</span>
    </button>
  );

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent aria-describedby={undefined} className="max-w-sm">
        <form className="flex flex-col gap-4" onSubmit={submit}>
          <DialogHeader>
            <DialogTitle>New to-do list in {projectName}</DialogTitle>
          </DialogHeader>
          <label
            className="flex flex-col gap-1 text-xs text-muted-foreground"
            htmlFor="todo-list-create-title"
          >
            Title
            <Input
              autoFocus
              data-testid="todo-list-create-title"
              id="todo-list-create-title"
              maxLength={1024}
              onChange={(event) => setTitle(event.target.value)}
              placeholder="Launch checklist"
              value={title}
            />
          </label>
          {titleError ? (
            <p className="text-2xs text-destructive">{titleError}</p>
          ) : null}
          <div className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground">
              Who can see it
            </span>
            <div className="flex gap-2">
              {choice(
                "project",
                <Users aria-hidden="true" className="h-3.5 w-3.5" />,
                "Project",
                "Every project member reads and edits it.",
              )}
              {choice(
                "personal",
                <Lock aria-hidden="true" className="h-3.5 w-3.5" />,
                "Personal",
                "Only you. The relay withholds it from everyone else.",
              )}
            </div>
            <p className="text-2xs text-muted-foreground">
              This cannot be changed later; make a new list instead.
            </p>
          </div>
          <label
            className="flex items-center justify-between gap-3 text-sm"
            htmlFor="todo-list-create-pinned"
          >
            <span className="flex flex-col">
              <span>Pin to the sidebar</span>
              <span className="text-2xs text-muted-foreground">
                {visibility === "personal"
                  ? "Only your sidebar, since only you can see it."
                  : "Every member's sidebar."}
              </span>
            </span>
            <Switch
              checked={pinned}
              data-testid="todo-list-create-pinned"
              id="todo-list-create-pinned"
              onCheckedChange={setPinned}
            />
          </label>
          {error ? (
            <p
              className="text-xs text-destructive"
              data-testid="todo-list-create-error"
            >
              {error}
            </p>
          ) : null}
          <DialogFooter>
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              Cancel
            </Button>
            <Button
              data-testid="todo-list-create-submit"
              disabled={
                isCreating || trimmed.length === 0 || titleError !== null
              }
              type="submit"
            >
              {isCreating ? "Creating…" : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
