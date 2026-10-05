import * as React from "react";
import { toast } from "sonner";

import {
  codingSessionNameKey,
  MAX_CODING_SESSION_NAME_BYTES,
  publishCodingSessionName,
} from "@/features/coding-sessions/lib/codingSessionName";
import { useCodingSessionNames } from "@/features/coding-sessions/useCodingSessionNames";
import { useIdentityQuery } from "@/shared/api/hooks";
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

import { CodingSessionTitleAttribution } from "./CodingSessionTitleOrigin";

/**
 * Rename a session: publishes the founder's 44229, which outranks any
 * generated 44252 title by tier (SV-31), so the "Auto-named" marker goes.
 *
 * The dialog reads the session's names itself rather than trusting the
 * caller's `currentName` alone: it opens prefilled with the **effective**
 * name — the founder's own, else a provider's generated title — and when that
 * name is generated it says so in one muted line naming the provider and the
 * model, so the attribution is one click away from any surface that shows the
 * name, whether or not that surface carries the marker itself.
 *
 * `founderPubkey` defaults to the signed-in identity, because only the
 * founder is offered this dialog (`canRename`) and only the founder's 44229
 * counts. Keeping the generated title unchanged and saving is allowed: it
 * adopts the model's words as the person's own, which is a real choice and
 * the marker honestly goes with it.
 */
export function CodingSessionNameDialog({
  channelId,
  currentName,
  founderPubkey,
  onOpenChange,
  open,
  sessionRef,
}: {
  channelId: string;
  currentName: string;
  founderPubkey?: string | null;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  sessionRef: string;
}) {
  const identity = useIdentityQuery();
  const founder = founderPubkey ?? identity.data?.pubkey ?? null;
  const channelIds = React.useMemo(() => [channelId], [channelId]);
  const nameSnapshot = useCodingSessionNames(open ? channelIds : []);
  const effective = founder
    ? (nameSnapshot.names.get(
        codingSessionNameKey(channelId, sessionRef, founder),
      ) ?? null)
    : null;
  const generated = effective?.origin === "generated" ? effective : null;
  const prefill = effective?.content ?? currentName;
  const [draft, setDraft] = React.useState(prefill);
  const [saving, setSaving] = React.useState(false);
  // The names read settles after the dialog opens; follow it until the person
  // types, never after — a late or live name must not clobber their draft.
  const edited = React.useRef(false);

  React.useEffect(() => {
    if (!open) edited.current = false;
    else if (!edited.current) setDraft(prefill);
  }, [prefill, open]);

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
        {generated ? (
          <CodingSessionTitleAttribution
            name={generated}
            testId="coding-session-name-dialog-attribution"
          />
        ) : null}
        <Input
          autoFocus
          data-testid="coding-session-name-input"
          maxLength={MAX_CODING_SESSION_NAME_BYTES}
          onChange={(event) => {
            edited.current = true;
            setDraft(event.target.value);
          }}
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
              saving ||
              !draft.trim() ||
              (generated === null && draft.trim() === prefill.trim())
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
