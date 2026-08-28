import { Loader2 } from "lucide-react";
import * as React from "react";

import { getCodingSessionWorkdirState } from "@/shared/api/tauriCodingSessionWorkdirs";
import type {
  InstallCrewRolePacksResponse,
  PickedCrewRolePacks,
} from "@/shared/api/tauriTeams";
import {
  installCrewRolePacks,
  pickCrewRolePacksDirectory,
  scanProjectRolePacks,
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
  crewRolesCheckoutLookupFailed,
  crewRolesFoundNothing,
  crewRolesProjectFolderNote,
  crewRolesSeatedNote,
  crewRolesUnreadableFolder,
  installCrewRolesProjectFolderLabel,
  INSTALL_CREW_ROLES_BODY,
  INSTALL_CREW_ROLES_CHOOSE_FOLDER,
  INSTALL_CREW_ROLES_NO_CHECKOUT,
  INSTALL_CREW_ROLES_NOTHING_FOUND,
  INSTALL_CREW_ROLES_ROSTER_PLAN,
  INSTALL_CREW_ROLES_TEAM_NAMES_HINT,
  INSTALL_CREW_ROLES_TEAM_NAMES_LABEL,
  INSTALL_CREW_ROLES_TITLE,
} from "./installCrewRolesCopy";

/** The project the installer was opened in, when it was opened in one. */
export type InstallCrewRolesProject = {
  /** The NIP-MP coordinate the checkout directory is remembered under. */
  address: string;
  /**
   * The project's display name, put on the folder label.
   *
   * Required, not optional: the Agents tab resolves this project without
   * being told which one by the route, so a label that does not name it is a
   * folder the operator cannot check.
   */
  name: string;
};

type InstallCrewRolesFormProps = {
  /**
   * The project showing when the dialog opened, or `null`/absent outside one.
   *
   * With a project, the folder is chosen before the operator touches
   * anything — from *that project's* checkout directory. Without one, nothing
   * is looked up and the dialog is exactly what it was.
   */
  project?: InstallCrewRolesProject | null;
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
  project,
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
  /** `true` once the operator has picked a folder of their own, which retires
   * the project label for good — that folder is theirs, not the project's. */
  const [operatorPickedFolder, setOperatorPickedFolder] = React.useState(false);
  /** Why the project's folder was not used, when it was not. */
  const [projectNote, setProjectNote] = React.useState<string | null>(null);
  /** Set the moment the operator opens the picker, so a slow lookup landing
   * afterwards can never overwrite the folder they chose themselves. */
  const operatorChose = React.useRef(false);

  React.useEffect(() => {
    onBusyChange?.(isInstalling);
  }, [isInstalling, onBusyChange]);

  // Stable so the project lookup's effect depends on it without re-running on
  // every render; the setters it closes over are stable already.
  const applyScan = React.useCallback((next: PickedCrewRolePacks) => {
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
  }, []);

  // Ledger 85: opened inside a project, the dialog looks in that project's
  // checkout for `personas/roles` and opens on it. Read-only — the same scan
  // the picker runs, so the two can never disagree about what a folder holds.
  const projectAddress = project?.address ?? null;
  React.useEffect(() => {
    if (!projectAddress) return;
    let cancelled = false;
    // The project can change under an open dialog (the Agents tab's selector),
    // and the previous project's folder, label and note must not outlive it —
    // a path from one checkout under a label naming another is the exact lie
    // this whole change exists to remove. Cleared before the lookup, not
    // after, so nothing stale is on screen while it runs.
    if (!operatorChose.current) {
      setPicked(null);
      setNames({});
      setResult(null);
      setProjectNote(null);
    }
    void (async () => {
      // Two reads, two different things to be at fault. The store read comes
      // first and touches no folder, so its failure is reported as its own.
      let checkout: string;
      try {
        const state = await getCodingSessionWorkdirState();
        if (cancelled || operatorChose.current) return;
        checkout = state.byProject[projectAddress]?.path?.trim() ?? "";
      } catch (cause) {
        if (cancelled || operatorChose.current) return;
        setProjectNote(
          crewRolesCheckoutLookupFailed(
            cause instanceof Error ? cause.message : String(cause),
          ),
        );
        return;
      }
      if (checkout.length === 0) {
        setProjectNote(INSTALL_CREW_ROLES_NO_CHECKOUT);
        return;
      }
      try {
        const scan = await scanProjectRolePacks(checkout);
        if (cancelled || operatorChose.current) return;
        const note = crewRolesProjectFolderNote(scan);
        if (note !== null) {
          setProjectNote(note);
          return;
        }
        applyScan({
          directory: scan.directory,
          packs: scan.packs,
          skipped: scan.skipped,
        });
      } catch (cause) {
        // A folder that is there and unreadable is the folder's fault, and
        // saying so beats leaving "No folder chosen" with no explanation.
        if (cancelled || operatorChose.current) return;
        setProjectNote(
          crewRolesUnreadableFolder(
            cause instanceof Error ? cause.message : String(cause),
          ),
        );
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [applyScan, projectAddress]);

  const choose = async () => {
    setError(null);
    operatorChose.current = true;
    try {
      const next = await pickCrewRolePacksDirectory();
      if (!next) return;
      // A folder the operator picked is theirs, not the project's — the label
      // and the note both belong to the folder they replaced.
      setOperatorPickedFolder(true);
      setProjectNote(null);
      applyScan(next);
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
        <div className="flex flex-col gap-1.5">
          {/* Named whenever a project was resolved, not only when its folder
              was usable: this surface resolves a project the route never
              named, so an operator who cannot see which one cannot check it.
              A folder the operator picked themselves retires it. */}
          {projectAddress && !operatorPickedFolder ? (
            <p
              className="text-xs font-medium text-foreground"
              data-testid="install-crew-roles-folder-label"
            >
              {installCrewRolesProjectFolderLabel(project?.name)}
            </p>
          ) : null}
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
          {projectNote ? (
            <p
              className="text-xs text-muted-foreground"
              data-testid="install-crew-roles-project-note"
            >
              {projectNote}
            </p>
          ) : null}
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
  /** The project showing when this opened, or `null` outside one. */
  project?: InstallCrewRolesProject | null;
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
  project,
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
          project={project}
        />
      </DialogContent>
    </Dialog>
  );
}
