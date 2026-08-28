import { Loader2 } from "lucide-react";
import * as React from "react";

import type {
  InstallCrewRolePacksResponse,
  PickedCrewRolePacks,
} from "@/shared/api/tauriTeams";
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
  crewRoleNameFields,
  crewRoleNamesMap,
  crewRoleResultLine,
  crewRoleResultRows,
  crewRolesDroppedNotes,
  crewRolesFailureMessage,
  crewRolesFoundNothing,
  crewRolesSeatedNote,
  crewRolesUnreadableFolder,
  INSTALL_CREW_ROLES_BODY,
  INSTALL_CREW_ROLES_CHOOSE_FOLDER,
  INSTALL_CREW_ROLES_NOTHING_FOUND,
  INSTALL_CREW_ROLES_ROSTER_PLAN,
  INSTALL_CREW_ROLES_TEAM_NAMES_HINT,
  INSTALL_CREW_ROLES_TEAM_NAMES_LABEL,
  INSTALL_CREW_ROLES_TITLE,
} from "./installCrewRolesCopy";

type InstallCrewRolesFormProps = {
  /** Called after a successful install so the caller can refetch and toast. */
  onInstalled: (result: InstallCrewRolePacksResponse) => void;
  /** Dismiss the surrounding dialog. */
  onClose: () => void;
  /** Told whenever an install starts or finishes, so the dialog can refuse
   * to close over a half-written store. */
  onBusyChange?: (busy: boolean) => void;
};

/**
 * The installer's body: choose a folder, name every identity in it, install.
 *
 * Exported separately from the dialog so a test can mount it without Radix's
 * portal — the same split `AddCodingSessionProviderForm` uses.
 */
