import * as React from "react";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { projectTeamSetupError } from "../lib/projectTeamSetup";
import {
  projectRosterAssociationLabel,
  type ProjectRosterEntry,
} from "../lib/projectRosterReadiness";
import { agentName, useProjectTeamSetupAgents } from "./ProjectTeamSetupAgents";

export const NAME_UNAVAILABLE = "Name not available";

/**
 * One installed role's agent: current name, role and whether it is this
 * project's agent on this computer, with rename in place. The pubkey is kept
 * out of the row (the roster card lists it under details). Rename is offered
 * only where a local record exists; it changes neither role nor association.
 */
export function InstalledRoleRow({ entry }: { entry: ProjectRosterEntry }) {
  const { names, rename } = useProjectTeamSetupAgents();
  const current = agentName(names, entry.agentPubkey);
  const [editing, setEditing] = React.useState(false);
  const [value, setValue] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [note, setNote] = React.useState<{
    text: string;
    alert: boolean;
  } | null>(null);
  const trimmed = value.trim();
  const canRename = rename !== null && entry.association !== "missing";
  const save = async () => {
    if (!rename || !trimmed || trimmed === current) return;
    setBusy(true);
    setNote(null);
    try {
      const result = await rename({ pubkey: entry.agentPubkey, name: trimmed });
      setEditing(false);
      if (result.profileSyncError)
        setNote({
          text: `Renamed on this computer, but the profile update didn't reach the relay yet: ${result.profileSyncError}`,
          alert: false,
        });
    } catch (failure) {
      setNote({
        text: `The name couldn't be changed. ${projectTeamSetupError(failure)}`,
        alert: true,
      });
    } finally {
      setBusy(false);
    }
  };
  const associated = entry.association === "associated";
  return (
    <li
      className="min-w-0 space-y-1 text-sm"
      data-association={entry.association}
      data-testid="project-team-setup-installed-role"
    >
      <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
        <span className="min-w-0 break-words font-medium">
          {current ?? entry.name ?? NAME_UNAVAILABLE}
        </span>
        <span className="text-muted-foreground">· {entry.role}</span>
        <span
          className={cn(
            "min-w-0 break-words",
            associated || entry.association === "unknown"
              ? "text-muted-foreground"
              : "text-destructive",
          )}
          data-testid="project-team-setup-installed-role-association"
        >
          · {projectRosterAssociationLabel(entry.association)}
        </span>
        {canRename && !editing ? (
          <Button
            onClick={() => {
              setValue(current ?? "");
              setNote(null);
              setEditing(true);
            }}
            size="sm"
            type="button"
            variant="ghost"
          >
            Rename
          </Button>
        ) : null}
      </div>
      {editing ? (
        <form
          className="flex min-w-0 flex-wrap items-center gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <Input
            aria-label={`New name for the ${entry.role} agent`}
            className="min-w-0 max-w-xs"
            disabled={busy}
            onChange={(event) => setValue(event.target.value)}
            value={value}
          />
          <Button
            disabled={busy || !trimmed || trimmed === current}
            size="sm"
            type="submit"
          >
            {busy ? "Saving…" : "Save name"}
          </Button>
          <Button
            disabled={busy}
            onClick={() => setEditing(false)}
            size="sm"
            type="button"
            variant="ghost"
          >
            Cancel
          </Button>
        </form>
      ) : null}
      {note ? (
        <p
          className={note.alert ? "text-destructive" : "text-muted-foreground"}
          role={note.alert ? "alert" : "status"}
        >
          {note.text}
        </p>
      ) : null}
    </li>
  );
}
