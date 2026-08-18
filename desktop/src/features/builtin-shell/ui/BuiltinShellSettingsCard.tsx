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
  type ShellSessionInfo,
} from "@/shared/api/tauriShell";

import {
  upsertShellSession,
  useShellSessions,
} from "../hooks/useShellSessions";

/**
 * Settings → Terminals (the "Built-in Shell" experiment): the persistence
 * toggle and a slim session list (status + close). Sharing and access —
 * project-wide watching and the collaborator/viewer invite roster — are
 * managed on each session's screen, not here.
 */
export function BuiltinShellSettingsCard() {
  const { sessions, loading, refresh } = useShellSessions();
  const [creating, setCreating] = React.useState(false);

  const newShell = React.useCallback(() => {
    if (creating) return;
    setCreating(true);
    createShellSession()
      .then((info) => {
        upsertShellSession(info);
      })
      .catch(() => {
        // Backend unavailable (e.g. browser preview).
      })
      .finally(() => setCreating(false));
  }, [creating]);

  return (
    <div>
      <SettingsSectionHeader
        title="Built-in shell"
        description="Terminal sessions hosted inside Buzz."
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
        <p className="mb-3 text-2xs text-muted-foreground">
          Sharing and access are managed on each session&rsquo;s screen.
        </p>
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
  onClose,
}: {
  session: ShellSessionInfo;
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
