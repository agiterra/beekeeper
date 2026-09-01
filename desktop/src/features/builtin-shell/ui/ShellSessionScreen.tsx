import * as React from "react";
import { useCanGoBack, useNavigate, useRouter } from "@tanstack/react-router";
import { listen } from "@tauri-apps/api/event";
import { ChevronDown, Eye, FolderGit2, Users, X } from "lucide-react";
import { toast } from "sonner";

import { PersonaShareRecipients } from "@/features/agents/ui/PersonaShareRecipients";
import { useUsersBatchQuery } from "@/features/profile/hooks";
import { ProfileAvatar } from "@/features/profile/ui/ProfileAvatar";
import type { UserSearchResult } from "@/shared/api/types";
import {
  ENTITY_ROLE_DESCRIPTIONS,
  ENTITY_ROLE_LABELS,
  SESSION_GRANTABLE_ROLES,
  type EntityRole,
} from "@/shared/lib/entityRoles";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";
import { Spinner } from "@/shared/ui/spinner";
import { Switch } from "@/shared/ui/switch";
import {
  SHELL_BROADCAST_WATCHERS_EVENT,
  closeShellSession,
  resumeShellSession,
  setShellSessionRoster,
  setShellSessionShared,
  shellBroadcastWatchers,
  type ShellRosterEntry,
  type ShellSessionInfo,
} from "@/shared/api/tauriShell";

import {
  upsertShellSession,
  useShellSessions,
} from "../hooks/useShellSessions";
import { ShellTerminal } from "./ShellTerminal";

/** Roster size cap, matching the relay's ingest limit for the announce. */
const ROSTER_LIMIT = 64;

/** Live "who is watching" roster for one session (NIP-ST owner indicator). */
function useShellWatchers(sessionId: string): string[] {
  const [watchers, setWatchers] = React.useState<string[]>([]);
  React.useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    shellBroadcastWatchers(sessionId)
      .then((initial) => {
        if (!disposed) setWatchers(initial);
      })
      .catch(() => {});
    void listen<{ sessionId: string; watchers: string[] }>(
      SHELL_BROADCAST_WATCHERS_EVENT,
      (event) => {
        if (event.payload.sessionId === sessionId) {
          setWatchers(event.payload.watchers);
        }
      },
    )
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
      setWatchers([]);
    };
  }, [sessionId]);
  return watchers;
}

function SessionRoleMenuItems({
  currentRole,
  onSelect,
}: {
  currentRole?: EntityRole;
  onSelect: (role: EntityRole) => void;
}) {
  return (
    <>
      {SESSION_GRANTABLE_ROLES.map((role) => (
        <DropdownMenuItem
          data-testid={`terminal-people-role-${role}`}
          key={role}
          onSelect={() => onSelect(role)}
        >
          <div className="flex flex-col gap-0.5">
            <span className="text-sm">
              {ENTITY_ROLE_LABELS[role]}
              {role === currentRole ? " ✓" : ""}
            </span>
            <span className="text-xs text-muted-foreground">
              {ENTITY_ROLE_DESCRIPTIONS[role]}
            </span>
          </div>
        </DropdownMenuItem>
      ))}
    </>
  );
}

/**
 * The session's sharing control surface: a "People" popover with the
 * project-wide watch switch, the invite roster (role menus + remove + live
 * presence dots), and the invite picker. Mirrors the project Members card's
 * patterns; writes go through `set_shell_session_shared` /
 * `set_shell_session_roster`, whose refreshed announce is what observers see.
 */
