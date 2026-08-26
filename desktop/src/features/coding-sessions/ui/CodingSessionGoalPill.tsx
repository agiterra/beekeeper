import * as React from "react";
import { Pencil, Target } from "lucide-react";
import { toast } from "sonner";

import {
  MAX_CODING_SESSION_GOAL_BYTES,
  publishCodingSessionGoal,
  type CodingSessionGoal,
} from "@/features/coding-sessions/lib/codingSessionGoal";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Textarea } from "@/shared/ui/textarea";
import { cn } from "@/shared/lib/cn";

import { CODING_SESSION_COLUMN_CLASS } from "./CodingSessionColumn";
import { CODING_SESSION_COLUMN_EXPANDED_CLASS } from "./CodingSessionColumn";

export function CodingSessionGoalPill({
  channelId,
  currentUserPubkey,
  founderPubkey,
  goal,
  headerCarriesGoal = false,
  workspaceExpanded = false,
  sessionRef,
  variant = "workspace",
}: {
  channelId: string;
  currentUserPubkey: string | null;
  founderPubkey: string | null;
  goal: CodingSessionGoal | null;
  /** The session header already renders the goal text; leave only edit access. */
  headerCarriesGoal?: boolean;
  /** Matches the transcript/composer when no secondary rail is open. */
  workspaceExpanded?: boolean;
  sessionRef: string | null;
  variant?: "workspace" | "catalog";
}) {
  const [open, setOpen] = React.useState(false);
  const [draft, setDraft] = React.useState(goal?.content ?? "");
  const [saving, setSaving] = React.useState(false);
  const canEdit =
    sessionRef !== null &&
    founderPubkey !== null &&
    currentUserPubkey?.toLowerCase() === founderPubkey.toLowerCase();

  React.useEffect(() => {
    if (!open) setDraft(goal?.content ?? "");
  }, [goal?.content, open]);

  if (!goal && !canEdit) return null;
  const compact = variant === "catalog";
  if (goal && headerCarriesGoal && !compact && !canEdit) return null;
  const save = async () => {
    if (!canEdit || !sessionRef) return;
    setSaving(true);
    try {
      await publishCodingSessionGoal({ channelId, content: draft, sessionRef });
      setOpen(false);
      toast.success(goal ? "Session goal updated." : "Session goal added.");
    } catch (error) {
      toast.error(
        error instanceof Error
          ? error.message
          : "Unable to update the session goal.",
      );
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <div
        className={
          compact
            ? "mt-2 flex min-w-0 items-start gap-1.5 rounded-md bg-primary/8 px-2 py-1.5 text-xs"
            : cn(
                CODING_SESSION_COLUMN_CLASS,
                workspaceExpanded && CODING_SESSION_COLUMN_EXPANDED_CLASS,
                goal && !headerCarriesGoal
                  ? "flex items-start gap-2 rounded-xl border border-primary/20 bg-primary/8 px-3 py-2"
                  : "flex items-center",
              )
        }
        data-testid={`coding-session-goal-${variant}`}
      >
        {!goal && !compact ? (
          <Button
            className="h-7 gap-1.5 px-2 text-xs text-muted-foreground"
            data-testid="coding-session-goal-add-workspace"
            onClick={() => setOpen(true)}
            size="sm"
            type="button"
            variant="ghost"
          >
            <Target className="size-3.5" />
            Add goal
          </Button>
        ) : headerCarriesGoal && !compact ? (
          canEdit ? (
            <Button
              className="h-7 gap-1.5 px-2 text-xs text-muted-foreground"
              data-testid="coding-session-goal-edit-workspace"
              onClick={() => setOpen(true)}
              size="sm"
              type="button"
              variant="ghost"
            >
              <Target className="size-3.5" />
              Edit goal
            </Button>
          ) : null
        ) : (
          <>
            <Target className="mt-0.5 size-3.5 shrink-0 text-primary" />
            <p className="min-w-0 flex-1 text-foreground">
              <span className="font-medium">Goal:</span>{" "}
              <span
                className={compact ? "line-clamp-2" : "whitespace-pre-wrap"}
              >
                {goal?.content ?? "Add a goal for this session"}
              </span>
            </p>
            {canEdit ? (
              <Button
                aria-label={goal ? "Edit session goal" : "Add session goal"}
                className="shrink-0"
                data-testid={`coding-session-goal-edit-${variant}`}
                onClick={() => setOpen(true)}
                size="icon-xs"
                type="button"
                variant="ghost"
              >
                <Pencil />
              </Button>
            ) : null}
          </>
        )}
      </div>
      {canEdit ? (
        <Dialog onOpenChange={setOpen} open={open}>
          <DialogContent data-testid="coding-session-goal-dialog">
            <DialogHeader>
              <DialogTitle>
                {goal ? "Edit session goal" : "Add session goal"}
              </DialogTitle>
              <DialogDescription>
                Keep the shared objective short enough to scan while the session
                is running.
              </DialogDescription>
            </DialogHeader>
            <Textarea
              autoFocus
              data-testid="coding-session-goal-input"
              maxLength={MAX_CODING_SESSION_GOAL_BYTES}
              onChange={(event) => setDraft(event.target.value)}
              placeholder="What should this session accomplish?"
              rows={4}
              value={draft}
            />
            <DialogFooter>
              <Button
                onClick={() => setOpen(false)}
                type="button"
                variant="ghost"
              >
                Cancel
              </Button>
              <Button
                data-testid="coding-session-goal-save"
                disabled={saving || !draft.trim()}
                onClick={() => void save()}
                type="button"
              >
                {saving ? "Saving…" : "Save goal"}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      ) : null}
    </>
  );
}
