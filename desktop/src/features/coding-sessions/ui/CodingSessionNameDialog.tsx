import * as React from "react";
import { toast } from "sonner";

import {
  MAX_CODING_SESSION_NAME_BYTES,
  publishCodingSessionName,
} from "@/features/coding-sessions/lib/codingSessionName";
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

export function CodingSessionNameDialog({
  channelId,
  currentName,
  onOpenChange,
  open,
  sessionRef,
}: {
  channelId: string;
  currentName: string;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  sessionRef: string;
}) {
  const [draft, setDraft] = React.useState(currentName);
  const [saving, setSaving] = React.useState(false);

  React.useEffect(() => {
    if (open) setDraft(currentName);
  }, [currentName, open]);

  const save = async () => {
    setSaving(true);
    try {
      await publishCodingSessionName({ channelId, content: draft, sessionRef });
      onOpenChange(false);
      toast.success("Session renamed.");
    } catch (error) {
      toast.error(
        error instanceof Error
          ? error.message
          : "Unable to rename the session.",
      );
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent data-testid="coding-session-name-dialog">
        <DialogHeader>
          <DialogTitle>Rename session</DialogTitle>
          <DialogDescription>
            Choose a short name for navigation. The session goal remains a
            separate description of the work.
          </DialogDescription>
        </DialogHeader>
        <Input
          autoFocus
          data-testid="coding-session-name-input"
          maxLength={MAX_CODING_SESSION_NAME_BYTES}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && draft.trim() && !saving) {
              event.preventDefault();
              void save();
            }
          }}
          placeholder="Session name"
          value={draft}
        />
        <DialogFooter>
          <Button
            onClick={() => onOpenChange(false)}
            type="button"
            variant="ghost"
          >
            Cancel
          </Button>
          <Button
            data-testid="coding-session-name-save"
            disabled={
              saving || !draft.trim() || draft.trim() === currentName.trim()
            }
            onClick={() => void save()}
            type="button"
          >
            {saving ? "Saving…" : "Save name"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
