import * as React from "react";
import { useNavigate, useParams } from "@tanstack/react-router";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/shared/ui/alert-dialog";
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
import {
  closeShellSession,
  renameShellSession,
  type ShellSessionInfo,
} from "@/shared/api/tauriShell";

import { useShellSessions } from "./useShellSessions";

/**
 * Owns the rename dialog and close confirmation for built-in shell sessions.
 * Callers wire `requestRename`/`requestClose` into their rows and render
 * `dialogs` once; closing the session that's on screen navigates away from
 * its now-dead route.
 */
export function useShellSessionDialogs(): {
  requestRename: (session: ShellSessionInfo) => void;
  requestClose: (session: ShellSessionInfo) => void;
  dialogs: React.ReactNode;
} {
  const { refresh } = useShellSessions();
  const navigate = useNavigate();
  const activeSessionId = useParams({
    strict: false,
    select: (p) => (p as { sessionId?: string }).sessionId,
  });
  const [renameTarget, setRenameTarget] =
    React.useState<ShellSessionInfo | null>(null);
  const [renameValue, setRenameValue] = React.useState("");
  const [renaming, setRenaming] = React.useState(false);
  const [closeTarget, setCloseTarget] = React.useState<ShellSessionInfo | null>(
    null,
  );
  const [closing, setClosing] = React.useState(false);

  const requestRename = React.useCallback((session: ShellSessionInfo) => {
    setRenameTarget(session);
    setRenameValue(session.title);
  }, []);

  const requestClose = React.useCallback((session: ShellSessionInfo) => {
    setCloseTarget(session);
  }, []);

  const submitRename = React.useCallback(() => {
    if (!renameTarget) return;
    const title = renameValue.trim();
    if (title === "" || title === renameTarget.title) {
      setRenameTarget(null);
      return;
    }
    setRenaming(true);
    renameShellSession(renameTarget.sessionId, title)
      .then(refresh)
      .catch(() => {
        // Backend rejected/unavailable; leave the name as-is.
      })
      .finally(() => {
        setRenaming(false);
        setRenameTarget(null);
      });
  }, [renameTarget, renameValue, refresh]);

  const confirmClose = React.useCallback(() => {
    if (!closeTarget) return;
    const { sessionId } = closeTarget;
    setClosing(true);
    closeShellSession(sessionId)
      .catch(() => {
        // Already gone.
      })
      .then(() => {
        refresh();
        // If the closed session is the one on screen, leave its now-dead route.
        if (sessionId === activeSessionId) {
          return navigate({ to: "/" });
        }
        return undefined;
      })
      .finally(() => {
        setClosing(false);
        setCloseTarget(null);
      });
  }, [closeTarget, activeSessionId, navigate, refresh]);

  const dialogs = (
    <>
      <Dialog
        open={renameTarget !== null}
        onOpenChange={(open) => {
          if (!open) setRenameTarget(null);
        }}
      >
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>Rename shell</DialogTitle>
            <DialogDescription>
              Give this session a name to recognize it in the sidebar.
            </DialogDescription>
          </DialogHeader>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              submitRename();
            }}
          >
            <Input
              autoFocus
              value={renameValue}
              onChange={(event) => setRenameValue(event.target.value)}
              placeholder="Session name"
              aria-label="Session name"
              data-testid="builtin-shell-rename-input"
              onKeyDown={(event) => {
                if (event.key === "Escape") setRenameTarget(null);
              }}
            />
            <DialogFooter className="mt-4">
              <Button
                type="button"
                variant="outline"
                onClick={() => setRenameTarget(null)}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                disabled={renaming || renameValue.trim() === ""}
                data-testid="builtin-shell-rename-save"
              >
                Save
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <AlertDialog
        open={closeTarget !== null}
        onOpenChange={(open) => {
          if (!open) setCloseTarget(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Close this shell?</AlertDialogTitle>
            <AlertDialogDescription>
              {closeTarget
                ? `"${closeTarget.title}" and anything running in it will be terminated, and its saved history forgotten. This can't be undone.`
                : ""}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={closing}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              disabled={closing}
              onClick={(event) => {
                // Keep the dialog mounted through the async close; we dismiss it
                // ourselves once the backend call settles.
                event.preventDefault();
                confirmClose();
              }}
              data-testid="builtin-shell-close-confirm"
            >
              Close shell
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );

  return { requestRename, requestClose, dialogs };
}
