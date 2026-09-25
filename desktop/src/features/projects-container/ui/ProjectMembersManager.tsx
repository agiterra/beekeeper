import { useQuery } from "@tanstack/react-query";
import { ChevronDown, UserPlus } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { PersonaShareRecipients } from "@/features/agents/ui/PersonaShareRecipients";
import { useManagedAgentsQuery } from "@/features/agents/hooks";
import { useUsersBatchQuery } from "@/features/profile/hooks";
import { ProfileAvatar } from "@/features/profile/ui/ProfileAvatar";
import type { UserSearchResult } from "@/shared/api/types";
import {
  ENTITY_ROLE_DESCRIPTIONS,
  ENTITY_ROLE_LABELS,
  entityRoleRank,
  PROJECT_GRANTABLE_ROLES,
  type EntityRole,
} from "@/shared/lib/entityRoles";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { getCodingSessionProviderStatus } from "@/shared/api/tauriSessionProvider";
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
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

import type { ProjectContainer } from "../hooks";
import { useProjectCapabilities } from "../lib/projectPermissions";
import {
  rosterWithOwner,
  usePutProjectRosterMutation,
  useProjectRosterQuery,
  useRemoveProjectRosterMutation,
  type ProjectRosterEntry,
} from "../lib/projectMembers";
import { projectMemberIdentity } from "../lib/projectMemberIdentity";
import { EmptyHint } from "./SectionCard";

/** Members the add dialog can hold at once — well above the picker's default
 * chip limit, so a real team fits in one put op. */
const ADD_MEMBERS_LIMIT = 64;

