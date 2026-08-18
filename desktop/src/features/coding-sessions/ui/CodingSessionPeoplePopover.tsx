import { ChevronDown, Users } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { PersonaShareRecipients } from "@/features/agents/ui/PersonaShareRecipients";
import { useUsersBatchQuery } from "@/features/profile/hooks";
import { ProfileAvatar } from "@/features/profile/ui/ProfileAvatar";
import {
  useCodingSessionGrantMutation,
  useCodingSessionRevokeMutation,
  useCodingSessionRoster,
  type CodingSessionRosterEntry,
} from "@/features/coding-sessions/lib/codingSessionRoster";
import { useIdentityQuery } from "@/shared/api/hooks";
import type { UserSearchResult } from "@/shared/api/types";
import {
  ENTITY_ROLE_DESCRIPTIONS,
  ENTITY_ROLE_LABELS,
  entityRoleRank,
  SESSION_GRANTABLE_ROLES,
  type EntityRole,
} from "@/shared/lib/entityRoles";
import { truncatePubkey } from "@/shared/lib/pubkey";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/shared/ui/alert-dialog";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

/** Invitees one grant round can hold. Each becomes its own chain link, so
 * the batch stays small enough that a mid-batch failure reads clearly. */
const INVITE_LIMIT = 8;

/**
 * The session People surface: the authority-chain roster (founder pinned as
 * Owner, live grants, pending invites) with owner-only management — invite
 * with a role over {@link SESSION_GRANTABLE_ROLES}, and per-row revoke.
 * Non-owners see the same roster read-only. All state is relay-derived:
 * grants are kind:44228 chain links, acceptance is the relay's kind:40099
 * receipt, and a row shows "Inviting…" until its receipt lands.
 */
