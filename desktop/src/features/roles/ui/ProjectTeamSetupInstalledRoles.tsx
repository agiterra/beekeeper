import * as React from "react";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import {
  projectTeamSetupError,
  type ProjectTeamSetupActivation,
  type ProjectTeamSetupInstalledRole,
} from "../lib/projectTeamSetup";
import {
  agentName,
  shortPubkey,
  useProjectTeamSetupAgents,
} from "./ProjectTeamSetupAgents";

const NAME_UNAVAILABLE = "Name not available";

function InstalledRoleRow({ role }: { role: ProjectTeamSetupInstalledRole }) {
  const { names, rename } = useProjectTeamSetupAgents();
  const current = agentName(names, role.agentPubkey);
  const [editing, setEditing] = React.useState(false);
  const [value, setValue] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [note, setNote] = React.useState<{
    text: string;
    alert: boolean;
  } | null>(null);
  const trimmed = value.trim();
  const save = async () => {
    if (!rename || !trimmed || trimmed === current) return;
    setBusy(true);
    setNote(null);
    try {
      const result = await rename({ pubkey: role.agentPubkey, name: trimmed });
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
  return (
    <li
      className="space-y-1 text-sm"
      data-testid="project-team-setup-installed-role"
    >
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span className="font-medium">{current ?? NAME_UNAVAILABLE}</span>
        <span className="text-muted-foreground">· {role.role}</span>
        <span
          className="font-mono text-xs text-muted-foreground"
          title={role.agentPubkey}
        >
          · {shortPubkey(role.agentPubkey)}
        </span>
        {rename && !editing ? (
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
          className="flex flex-wrap items-center gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <Input
            aria-label={`New name for the ${role.role} agent`}
            className="max-w-xs"
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

/** Each installed identity with its current name, role and short pubkey. */
export function ProjectTeamSetupInstalledRoles({
  roles,
}: {
  roles: readonly ProjectTeamSetupInstalledRole[];
}) {
  return (
    <div className="space-y-1" data-testid="project-team-setup-installed-roles">
      <p className="text-sm font-medium">Roles installed on this computer</p>
      {roles.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          The installation recorded no roles.
        </p>
      ) : (
        <ul className="space-y-1">
          {roles.map((role) => (
            <InstalledRoleRow key={role.agentPubkey} role={role} />
          ))}
        </ul>
      )}
    </div>
  );
}

/** Who the project lead is: name, role, project and short pubkey. */
export function ProjectTeamSetupLeadIdentity({
  activation,
  projectName,
}: {
  activation: ProjectTeamSetupActivation;
  projectName?: string;
}) {
  const { names } = useProjectTeamSetupAgents();
  const pubkey = activation.lead.leadPubkey ?? null;
  if (!pubkey)
    return (
      <p className="text-sm text-muted-foreground">
        No lead identity is recorded for this installation.
      </p>
    );
  const role =
    activation.installation.installedRoles.find(
      (entry) => entry.agentPubkey.toLowerCase() === pubkey.toLowerCase(),
    )?.role ?? "lead";
  return (
    <p className="text-sm" data-testid="project-team-setup-lead-identity">
      Project lead:{" "}
      <span className="font-medium">
        {agentName(names, pubkey) ?? NAME_UNAVAILABLE}
      </span>{" "}
      · {role}
      {projectName ? ` · ${projectName}` : ""} ·{" "}
      <span className="font-mono text-xs" title={pubkey}>
        {shortPubkey(pubkey)}
      </span>
    </p>
  );
}