function SessionPeoplePopover({
  session,
  watchers,
  onSessionUpdated,
}: {
  session: ShellSessionInfo;
  watchers: string[];
  onSessionUpdated: (info: ShellSessionInfo) => void;
}) {
  const shared = session.shared ?? true;
  const roster = React.useMemo(() => session.roster ?? [], [session.roster]);

  const [inviteUsers, setInviteUsers] = React.useState<UserSearchResult[]>([]);
  const [inviteRole, setInviteRole] =
    React.useState<EntityRole>("collaborator");
  const [open, setOpen] = React.useState(false);
  const [pending, setPending] = React.useState(false);

  React.useEffect(() => {
    if (open) return;
    setInviteUsers([]);
    setInviteRole("collaborator");
  }, [open]);

  const rosterPubkeys = React.useMemo(
    () => roster.map((entry) => entry.pubkey),
    [roster],
  );
  const profilesQuery = useUsersBatchQuery(rosterPubkeys);
  const profiles = profilesQuery.data?.profiles;
  const watcherSet = React.useMemo(
    () => new Set(watchers.map((w) => w.toLowerCase())),
    [watchers],
  );

  const displayName = React.useCallback(
    (pubkey: string) =>
      profiles?.[pubkey]?.displayName?.trim() || truncatePubkey(pubkey),
    [profiles],
  );

  const putRoster = React.useCallback(
    (next: ShellRosterEntry[]) => {
      setPending(true);
      setShellSessionRoster(session.sessionId, next)
        .then(() => onSessionUpdated({ ...session, roster: next }))
        .catch((error) => {
          toast.error(
            error instanceof Error
              ? error.message
              : "Failed to update the session's people.",
          );
        })
        .finally(() => setPending(false));
    },
    [session, onSessionUpdated],
  );

  const toggleShared = React.useCallback(
    (next: boolean) => {
      setShellSessionShared(session.sessionId, next)
        .then(() => onSessionUpdated({ ...session, shared: next }))
        .catch((error) => {
          toast.error(
            error instanceof Error
              ? error.message
              : "Failed to update sharing.",
          );
        });
    },
    [session, onSessionUpdated],
  );

  const invite = React.useCallback(() => {
    if (inviteUsers.length === 0) return;
    const additions: ShellRosterEntry[] = inviteUsers.map((user) => ({
      pubkey: user.pubkey.toLowerCase(),
      role: inviteRole === "viewer" ? "viewer" : "collaborator",
    }));
    const merged = [
      ...roster.filter(
        (entry) => !additions.some((a) => a.pubkey === entry.pubkey),
      ),
      ...additions,
    ];
    putRoster(merged);
    setInviteUsers([]);
  }, [inviteUsers, inviteRole, roster, putRoster]);

  const changeRole = React.useCallback(
    (pubkey: string, role: EntityRole) => {
      putRoster(
        roster.map((entry) =>
          entry.pubkey === pubkey
            ? { ...entry, role: role === "viewer" ? "viewer" : "collaborator" }
            : entry,
        ),
      );
    },
    [roster, putRoster],
  );

  const remove = React.useCallback(
    (pubkey: string) => {
      putRoster(roster.filter((entry) => entry.pubkey !== pubkey));
    },
    [roster, putRoster],
  );

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="outline"
          size="sm"
          data-testid="terminal-people"
          title="Manage who can watch or type in this terminal"
        >
          <Users className="mr-2 size-4" />
          People
          {roster.length > 0 ? (
            <span className="ml-1.5 rounded-full bg-muted px-1.5 text-2xs text-muted-foreground">
              {roster.length}
            </span>
          ) : null}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-96 p-4">
        <div className="flex flex-col gap-4">
          <div className="flex items-center justify-between gap-4">
            <div>
              <p className="text-sm font-medium">Project members can watch</p>
              <p className="text-2xs text-muted-foreground">
                Anyone in this project may observe read-only. Off, only the
                people invited below have access.
              </p>
            </div>
            <Switch
              checked={shared}
              onCheckedChange={toggleShared}
              aria-label="Project members can watch"
              data-testid="terminal-people-shared"
            />
          </div>

          <div className="flex flex-col gap-1">
            <p className="text-2xs font-medium uppercase tracking-wide text-muted-foreground">
              Invited
            </p>
            {roster.length === 0 ? (
              <p className="text-xs text-muted-foreground">
                No one is invited. Invite someone to give them access even when
                the terminal is private — collaborators can type.
              </p>
            ) : (
              <ul className="flex flex-col gap-1">
                {roster.map((entry) => {
                  const profile = profiles?.[entry.pubkey];
                  const name = displayName(entry.pubkey);
                  const watching = watcherSet.has(entry.pubkey);
                  return (
                    <li
                      className="flex min-h-8 items-center gap-2"
                      data-testid={`terminal-people-row-${entry.pubkey}`}
                      key={entry.pubkey}
                    >
                      <div className="relative">
                        <ProfileAvatar
                          avatarUrl={profile?.avatarUrl ?? null}
                          className="h-6 w-6 text-2xs shadow-none"
                          iconClassName="h-3 w-3"
                          label={name}
                        />
                        {watching ? (
                          <span
                            className="absolute -bottom-0.5 -right-0.5 size-2 rounded-full border border-background bg-emerald-500"
                            data-testid={`terminal-people-watching-${entry.pubkey}`}
                            title="Watching now"
                          />
                        ) : null}
                      </div>
                      <span className="min-w-0 flex-1 truncate text-sm">
                        {name}
                      </span>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <button
                            aria-label={`Change role for ${name}`}
                            className="flex shrink-0 items-center gap-1 rounded-md px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                            data-testid={`terminal-people-role-menu-${entry.pubkey}`}
                            disabled={pending}
                            type="button"
                          >
                            {
                              ENTITY_ROLE_LABELS[
                                entry.role === "viewer"
                                  ? "viewer"
                                  : "collaborator"
                              ]
                            }
                            <ChevronDown className="size-3" />
                          </button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <SessionRoleMenuItems
                            currentRole={
                              entry.role === "viewer"
                                ? "viewer"
                                : "collaborator"
                            }
                            onSelect={(role) => changeRole(entry.pubkey, role)}
                          />
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            className="text-destructive focus:text-destructive"
                            data-testid={`terminal-people-remove-${entry.pubkey}`}
                            onSelect={() => remove(entry.pubkey)}
                          >
                            Remove
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>

          <div className="flex flex-col gap-2 border-t border-border pt-3">
            <p className="text-2xs font-medium uppercase tracking-wide text-muted-foreground">
              Invite
            </p>
            <PersonaShareRecipients
              allowDirectPubkeyEntry
              disabled={pending}
              excludedPubkeys={rosterPubkeys}
              limit={ROSTER_LIMIT}
              onSelectionChange={setInviteUsers}
              open={open}
              selectedUsers={inviteUsers}
              testIdPrefix="terminal-people-invite"
            />
            <div className="flex items-center justify-between gap-4">
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    className="flex items-center gap-1 rounded-md border border-input px-3 py-1.5 text-sm transition-colors hover:bg-muted"
                    data-testid="terminal-people-invite-role"
                    type="button"
                  >
                    {ENTITY_ROLE_LABELS[inviteRole]}
                    <ChevronDown className="size-3" />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start">
                  <SessionRoleMenuItems
                    currentRole={inviteRole}
                    onSelect={setInviteRole}
                  />
                </DropdownMenuContent>
              </DropdownMenu>
              <Button
                type="button"
                size="sm"
                data-testid="terminal-people-invite-confirm"
                disabled={pending || inviteUsers.length === 0}
                onClick={invite}
              >
                Invite
              </Button>
            </div>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}

/**
 * A dark, terminal-colored placeholder with a spinner for the moments where a
 * session is expected to appear shortly (just created, or resuming from disk)
 * but isn't attached yet — reads as "the terminal is about to render" rather
 * than an app-chrome loading state or (worse) a false "this session has
 * ended".
 */
function ShellConnectingView({ label }: { label: string }) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 bg-[#1e1e2e]">
      <Spinner className="border-white/15 border-t-white/70" />
      <p className="text-2xs text-white/60">{label}</p>
    </div>
  );
}

/**
 * A built-in shell session in the main content area: header (title, cwd,
 * People, close) over a live xterm terminal. This is always the owner's own
 * session, so typing is always on; who else may watch or type is managed in
 * the People popover (share switch + invite roster).
 */
export function ShellSessionScreen({ sessionId }: { sessionId: string }) {
  const navigate = useNavigate();
  const router = useRouter();
  const canGoBack = useCanGoBack();
  const { sessions, loading } = useShellSessions();
  const watchers = useShellWatchers(sessionId);

  const session = React.useMemo(
    () => sessions.find((s) => s.sessionId === sessionId) ?? null,
    [sessions, sessionId],
  );

  // A restored session has history but no live shell — respawn it in its saved
  // directory the moment its screen opens, so it "just works". Guard so the
  // resume fires once even as the session list refreshes.
  const resumingRef = React.useRef(false);
  React.useEffect(() => {
    if (session?.restorable && !resumingRef.current) {
      resumingRef.current = true;
      void resumeShellSession(sessionId)
        .then(upsertShellSession)
        .catch(() => {
          // The saved directory may be gone; the backend falls back to $HOME.
          // If it still fails, allow another attempt.
          resumingRef.current = false;
        });
    }
  }, [session?.restorable, sessionId]);

  // Closing a terminal should return you where you came from, not to the
  // Dashboard. The hard `/` was only ever right because this screen used to
  // carry its own back arrow next to it; with that gone, a close that
  // discarded history would be the only way out and would lose your place.
  const close = React.useCallback(() => {
    void closeShellSession(sessionId)
      .catch(() => {
        // Already gone.
      })
      .then(() => {
        if (canGoBack) {
          router.history.back();
          return;
        }
        return navigate({ to: "/" });
      });
  }, [canGoBack, navigate, router.history, sessionId]);

  if (!session) {
    return loading ? (
      <ShellConnectingView label="Opening shell…" />
    ) : (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        This shell session has ended.
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex items-center gap-3 border-b border-border px-4 py-3">
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-semibold">{session.title}</p>
          <p className="flex items-center gap-1 truncate text-2xs text-muted-foreground">
            <FolderGit2 className="size-3 shrink-0" />
            {session.currentDirectory}
          </p>
        </div>
        {!session.running ? (
          <span className="rounded-full border border-border bg-muted px-2 py-0.5 text-2xs font-medium text-muted-foreground">
            Exited
          </span>
        ) : null}
        {watchers.length > 0 ? (
          <span
            className="flex items-center gap-1 rounded-full border border-emerald-500/40 bg-emerald-500/15 px-2 py-0.5 text-2xs font-medium text-emerald-500"
            data-testid="shell-session-watchers"
            title="People observing this terminal"
          >
            <Eye className="size-3" />
            {watchers.length} watching
          </span>
        ) : null}
        {session.projectRef ? (
          <SessionPeoplePopover
            session={session}
            watchers={watchers}
            onSessionUpdated={upsertShellSession}
          />
        ) : null}
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={close}
          data-testid="shell-session-close"
        >
          <X className="mr-2 size-4" />
          Close
        </Button>
      </header>

      {/* Mount the terminal only once the session is live. For a restorable
          session the resume above spawns the host and replays history into the
          backend first; the terminal then attaches to a populated scrollback,
          so history shows without a race. */}
      {session.running ? (
        <ShellTerminal sessionId={sessionId} />
      ) : session.restorable ? (
        <ShellConnectingView label="Resuming session…" />
      ) : (
        <div className="flex min-h-0 flex-1 items-center justify-center text-sm text-muted-foreground">
          Session ended.
        </div>
      )}
    </div>
  );
}
