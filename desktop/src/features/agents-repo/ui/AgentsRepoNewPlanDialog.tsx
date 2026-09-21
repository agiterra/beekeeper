import * as React from "react";

import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";

import { agentsRepoCopy as copy } from "../lib/agentsRepoCopy";
import { newPlanPath } from "../lib/agentsRepoPaths";

/**
 * Asks for a plan name and hands back `plans/<slug>.md`. A dialog rather than
 * `window.prompt`: the Tauri webview does not implement `prompt` (it returns
 * null without showing anything), so the button did nothing in the app.
 */
export function AgentsRepoNewPlanDialog({
  open,
  onOpenChange,
  onCreate,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (path: string) => void;
}) {
  const [name, setName] = React.useState("");
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (open) {
      setName("");
      setError(null);
    }
  }, [open]);

  const preview = React.useMemo(() => {
    if (name.trim().length === 0) return null;
    const made = newPlanPath(name);
    return made.ok ? made.path : null;
  }, [name]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const made = newPlanPath(name);
    if (!made.ok) {
      setError(made.error);
      return;
    }
    onCreate(made.path);
    onOpenChange(false);
  };

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent data-testid="agents-repo-new-plan-dialog">
        <form onSubmit={submit}>
          <DialogHeader>
            <DialogTitle>{copy.newPlan}</DialogTitle>
            <DialogDescription>{copy.newPlanHelp}</DialogDescription>
          </DialogHeader>
          <div className="space-y-2 py-4">
            <Input
              autoFocus
              data-testid="agents-repo-new-plan-name"
              onChange={(event) => {
                setName(event.target.value);
                setError(null);
              }}
              placeholder="roadmap"
              value={name}
            />
            <p
              className="text-xs text-muted-foreground"
              data-testid="agents-repo-new-plan-preview"
            >
              {error ?? preview ?? copy.newPlanHelp}
            </p>
          </div>
          <DialogFooter>
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              {copy.cancel}
            </Button>
            <Button
              data-testid="agents-repo-new-plan-create"
              disabled={name.trim().length === 0}
              type="submit"
            >
              {copy.create}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
