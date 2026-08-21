import * as React from "react";
import { useNavigate } from "@tanstack/react-router";
import { Eye, Plus, Terminal } from "lucide-react";

import { useUsersBatchQuery } from "@/features/profile/hooks";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { Button } from "@/shared/ui/button";

import { useCreateShellSession } from "../hooks/useCreateShellSession";
import { useShellSessions } from "../hooks/useShellSessions";
import { projectDefaultCwd, type ShellCwdRepo } from "../lib/projectShellCwd";
import { useProjectTerminals } from "../observe/useProjectTerminals";

/**
 * The "Terminals" section of a project screen: the viewer's own sessions in
 * this project (open the interactive terminal) plus other members' shared
 * sessions from their NIP-ST announces (open the read-only observer). Mount
 * behind the builtin-shell feature gate.
 */
export function ProjectTerminalsCard({
  projectAddress,
  isFallback,
  repos = [],
}: {
  /** The project's `30621:<owner>:<dtag>` address, or null for the local
   * General placeholder (nothing is announced there). */
  projectAddress: string | null;
  /** True for the local General bucket — sessions there are unassigned. */
  isFallback: boolean;
  /** The project's repositories, for the new-terminal default cwd (the first
   * repo with a local checkout wins). */
  repos?: readonly ShellCwdRepo[];
}) {
  const navigate = useNavigate();
  const { sessions } = useShellSessions();
  const { createFor } = useCreateShellSession();
  const remote = useProjectTerminals(projectAddress);

  const createTerminal = React.useCallback(() => {
    void projectDefaultCwd(repos).then((cwd) =>
      createFor(projectAddress ?? undefined, cwd),
    );
  }, [repos, createFor, projectAddress]);

  const own = React.useMemo(
    () =>
      sessions.filter((session) =>
        projectAddress
          ? session.projectRef === projectAddress
          : isFallback && !session.projectRef,
      ),
    [sessions, projectAddress, isFallback],
  );
  const remoteTerminals = remote.data ?? [];

  const ownerPubkeys = React.useMemo(
    () => [...new Set(remoteTerminals.map((t) => t.ownerPubkey.toLowerCase()))],
    [remoteTerminals],
  );
  const owners = useUsersBatchQuery(ownerPubkeys);
  const ownerLabel = (pubkey: string) => {
    const profile = owners.data?.profiles[pubkey.toLowerCase()];
    return profile?.displayName ?? profile?.name ?? truncatePubkey(pubkey);
  };

  const count = own.length + remoteTerminals.length;

  return (
    <section
      className="rounded-lg border border-border bg-card p-4"
      data-testid="project-terminals-card"
    >
      <div className="mb-3 flex items-center gap-2 text-sm font-medium text-foreground">
        <Terminal className="size-4" />
        <span>Terminals</span>
        <span className="text-2xs text-muted-foreground">{count}</span>
        <span className="flex-1" />
        <Button
          aria-label="New terminal"
          data-testid="project-terminals-new"
          onClick={createTerminal}
          size="icon-xs"
          variant="ghost"
        >
          <Plus className="size-4" />
        </Button>
      </div>
      {count === 0 ? (
        <p className="text-sm text-muted-foreground">
          No open terminals in this project.
        </p>
      ) : (
        <ul className="flex flex-col gap-1">
          {own.map((session) => (
            <li key={session.sessionId}>
              <Button
                className="h-8 w-full justify-start gap-2 px-2"
                data-testid="project-terminal-own-row"
                onClick={() =>
                  void navigate({
                    to: "/shell/$sessionId",
                    params: { sessionId: session.sessionId },
                  })
                }
                variant="ghost"
              >
                <Terminal className="size-4 shrink-0 text-muted-foreground" />
                <span className="truncate">{session.title}</span>
                {!session.running ? (
                  <span className="text-2xs text-muted-foreground">
                    {session.restorable ? "(paused)" : "(exited)"}
                  </span>
                ) : null}
              </Button>
            </li>
          ))}
          {remoteTerminals.map((terminal) => (
            <li key={`${terminal.ownerPubkey}:${terminal.sessionId}`}>
              <Button
                className="h-8 w-full justify-start gap-2 px-2"
                data-testid="project-terminal-remote-row"
                onClick={() =>
                  void navigate({
                    to: "/observe/$owner/$sessionId",
                    params: {
                      owner: terminal.ownerPubkey,
                      sessionId: terminal.sessionId,
                    },
                    search: { project: terminal.projectRef },
                  })
                }
                variant="ghost"
              >
                <Eye className="size-4 shrink-0 text-muted-foreground" />
                <span className="truncate">{terminal.title}</span>
                <span className="truncate text-2xs text-muted-foreground">
                  · {ownerLabel(terminal.ownerPubkey)}
                </span>
              </Button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
