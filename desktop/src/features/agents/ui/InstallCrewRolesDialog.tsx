import { Loader2 } from "lucide-react";
import * as React from "react";

import type { InstallCrewRolePacksResponse } from "@/shared/api/tauriTeams";
import {
  installCrewRolePacks,
  pickCrewRolePacksDirectory,
} from "@/shared/api/tauriTeams";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import {
  crewRoleResultRows,
  crewRolesFoundNothing,
  crewRolesUnreadableFolder,
  INSTALL_CREW_ROLES_BODY,
  INSTALL_CREW_ROLES_CHOOSE_FOLDER,
  INSTALL_CREW_ROLES_NOTHING_FOUND,
  INSTALL_CREW_ROLES_ROSTER_NOTE,
  INSTALL_CREW_ROLES_TITLE,
} from "./installCrewRolesCopy";

type InstallCrewRolesDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Called after a successful install so the caller can refetch and toast. */
  onInstalled: (result: InstallCrewRolePacksResponse) => void;
};

/**
 * Turn a folder of role packs into agents, under one team that is a crew.
 *
 * The result list is the whole point: it names every role that was installed,
 * says which of them were only refreshed, and lists every child of the folder
 * that produced nothing and why. A dialog that reported only a count would let
 * a silently skipped pack look like a pack that installed.
 */
export function InstallCrewRolesDialog({
  open,
  onOpenChange,
  onInstalled,
}: InstallCrewRolesDialogProps) {
  const [directory, setDirectory] = React.useState("");
  const [isInstalling, setIsInstalling] = React.useState(false);
  const [result, setResult] =
    React.useState<InstallCrewRolePacksResponse | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (open) {
      setDirectory("");
      setIsInstalling(false);
      setResult(null);
      setError(null);
    }
  }, [open]);

  const choose = async () => {
    setError(null);
    try {
      const picked = await pickCrewRolePacksDirectory();
      if (picked) {
        setDirectory(picked);
        setResult(null);
      }
    } catch (cause) {
      setError(crewRolesUnreadableFolder(String(cause)));
    }
  };

  const install = async () => {
    if (!directory || isInstalling) return;
    setIsInstalling(true);
    setError(null);
    try {
      const installed = await installCrewRolePacks(directory);
      setResult(installed);
      if (!crewRolesFoundNothing(installed)) {
        onInstalled(installed);
      }
    } catch (cause) {
      setError(crewRolesUnreadableFolder(String(cause)));
    } finally {
      setIsInstalling(false);
    }
  };

  const rows = result ? crewRoleResultRows(result) : [];
  const foundNothing = result !== null && crewRolesFoundNothing(result);
  const isDone = result !== null && !foundNothing;

  return (
    <Dialog
      onOpenChange={(next) => {
        // Not dismissible mid-install: closing would leave a half-written
        // store with no screen reporting what landed.
        if (isInstalling) return;
        onOpenChange(next);
      }}
      open={open}
    >
      <DialogContent
        className="max-w-lg"
        data-testid="install-crew-roles-dialog"
      >
        <DialogHeader>
          <DialogTitle>{INSTALL_CREW_ROLES_TITLE}</DialogTitle>
          <DialogDescription>{INSTALL_CREW_ROLES_BODY}</DialogDescription>
        </DialogHeader>

        {isDone ? null : (
          <div className="flex items-center gap-2">
            <input
              className="min-w-0 flex-1 truncate rounded-md border border-border bg-muted/40 px-3 py-2 text-sm text-muted-foreground"
              data-testid="install-crew-roles-path"
              placeholder="No folder chosen"
              readOnly
              value={directory}
            />
            <Button
              data-testid="install-crew-roles-choose"
              disabled={isInstalling}
              onClick={choose}
              type="button"
              variant="outline"
            >
              {INSTALL_CREW_ROLES_CHOOSE_FOLDER}
            </Button>
          </div>
        )}

        {foundNothing ? (
          <p
            className="text-sm text-muted-foreground"
            data-testid="install-crew-roles-empty"
          >
            {INSTALL_CREW_ROLES_NOTHING_FOUND}
          </p>
        ) : null}

        {error ? (
          <p
            className="text-sm text-destructive"
            data-testid="install-crew-roles-error"
          >
            {error}
          </p>
        ) : null}

        {rows.length > 0 ? (
          <ul
            className="space-y-1 text-sm"
            data-testid="install-crew-roles-result"
          >
            {rows.map((row) => (
              <li
                className={
                  row.kind === "skipped"
                    ? "text-muted-foreground"
                    : "text-foreground"
                }
                key={`${row.kind}-${row.text}`}
              >
                {row.text}
                {row.kind === "installed" && row.note ? (
                  <span className="text-muted-foreground"> — {row.note}</span>
                ) : null}
              </li>
            ))}
          </ul>
        ) : null}

        <p className="text-xs text-muted-foreground">
          {INSTALL_CREW_ROLES_ROSTER_NOTE}
        </p>

        <div className="flex justify-end gap-2">
          {isDone ? (
            <Button
              data-testid="install-crew-roles-close"
              onClick={() => onOpenChange(false)}
              type="button"
            >
              Close
            </Button>
          ) : (
            <Button
              data-testid="install-crew-roles-submit"
              disabled={!directory || isInstalling}
              onClick={install}
              type="button"
            >
              {isInstalling ? (
                <Loader2 aria-hidden className="size-4 animate-spin" />
              ) : null}
              Install
            </Button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