export function InstallCrewRolesForm({
  onInstalled,
  onClose,
  onBusyChange,
}: InstallCrewRolesFormProps) {
  const [picked, setPicked] = React.useState<PickedCrewRolePacks | null>(null);
  const [names, setNames] = React.useState<Record<string, string>>({});
  const [isInstalling, setIsInstalling] = React.useState(false);
  const [result, setResult] =
    React.useState<InstallCrewRolePacksResponse | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    onBusyChange?.(isInstalling);
  }, [isInstalling, onBusyChange]);

  const choose = async () => {
    setError(null);
    try {
      const next = await pickCrewRolePacksDirectory();
      if (!next) return;
      setPicked(next);
      // Every field starts on the name that identity already carries here, so
      // an operator who installs without touching anything renames nobody.
      setNames(
        Object.fromEntries(
          crewRoleNameFields(next.packs).map((field) => [
            field.role,
            field.defaultName,
          ]),
        ),
      );
      setResult(null);
    } catch (cause) {
      // The picker only ever fails at picking or at reading, so this one *is*
      // the folder.
      setError(
        crewRolesUnreadableFolder(
          cause instanceof Error ? cause.message : String(cause),
        ),
      );
    }
  };

  const install = async () => {
    if (!picked || picked.packs.length === 0 || isInstalling) return;
    setIsInstalling(true);
    setError(null);
    try {
      const installed = await installCrewRolePacks(
        picked.directory,
        crewRoleNamesMap(picked.packs, names),
      );
      setResult(installed);
      if (!crewRolesFoundNothing(installed)) {
        onInstalled(installed);
      }
    } catch (cause) {
      // Which stage failed comes off the backend's own answer. Wrapping every
      // failure in the folder sentence sent an operator with a locked keychain
      // to look at their folder.
      setError(crewRolesFailureMessage(cause));
    } finally {
      setIsInstalling(false);
    }
  };

  const fields = picked ? crewRoleNameFields(picked.packs) : [];
  const rows = result ? crewRoleResultRows(result) : [];
  const foundNothing =
    (result !== null && crewRolesFoundNothing(result)) ||
    (result === null && picked !== null && picked.packs.length === 0);
  const isDone = result !== null && !crewRolesFoundNothing(result);

  return (
    <>
      {isDone ? null : (
        <div className="flex items-center gap-2">
          <input
            className="min-w-0 flex-1 truncate rounded-md border border-border bg-muted/40 px-3 py-2 text-sm text-muted-foreground"
            data-testid="install-crew-roles-path"
            placeholder="No folder chosen"
            readOnly
            value={picked?.directory ?? ""}
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

      {isDone || fields.length === 0 ? null : (
        <div className="flex flex-col gap-2">
          <p className="text-xs font-medium text-foreground">
            {INSTALL_CREW_ROLES_TEAM_NAMES_LABEL}
          </p>
          <div
            className="flex max-h-64 flex-col gap-2 overflow-y-auto"
            data-testid="install-crew-roles-names"
          >
            {fields.map((field) => (
              <div className="flex items-center gap-2" key={field.role}>
                <label
                  className="w-24 shrink-0 text-xs text-muted-foreground"
                  htmlFor={`install-crew-roles-name-${field.role}`}
                >
                  {field.label}
                </label>
                <input
                  className="min-w-0 flex-1 rounded-md border border-border bg-transparent px-3 py-2 text-sm"
                  data-testid={`install-crew-roles-name-${field.role}`}
                  disabled={isInstalling}
                  id={`install-crew-roles-name-${field.role}`}
                  onChange={(event) =>
                    setNames((current) => ({
                      ...current,
                      [field.role]: event.target.value,
                    }))
                  }
                  placeholder={field.defaultName}
                  value={names[field.role] ?? field.defaultName}
                />
              </div>
            ))}
          </div>
          <p className="text-2xs text-muted-foreground">
            {INSTALL_CREW_ROLES_TEAM_NAMES_HINT}
          </p>
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
              {crewRoleResultLine(row)}
            </li>
          ))}
        </ul>
      ) : null}

      {result && !crewRolesFoundNothing(result) ? (
        <div
          className="space-y-1 text-xs text-muted-foreground"
          data-testid="install-crew-roles-seats"
        >
          <p>{crewRolesSeatedNote(result)}</p>
          {crewRolesDroppedNotes(result).map((note) => (
            <p className="text-amber-600 dark:text-amber-400" key={note}>
              {note}
            </p>
          ))}
          {/* The store is written either way; what failed is the relay half.
              Swallowing this is what left a session header reading the old
              name with nothing on screen to explain it (ledger 80 (e)). */}
          {result.profileSyncError ? (
            <p
              className="text-amber-600 dark:text-amber-400"
              data-testid="install-crew-roles-profile-sync-error"
            >
              {result.profileSyncError}
            </p>
          ) : null}
        </div>
      ) : (
        <p className="text-xs text-muted-foreground">
          {INSTALL_CREW_ROLES_ROSTER_PLAN}
        </p>
      )}

      <div className="flex justify-end gap-2">
        {isDone ? (
          <Button
            data-testid="install-crew-roles-close"
            onClick={onClose}
            type="button"
          >
            Close
          </Button>
        ) : (
          <Button
            data-testid="install-crew-roles-submit"
            disabled={fields.length === 0 || isInstalling}
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
    </>
  );
}

type InstallCrewRolesDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Called after a successful install so the caller can refetch and toast. */
  onInstalled: (result: InstallCrewRolePacksResponse) => void;
};

/**
 * Turn a folder of role packs into agents, under one team.
 *
 * Two things the operator has to be able to see here. First, the names: every
 * pack in the folder gets a field, because each one becomes an identity a
 * person addresses by name and mints once — typing over a name that is already
 * installed renames that identity rather than making a second one. Second, the
 * result list: it names every role that was installed, says which of them were
 * only refreshed and which were renamed, and lists every child of the folder
 * that produced nothing and why. A dialog that reported only a count would let
 * a silently skipped pack look like a pack that installed.
 */
export function InstallCrewRolesDialog({
  open,
  onOpenChange,
  onInstalled,
}: InstallCrewRolesDialogProps) {
  const [isInstalling, setIsInstalling] = React.useState(false);

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

        {/* The form holds every piece of install state, and `DialogContent`
            unmounts when the dialog closes — so re-opening starts clean
            without an effect that has to remember to reset each field. */}
        <InstallCrewRolesForm
          onBusyChange={setIsInstalling}
          onClose={() => onOpenChange(false)}
          onInstalled={onInstalled}
        />
      </DialogContent>
    </Dialog>
  );
}