export function CodingSessionPeoplePopover({
  channelId,
  founderPubkey,
  genesisRef,
  onOpenChange,
  open,
}: {
  channelId: string;
  founderPubkey: string | null;
  genesisRef: string | null;
  onOpenChange: (open: boolean) => void;
  open: boolean;
}) {
  const identityQuery = useIdentityQuery();
  const self = identityQuery.data?.pubkey?.toLowerCase();
  const isOwner =
    !!self && founderPubkey !== null && self === founderPubkey.toLowerCase();

  const rosterQuery = useCodingSessionRoster(
    channelId,
    genesisRef,
    founderPubkey,
  );
  const entries = React.useMemo(
    () => rosterQuery.data ?? [],
    [rosterQuery.data],
  );

  const memberPubkeys = React.useMemo(
    () => entries.map((entry) => entry.pubkey),
    [entries],
  );
  const profilesQuery = useUsersBatchQuery(memberPubkeys);
  const profiles = profilesQuery.data?.profiles;

  const displayName = React.useCallback(
    (pubkey: string) =>
      profiles?.[pubkey]?.displayName?.trim() || truncatePubkey(pubkey),
    [profiles],
  );

  const sortedEntries = React.useMemo(
    () =>
      [...entries].sort((a, b) => {
        // Owner pinned first, live grants before pending invites, then by
        // descending capability, then name.
        if ((a.role === "owner") !== (b.role === "owner")) {
          return a.role === "owner" ? -1 : 1;
        }
        if (!!a.pending !== !!b.pending) return a.pending ? 1 : -1;
        const rank = entityRoleRank(a.role) - entityRoleRank(b.role);
        if (rank !== 0) return rank;
        return displayName(a.pubkey).localeCompare(displayName(b.pubkey));
      }),
    [entries, displayName],
  );

  const grantMutation = useCodingSessionGrantMutation(channelId, genesisRef);
  const revokeMutation = useCodingSessionRevokeMutation(channelId, genesisRef);

  const [inviteUsers, setInviteUsers] = React.useState<UserSearchResult[]>([]);
  const [inviteRole, setInviteRole] =
    React.useState<EntityRole>("collaborator");
  const [revokeTarget, setRevokeTarget] =
    React.useState<CodingSessionRosterEntry | null>(null);

  React.useEffect(() => {
    if (open) return;
    setInviteUsers([]);
    setInviteRole("collaborator");
  }, [open]);

  const invite = async () => {
    if (inviteUsers.length === 0) return;
    try {
      // Sequential on purpose: each grant is one authority-chain link, so a
      // parallel batch would race itself on the chain head.
      for (const user of inviteUsers) {
        await grantMutation.mutateAsync({
          pubkey: user.pubkey,
          role: inviteRole,
        });
      }
      toast.success(
        inviteUsers.length === 1
          ? "Invite sent."
          : `${inviteUsers.length} invites sent.`,
      );
      setInviteUsers([]);
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : "Failed to send the invite.",
      );
    }
  };

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent className="max-w-md" data-testid="coding-session-people">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Users aria-hidden className="size-4" />
            People
          </DialogTitle>
          <DialogDescription>
            {isOwner
              ? "Share this session. Collaborators may steer it; viewers follow along read-only."
              : "Who has access to this session. Only the session owner can change it."}
          </DialogDescription>
        </DialogHeader>

        {rosterQuery.isPending ? (
          <p className="py-4 text-center text-sm text-muted-foreground">
            Loading session access…
          </p>
        ) : rosterQuery.isError ? (
          <p className="py-4 text-center text-sm text-muted-foreground">
            Could not load the session roster.
          </p>
        ) : sortedEntries.length === 0 ? (
          <p className="py-4 text-center text-sm text-muted-foreground">
            No one has access to this session yet.
          </p>
        ) : (
          <ul className="flex max-h-72 flex-col gap-1 overflow-y-auto">
            {sortedEntries.map((entry) => {
              const profile = profiles?.[entry.pubkey];
              const name = displayName(entry.pubkey);
              const isOwnerRow = entry.role === "owner";
              return (
                <li
                  className="flex min-h-8 items-center gap-2 px-1"
                  data-testid={`coding-session-people-row-${entry.pubkey}`}
                  key={entry.pubkey}
                >
                  <ProfileAvatar
                    avatarUrl={profile?.avatarUrl ?? null}
                    className="h-6 w-6 text-2xs shadow-none"
                    iconClassName="h-3 w-3"
                    label={name}
                  />
                  <span className="min-w-0 flex-1 truncate text-sm">
                    {name}
                  </span>
                  {profile?.isAgent ? (
                    <Badge variant="outline">Agent</Badge>
                  ) : null}
                  {entry.pending ? (
                    <span
                      className="shrink-0 text-xs text-muted-foreground italic"
                      title="Waiting for the relay's acceptance receipt"
                    >
                      Inviting…
                    </span>
                  ) : isOwnerRow ? (
                    <Badge variant="secondary">
                      {ENTITY_ROLE_LABELS.owner}
                    </Badge>
                  ) : isOwner ? (
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <button
                          aria-label={`Manage access for ${name}`}
                          className="flex shrink-0 items-center gap-1 rounded-md px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                          data-testid={`coding-session-people-menu-${entry.pubkey}`}
                          type="button"
                        >
                          {ENTITY_ROLE_LABELS[entry.role]}
                          <ChevronDown className="size-3" />
                        </button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuItem
                          className="text-destructive focus:text-destructive"
                          data-testid={`coding-session-people-revoke-${entry.pubkey}`}
                          onSelect={() => setRevokeTarget(entry)}
                        >
                          Revoke access
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  ) : (
                    <span className="shrink-0 text-xs text-muted-foreground">
                      {ENTITY_ROLE_LABELS[entry.role]}
                    </span>
                  )}
                </li>
              );
            })}
          </ul>
        )}

        {isOwner ? (
          <div className="flex flex-col gap-3 border-t border-border/60 pt-4">
            <p className="text-sm font-medium">Invite</p>
            <PersonaShareRecipients
              allowDirectPubkeyEntry
              disabled={grantMutation.isPending}
              excludedPubkeys={memberPubkeys}
              limit={INVITE_LIMIT}
              onSelectionChange={setInviteUsers}
              open={open}
              selectedUsers={inviteUsers}
              testIdPrefix="coding-session-people-invite"
            />
            <div className="flex items-center justify-between gap-4">
              <span className="text-sm text-muted-foreground">Role</span>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    className="flex items-center gap-1 rounded-md border border-input px-3 py-1.5 text-sm transition-colors hover:bg-muted"
                    data-testid="coding-session-people-invite-role"
                    type="button"
                  >
                    {ENTITY_ROLE_LABELS[inviteRole]}
                    <ChevronDown className="size-3" />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  {SESSION_GRANTABLE_ROLES.map((role) => (
                    <DropdownMenuItem
                      data-testid={`coding-session-people-role-${role}`}
                      key={role}
                      onSelect={() => setInviteRole(role)}
                    >
                      <div className="flex flex-col gap-0.5">
                        <span className="text-sm">
                          {ENTITY_ROLE_LABELS[role]}
                          {role === inviteRole ? " ✓" : ""}
                        </span>
                        <span className="text-xs text-muted-foreground">
                          {ENTITY_ROLE_DESCRIPTIONS[role]}
                        </span>
                      </div>
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
            <Button
              className="self-end"
              data-testid="coding-session-people-invite"
              disabled={grantMutation.isPending || inviteUsers.length === 0}
              onClick={() => void invite()}
              type="button"
            >
              {grantMutation.isPending ? "Inviting…" : "Invite"}
            </Button>
          </div>
        ) : null}

        <AlertDialog
          onOpenChange={(alertOpen) => {
            if (!alertOpen) setRevokeTarget(null);
          }}
          open={revokeTarget !== null}
        >
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Revoke access?</AlertDialogTitle>
              <AlertDialogDescription>
                {revokeTarget
                  ? `${displayName(revokeTarget.pubkey)} will lose ${
                      revokeTarget.role === "collaborator"
                        ? "collaborator"
                        : "viewer"
                    } access to this session.`
                  : ""}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel disabled={revokeMutation.isPending}>
                Cancel
              </AlertDialogCancel>
              <AlertDialogAction
                data-testid="coding-session-people-revoke-confirm"
                disabled={revokeMutation.isPending}
                onClick={(event) => {
                  event.preventDefault();
                  if (!revokeTarget) return;
                  revokeMutation.mutate(
                    { pubkey: revokeTarget.pubkey },
                    {
                      onSuccess: () => {
                        toast.success("Access revoked.");
                        setRevokeTarget(null);
                      },
                      onError: (error) => {
                        toast.error(
                          error instanceof Error
                            ? error.message
                            : "Failed to revoke access.",
                        );
                      },
                    },
                  );
                }}
              >
                Revoke
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </DialogContent>
    </Dialog>
  );
}
