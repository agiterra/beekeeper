import * as React from "react";
import { useNavigate } from "@tanstack/react-router";
import { EllipsisVertical, Eye, Plus, Terminal, Trash2 } from "lucide-react";

import { useUsersBatchQuery } from "@/features/profile/hooks";
import { useIdentityQuery } from "@/shared/api/hooks";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

import { useCreateShellSession } from "../hooks/useCreateShellSession";
import { useShellSessions } from "../hooks/useShellSessions";
import { projectDefaultCwd, type ShellCwdRepo } from "../lib/projectShellCwd";
import { useProjectTerminals } from "../observe/useProjectTerminals";
import {
  useDeleteTerminalDialog,
  type DeletableTerminal,
} from "./useDeleteTerminalDialog";

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
  canDelete,
}: {
  /** The project's `30621:<owner>:<dtag>` address, or null for the local
   * General placeholder (nothing is announced there). */
  projectAddress: string | null;
  /** True for the local General bucket — sessions there are unassigned. */
  isFallback: boolean;
  /** The project's repositories, for the new-terminal default cwd (the first
   * repo with a local checkout wins). */
  repos?: readonly ShellCwdRepo[];
  /**
   * Whether the viewer may delete the shared announce of a terminal owned by
   * `ownerPubkey`. Passed in rather than resolved here so this card stays
   * ignorant of the project roster: the caller already holds the project's
   * capabilities, and a terminal's delete rule is the project's rule
   * (`canDeleteResource` — an Owner reaches anything, everyone else reaches
   * only their own). Omitted means no delete affordance at all, which is
   * what the local General placeholder gets.
   */
  canDelete?: (ownerPubkey: string) => boolean;
}) {
  const navigate = useNavigate();
  const { sessions } = useShellSessions();
  const { createFor } = useCreateShellSession();
  const remote = useProjectTerminals(projectAddress);
  const identity = useIdentityQuery();
  // Own rows come from the local session manager, which does not carry a
  // pubkey — the announce for one is addressed to this identity.
  const selfPubkey = identity.data?.pubkey?.toLowerCase() ?? null;
  const { requestDelete, dialog: deleteDialog } =
    useDeleteTerminalDialog(projectAddress);

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

  // The announce is what a delete removes, so a session that was never
  // shared has nothing to delete: `own` rows exist for local sessions too,
  // and those carry no coordinate. `projectAddress` being set is exactly the
  // condition under which one was announced.
  const deleteMenu = (terminal: DeletableTerminal) =>
    projectAddress && canDelete?.(terminal.ownerPubkey) ? (
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            aria-label={`Actions for ${terminal.title}`}
            className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground/60 opacity-0 transition-colors hover:text-foreground focus-visible:opacity-100 group-hover/terminal-row:opacity-100 data-[state=open]:opacity-100"
            data-testid="project-terminal-actions"
            type="button"
          >
            <EllipsisVertical className="size-4" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem
            className="text-destructive focus:text-destructive"
            data-testid="project-terminal-delete"
            onSelect={() => requestDelete(terminal)}
          >
            <Trash2 />
            Delete terminal
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    ) : null;

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
            <li
              className="group/terminal-row flex items-center gap-1"
              key={session.sessionId}
            >
              <Button
                className="h-8 min-w-0 flex-1 justify-start gap-2 px-2"
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
              {deleteMenu({
                sessionId: session.sessionId,
                ownerPubkey: selfPubkey ?? "",
                title: session.title,
                isOwn: true,
              })}
            </li>
          ))}
          {remoteTerminals.map((terminal) => (
            <li
              className="group/terminal-row flex items-center gap-1"
              key={`${terminal.ownerPubkey}:${terminal.sessionId}`}
            >
              <Button
                className="h-8 min-w-0 flex-1 justify-start gap-2 px-2"
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
              {deleteMenu({
                sessionId: terminal.sessionId,
                ownerPubkey: terminal.ownerPubkey,
                title: terminal.title,
                isOwn: false,
              })}
            </li>
          ))}
        </ul>
      )}
      {deleteDialog}
    </section>
  );
}