function RoleMenuItems({
  currentRole,
  onSelect,
}: {
  currentRole?: EntityRole;
  onSelect: (role: EntityRole) => void;
}) {
  return (
    <>
      {PROJECT_GRANTABLE_ROLES.map((role) => (
        <DropdownMenuItem
          data-testid={`project-member-role-${role}`}
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
 * The single member-management surface for a project's invite roster
 * (kind:39010 read; kind:9010/9011 ops) — the roster list, role menus, the
 * add-members dialog, and the remove confirmation. Mounted by the project
 * page's Members card and by the Project Settings dialog's Members tab; both
 * write through the same roster mutations, so there is still exactly one
 * write path. The creator is an implicit Owner, pinned first and
 * unmanageable. Owners (the creator or a roster owner) manage; everyone
 * else sees a read-only list.
 *
 * The add-members dialog is controlled when `addOpen`/`onAddOpenChange` are
 * passed (the card keeps its header-slot trigger); uncontrolled otherwise,
 * with a built-in "Add members" button above the list.
 */
export function ProjectMembersManager({
  project,
  addOpen: addOpenProp,
  onAddOpenChange,
}: {
  project: ProjectContainer;
  addOpen?: boolean;
  onAddOpenChange?: (open: boolean) => void;
}) {
  const rosterQuery = useProjectRosterQuery(project);
  const roster = rosterQuery.data ?? project.members;
  const capabilities = useProjectCapabilities(project);

  const entries = React.useMemo(
    () => rosterWithOwner(project, roster),
    [project, roster],
  );

  const memberPubkeys = React.useMemo(
    () => entries.map((entry) => entry.pubkey),
    [entries],
  );
  const profilesQuery = useUsersBatchQuery(memberPubkeys);
  const profiles = profilesQuery.data?.profiles;
  // The host holding this panel knows its own managed agents by name and
  // primary role; without this the project's own agents rendered as bare hex
  // keys labelled Collaborator (ledger 207(4)).
  const managedAgentsQuery = useManagedAgentsQuery();
  const managedAgentsByPubkey = React.useMemo(
    () =>
      new Map(
        (managedAgentsQuery.data ?? []).map((agent) => [
          agent.pubkey.toLowerCase(),
          agent,
        ]),
      ),
    [managedAgentsQuery.data],
  );

  // This computer's session-provider key: a private project's roster names
  // it so the host can read the project (ledger 266), and a bare hex row
  // would hide which member that is.
  const providerStatusQuery = useQuery({
    queryKey: ["coding-session-provider-status"],
    queryFn: getCodingSessionProviderStatus,
  });
  const hostPubkey = providerStatusQuery.data?.providerPubkey ?? null;

  const identityFor = React.useCallback(
    (pubkey: string) =>
      projectMemberIdentity({
        pubkey,
        profileName: profiles?.[pubkey]?.displayName ?? null,
        profileIsAgent: profiles?.[pubkey]?.isAgent ?? null,
        managedAgent: managedAgentsByPubkey.get(pubkey.toLowerCase()) ?? null,
        hostPubkey,
        truncate: truncatePubkey,
      }),
    [hostPubkey, managedAgentsByPubkey, profiles],
  );
  const displayName = React.useCallback(
    (pubkey: string) => identityFor(pubkey).name,
    [identityFor],
  );

  const sortedEntries = React.useMemo(
    () =>
      [...entries].sort((a, b) => {
        // Creator pinned first, then by descending capability, then name.
        if (a.isCreator !== b.isCreator) return a.isCreator ? -1 : 1;
        const rank = entityRoleRank(a.role) - entityRoleRank(b.role);
        if (rank !== 0) return rank;
        return displayName(a.pubkey).localeCompare(displayName(b.pubkey));
      }),
    [entries, displayName],
  );

  const viewerIsOwner = capabilities.canManageRoster;

  const putMutation = usePutProjectRosterMutation(project);
  const removeMutation = useRemoveProjectRosterMutation(project);

  const [addOpenState, setAddOpenState] = React.useState(false);
  const addOpen = addOpenProp ?? addOpenState;
  const setAddOpen = onAddOpenChange ?? setAddOpenState;
  const [addUsers, setAddUsers] = React.useState<UserSearchResult[]>([]);
  const [addRole, setAddRole] = React.useState<EntityRole>("collaborator");
  const [removeTarget, setRemoveTarget] =
    React.useState<ProjectRosterEntry | null>(null);

  React.useEffect(() => {
    if (addOpen) return;
    setAddUsers([]);
    setAddRole("collaborator");
  }, [addOpen]);

  const changeRole = (entry: ProjectRosterEntry, role: EntityRole) => {
    if (role === entry.role) return;
    putMutation.mutate([{ pubkey: entry.pubkey, role }], {
      onError: (error) => {
        toast.error(
          error instanceof Error
            ? error.message
            : "Failed to update the member's role.",
        );
      },
    });
  };

  const addMembers = async () => {
    if (addUsers.length === 0) return;
    try {
      await putMutation.mutateAsync(
        addUsers.map((user) => ({ pubkey: user.pubkey, role: addRole })),
      );
      toast.success(
        addUsers.length === 1
          ? "Member added."
          : `${addUsers.length} members added.`,
      );
      setAddOpen(false);
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : "Failed to add members.",
      );
    }
  };

  // Uncontrolled mounts (the settings dialog tab) get a built-in trigger;
  // controlled mounts (the card) keep their own header-slot button.
  const showBuiltInAddButton = viewerIsOwner && addOpenProp === undefined;

  return (
    <div className="flex flex-col gap-2">
      {showBuiltInAddButton ? (
        <div className="flex justify-end">
          <Button
            data-testid="project-members-manager-add"
            onClick={() => setAddOpen(true)}
            size="sm"
            type="button"
            variant="outline"
          >
            <UserPlus className="mr-1.5 size-4" />
            Add members
          </Button>
        </div>
      ) : null}

      {sortedEntries.length === 0 ? (
        <EmptyHint>No members in this project.</EmptyHint>
      ) : (
        <ul className="flex flex-col gap-1">
          {sortedEntries.map((entry) => {
            const profile = profiles?.[entry.pubkey];
            const identity = identityFor(entry.pubkey);
            const name = identity.name;
            return (
              <li
                className="flex min-h-8 items-center gap-2 px-2"
                data-testid={`project-member-row-${entry.pubkey}`}
                key={entry.pubkey}
              >
                <ProfileAvatar
                  avatarUrl={profile?.avatarUrl ?? null}
                  className="h-6 w-6 text-2xs shadow-none"
                  iconClassName="h-3 w-3"
                  label={name}
                />
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-sm">{name}</span>
                  {identity.role || identity.showKey ? (
                    <span
                      className="truncate text-2xs text-muted-foreground"
                      data-testid={`project-member-secondary-${entry.pubkey}`}
                    >
                      {[identity.role, identity.showKey ? entry.pubkey : null]
                        .filter(Boolean)
                        .join(" · ")}
                    </span>
                  ) : null}
                </span>
                {identity.kind === "agent" ? (
                  <Badge variant="outline">Agent</Badge>
                ) : null}
                {identity.kind === "host" ? (
                  <Badge
                    data-testid={`project-member-host-${entry.pubkey}`}
                    variant="outline"
                  >
                    Session host
                  </Badge>
                ) : null}
                {entry.isCreator ? (
                  <Badge variant="secondary">{ENTITY_ROLE_LABELS.owner}</Badge>
                ) : viewerIsOwner ? (
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <button
                        aria-label={`Change role for ${name}`}
                        className="flex shrink-0 items-center gap-1 rounded-md px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                        data-testid={`project-member-role-menu-${entry.pubkey}`}
                        type="button"
                      >
                        {ENTITY_ROLE_LABELS[entry.role]}
                        <ChevronDown className="size-3" />
                      </button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <RoleMenuItems
                        currentRole={entry.role}
                        onSelect={(role) => changeRole(entry, role)}
                      />
                      <DropdownMenuSeparator />
                      <DropdownMenuItem
                        className="text-destructive focus:text-destructive"
                        data-testid={`project-member-remove-${entry.pubkey}`}
                        onSelect={() => setRemoveTarget(entry)}
                      >
                        Remove
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

      <Dialog onOpenChange={setAddOpen} open={addOpen}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>Add members</DialogTitle>
            <DialogDescription>
              Invite people to {project.name}. They join with the role you pick;
              you can change it per member afterward.
            </DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-3">
            <PersonaShareRecipients
              allowDirectPubkeyEntry
              disabled={putMutation.isPending}
              excludedPubkeys={memberPubkeys}
              limit={ADD_MEMBERS_LIMIT}
              onSelectionChange={setAddUsers}
              open={addOpen}
              selectedUsers={addUsers}
              testIdPrefix="project-members-add-dialog"
            />
            <div className="flex items-center justify-between gap-4">
              <span className="text-sm text-muted-foreground">Role</span>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    className="flex items-center gap-1 rounded-md border border-input px-3 py-1.5 text-sm transition-colors hover:bg-muted"
                    data-testid="project-members-add-role"
                    type="button"
                  >
                    {ENTITY_ROLE_LABELS[addRole]}
                    <ChevronDown className="size-3" />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <RoleMenuItems currentRole={addRole} onSelect={setAddRole} />
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </div>
          <DialogFooter className="mt-4">
            <Button
              onClick={() => setAddOpen(false)}
              type="button"
              variant="outline"
            >
              Cancel
            </Button>
            <Button
              data-testid="project-members-add-confirm"
              disabled={putMutation.isPending || addUsers.length === 0}
              onClick={() => void addMembers()}
              type="button"
            >
              Add members
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog
        onOpenChange={(open) => {
          if (!open) setRemoveTarget(null);
        }}
        open={removeTarget !== null}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove this member?</AlertDialogTitle>
            <AlertDialogDescription>
              {removeTarget
                ? `${displayName(removeTarget.pubkey)} will lose access to this project.${
                    removeTarget.role === "owner"
                      ? " They are an Owner — removing them also revokes their ability to manage members."
                      : ""
                  }`
                : ""}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={removeMutation.isPending}>
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              data-testid="project-member-remove-confirm"
              disabled={removeMutation.isPending}
              onClick={(event) => {
                event.preventDefault();
                if (!removeTarget) return;
                removeMutation.mutate([removeTarget.pubkey], {
                  onSuccess: () => {
                    toast.success("Member removed.");
                    setRemoveTarget(null);
                  },
                  onError: (error) => {
                    toast.error(
                      error instanceof Error
                        ? error.message
                        : "Failed to remove the member.",
                    );
                  },
                });
              }}
            >
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

/** True when the current viewer can manage this project's roster — the
 * creator or a roster owner. Kept as a named re-export so mount points read
 * as what they gate; the decision itself lives in one place. */
export function useViewerIsProjectOwner(project: ProjectContainer): boolean {
  return useProjectCapabilities(project).canManageRoster;
}
