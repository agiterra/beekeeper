import * as React from "react";
import { FolderGit2, Plus, Terminal, X } from "lucide-react";

import { Button } from "@/shared/ui/button";
import { Switch } from "@/shared/ui/switch";
import { cn } from "@/shared/lib/cn";
import {
  SettingsOptionGroup,
  SettingsOptionRow,
} from "@/features/settings/ui/SettingsOptionGroup";
import { SettingsSectionHeader } from "@/features/settings/ui/SettingsSectionHeader";
import {
  closeShellSession,
  createShellSession,
  setShellPersistenceEnabled,
  shellPersistenceEnabled,
  shellWorkspaceId,
  type ShellSessionInfo,
} from "@/shared/api/tauriShell";
import { useAgentConsent } from "../hooks/useAgentConsent";
import { useSessionConsent } from "../hooks/useSessionConsent";

import {
  upsertShellSession,
  useShellSessions,
} from "../hooks/useShellSessions";

/**
 * Settings → Shell (the "Built-in Shell" experiment). Terminal sessions hosted
 * inside Buzz, with the same per-session control surface as cmux sessions:
 * "Interact" is the owner's consent to type into the terminal (granted
 * automatically for shells the owner creates, revocable here), and "Agents" is
 * the separate default-off consent for buzz agents to drive the session
 * through the session broker (`buzz session` CLI, workspace id
 * `shell:<sessionId>`).
 */
export function BuiltinShellSettingsCard() {
  const { sessions, loading, refresh } = useShellSessions();
  const { isConsented, grant, revoke } = useSessionConsent();
  const { isAgentConsented, setAgentConsented } = useAgentConsent();
  const [creating, setCreating] = React.useState(false);

  const newShell = React.useCallback(() => {
    if (creating) return;
    setCreating(true);
    createShellSession()
      .then((info) => {
        grant(shellWorkspaceId(info.sessionId));
        upsertShellSession(info);
      })
      .catch(() => {
        // Backend unavailable (e.g. browser preview).
      })
      .finally(() => setCreating(false));
  }, [creating, grant]);

  return (
    <div>
      <SettingsSectionHeader
        title="Built-in shell"
        description="Terminal sessions hosted inside Buzz. Agents reach these sessions through the local session broker, gated by per-session consent."
        action={
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={creating}
            onClick={newShell}
            data-testid="builtin-shell-settings-new"
          >
            <Plus className="mr-2 size-4" />
            New shell
          </Button>
        }
      />

      <PersistenceToggle />

      <div className="mt-8">
        <h3 className="mb-3 text-sm font-medium text-muted-foreground">
          Sessions
        </h3>
        {loading ? (
          <p className="text-sm text-muted-foreground">Loading sessions…</p>
        ) : sessions.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No shell sessions are running.
          </p>
        ) : (
          <SettingsOptionGroup>
            {sessions.map((session) => (
              <ShellSessionRow
                key={session.sessionId}
                session={session}
                interactive={isConsented(shellWorkspaceId(session.sessionId))}
                onToggleInteractive={(allowed) =>
                  allowed
                    ? grant(shellWorkspaceId(session.sessionId))
                    : revoke(shellWorkspaceId(session.sessionId))
                }
                agentAllowed={isAgentConsented(
                  shellWorkspaceId(session.sessionId),
                )}
                onToggleAgentAllowed={(allowed) =>
                  setAgentConsented(
                    shellWorkspaceId(session.sessionId),
                    allowed,
                  )
                }
                onClose={() => {
                  void closeShellSession(session.sessionId)
                    .catch(() => {
                      // Already gone.
                    })
                    .then(refresh);
                }}
              />
            ))}
          </SettingsOptionGroup>
        )}
      </div>
    </div>
  );
}

/**
 * Toggle for persisting sessions across restarts (default on). When on, each
 * session's history + working directory is checkpointed to disk, and sessions
 * killed by a reboot or app update come back as restorable entries. Turning it
 * off purges any saved history.
 */
function PersistenceToggle() {
  const [enabled, setEnabled] = React.useState<boolean | null>(null);

  React.useEffect(() => {
    shellPersistenceEnabled()
      .then(setEnabled)
      .catch(() => setEnabled(true));
  }, []);

  const toggle = React.useCallback((next: boolean) => {
    setEnabled(next);
    setShellPersistenceEnabled(next).catch(() => {
      // Revert on failure.
      shellPersistenceEnabled()
        .then(setEnabled)
        .catch(() => {});
    });
  }, []);

  return (
    <SettingsOptionGroup>
      <SettingsOptionRow>
        <div>
          <p className="text-sm font-medium">
            Persist sessions across restarts
          </p>
          <p className="text-2xs text-muted-foreground">
            Save each session's history and working directory. After a reboot or
            app update, killed sessions come back — reopen one to respawn a
            shell where it left off.
          </p>
        </div>
        <Switch
          checked={enabled ?? true}
          disabled={enabled === null}
          onCheckedChange={toggle}
          aria-label="Persist sessions across restarts"
          data-testid="builtin-shell-persist-toggle"
        />
      </SettingsOptionRow>
    </SettingsOptionGroup>
  );
}

function ShellSessionRow({
  session,
  interactive,
  onToggleInteractive,
  agentAllowed,
  onToggleAgentAllowed,
  onClose,
}: {
  session: ShellSessionInfo;
  interactive: boolean;
  onToggleInteractive: (allowed: boolean) => void;
  agentAllowed: boolean;
  onToggleAgentAllowed: (allowed: boolean) => void;
  onClose: () => void;
}) {
  return (
    <SettingsOptionRow data-testid="builtin-shell-session-row">
      <div className="flex min-w-0 items-center gap-3">
        <Terminal className="size-4 shrink-0 text-muted-foreground" />
        <div className="min-w-0">
          <p className="truncate font-medium">{session.title}</p>
          <p className="flex items-center gap-1 truncate text-2xs text-muted-foreground">
            <FolderGit2 className="size-3 shrink-0" />
            {session.currentDirectory}
          </p>
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-4">
        <span
          className={cn(
            "rounded-full border px-2 py-0.5 text-2xs font-medium",
            session.running
              ? "border-border bg-muted text-muted-foreground"
              : "border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-300",
          )}
        >
          {session.running ? "Running" : "Exited"}
        </span>
        <ToggleWithLabel
          label="Interact"
          hint="Allow you to type into this session"
          checked={interactive}
          onCheckedChange={onToggleInteractive}
          testId="builtin-shell-session-interactive"
        />
        <ToggleWithLabel
          label="Agents"
          hint="Allow buzz agents to drive this session"
          checked={agentAllowed}
          onCheckedChange={onToggleAgentAllowed}
          testId="builtin-shell-session-agent"
        />
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label="Close session"
          onClick={onClose}
          data-testid="builtin-shell-session-close"
        >
          <X className="size-4" />
        </Button>
      </div>
    </SettingsOptionRow>
  );
}

function ToggleWithLabel({
  label,
  hint,
  checked,
  onCheckedChange,
  testId,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  testId: string;
}) {
  return (
    <div className="flex flex-col items-center gap-1" title={hint}>
      <span className="text-3xs uppercase tracking-wide text-muted-foreground">
        {label}
      </span>
      <Switch
        checked={checked}
        onCheckedChange={onCheckedChange}
        aria-label={hint ?? label}
        data-testid={testId}
      />
    </div>
  );
}
