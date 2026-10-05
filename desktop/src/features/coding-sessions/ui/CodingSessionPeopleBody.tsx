import { ChevronDown } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { PersonaShareRecipients } from "@/features/agents/ui/PersonaShareRecipients";
import { useKnownAgentPubkeys } from "@/features/agents/useKnownAgentPubkeys";
import { useUsersBatchQuery } from "@/features/profile/hooks";
import {
  buildCodingSessionIngressAuthorityIdentity,
  OPEN_CODING_SESSION_INGRESS_AUTHORITY,
} from "@/features/coding-sessions/lib/codingSessionIngressAuthority";
import { peekCodingSessionIngressStore } from "@/features/coding-sessions/lib/codingSessionIngressStoreCache";
import {
  useCodingSessionGrantMutation,
  useCodingSessionRevokeMutation,
  useCodingSessionRoster,
  type CodingSessionRosterEntry,
} from "@/features/coding-sessions/lib/codingSessionRoster";
import {
  codingSessionRosterBadgesSettled,
  codingSessionRowKind,
  codingSessionRowKindIsFinal,
  deriveCodingSessionProviderRuntimeLabels,
} from "@/features/coding-sessions/lib/codingSessionRowKind";
import { CodingSessionPersonRow } from "@/features/coding-sessions/ui/CodingSessionPersonRow";
import { useIdentityQuery } from "@/shared/api/hooks";
import type { UserSearchResult } from "@/shared/api/types";
import { cn } from "@/shared/lib/cn";
import {
  ENTITY_ROLE_DESCRIPTIONS,
  ENTITY_ROLE_LABELS,
  entityRoleRank,
  SESSION_GRANTABLE_ROLES,
  type EntityRole,
} from "@/shared/lib/entityRoles";
import { normalizePubkey, truncatePubkey } from "@/shared/lib/pubkey";
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
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

/** Invitees one grant round can hold. Each becomes its own chain link, so
 * the batch stays small enough that a mid-batch failure reads clearly. */
const INVITE_LIMIT = 8;

/** Stable identity for "no provider fact reached this roster". */
const NO_PROVIDER_LABELS: ReadonlyMap<string, string | null> = new Map();

/** Whether the viewer founded this session — the one key that may manage it. */
export function useCodingSessionPeopleIsOwner(
  founderPubkey: string | null,
): boolean {
  const identityQuery = useIdentityQuery();
  const self = identityQuery.data?.pubkey?.toLowerCase();
  return (
    !!self && founderPubkey !== null && self === founderPubkey.toLowerCase()
  );
}

/** The sentence under "People", for the founder and for everyone else. */
export function codingSessionPeopleIntro(isOwner: boolean): string {
  return isOwner
    ? "Share this session. Collaborators may steer it; viewers follow along read-only."
    : "Who has access to this session. Only the session owner can change it.";
}

/**
 * The session People body: the authority-chain roster (founder pinned as
 * Owner, live grants, pending invites) with owner-only management — invite
 * with a role over {@link SESSION_GRANTABLE_ROLES}, and per-row revoke.
 * Non-owners see the same roster read-only. All state is relay-derived:
 * grants are kind:44228 chain links, acceptance is the relay's kind:40099
 * receipt, and a row shows "Inviting…" until its receipt lands.
 *
 * Mounted twice: inside the People dialog (`CodingSessionPeoplePopover`, a
 * thin wrapper) and as the People surface (SV-24), so inviting works from
 * either and the two can never disagree about who has access.
 */
export function CodingSessionPeopleBody({
  active,
  channelId,
  founderPubkey,
  genesisRef,
  providerAuthorityLabels,
  rosterRef,
  variant = "dialog",
}: {
  /** The body is on screen: the dialog is open, or the surface is showing. */
  active: boolean;
  channelId: string;
  founderPubkey: string | null;
  genesisRef: string | null;
  /**
   * Provider authority keys → the runtime label their facts reached.
   *
   * Optional and unset at every call site today: the body reads the same
   * fact out of the verified ingress store itself (below). It exists as the
   * one seam a caller that *already holds* the umbrella can hand down —
   * `providerAuthorityLabels={new Map(umbrella.executions.map((execution) =>
   * [execution.signerPubkey, execution.activeGeneration.runtime]))}` — without
   * this file having to reach for a hook it would otherwise have to mount.
   */
  providerAuthorityLabels?: ReadonlyMap<string, string | null>;
  /** The roster region, for the dialog's opening focus. */
  rosterRef?: React.Ref<HTMLElement>;
  /** `surface` lets the roster grow with its panel instead of a fixed cap. */
  variant?: "dialog" | "surface";
}) {
  const isOwner = useCodingSessionPeopleIsOwner(founderPubkey);

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

  // The additive agent baseline (`knownAgentPubkeys.ts`): managed on this disk
  // ∪ relay-registered, published over context by `KnownAgentPubkeysProvider`.
  // Reading it adds no query observer. Folded with the profile flag below, it
  // is strictly wider than `profile?.isAgent` alone — which is why a managed
  // or relay-registered agent whose owner never attested now reads as one.
  const knownAgentPubkeys = useKnownAgentPubkeys();

  /**
   * The provider authority keys behind this channel's executions, with the
   * runtime label their own signed facts reached.
   *
   * Read straight out of the verified ingress store this window already holds
   * — `CodingSessionWorkspace` mounts `useCodingSessionCatalog(channelId, …,
   * { authorityMode: "open" })`, which acquires exactly this store identity
   * (`useTrustedCodingSessionIngress.ts`: `${authorityIdentity}|${channels}`,
   * a single channel needing no separator). `peek` never creates one, so this
   * adds no relay subscription, no history fetch, and no query key; it only
   * reads bytes that already passed the full signature/authority classifier.
   *
   * Deliberately re-read each time the body becomes active rather than
   * subscribed: the fail direction is silence. A cold store yields no
   * providers and every row falls back to its other evidence — never a claim
   * that some key *is* a provider.
   */
  const peekedProviderLabels = React.useMemo(() => {
    if (providerAuthorityLabels !== undefined) return NO_PROVIDER_LABELS;
    if (!active || channelId.length === 0) return NO_PROVIDER_LABELS;
    const store = peekCodingSessionIngressStore(
      `${buildCodingSessionIngressAuthorityIdentity(
        OPEN_CODING_SESSION_INGRESS_AUTHORITY,
      )}|${channelId}`,
    );
    if (store === null) return NO_PROVIDER_LABELS;
    return deriveCodingSessionProviderRuntimeLabels(
      channelId,
      store.snapshot([channelId]).metadata,
    );
  }, [active, channelId, providerAuthorityLabels]);
  const providerLabels = providerAuthorityLabels ?? peekedProviderLabels;

  // Kind badges wait for a real profile result, so a row's one identity word
  // cannot pop in late — except where the chain already settled it (owner,
  // provider, seat), which needs no profile at all.
  const badgesSettled = codingSessionRosterBadgesSettled({
    memberCount: memberPubkeys.length,
    profilesUpdatedAt: profilesQuery.dataUpdatedAt,
    profilesFailed: profilesQuery.isError,
  });

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
    if (active) return;
    setInviteUsers([]);
    setInviteRole("collaborator");
  }, [active]);

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
    <>
      <section
        aria-label="Session access"
        className="flex flex-col gap-2 outline-hidden"
        data-testid="coding-session-people-roster"
        ref={rosterRef}
        tabIndex={-1}
      >
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
          <>
            <ul
              className={cn(
                "flex flex-col gap-1",
                variant === "dialog" && "max-h-72 overflow-y-auto",
              )}
            >
              {sortedEntries.map((entry) => {
                const profile = profiles?.[entry.pubkey];
                const normalized = normalizePubkey(entry.pubkey);
                const providerLabel = providerLabels.has(normalized)
                  ? (providerLabels.get(normalized) ?? null)
                  : null;
                const kind = codingSessionRowKind({
                  isFounder: entry.role === "owner",
                  isProvider: providerLabels.has(normalized),
                  seatRole: entry.seatRole ?? null,
                  // Additive merge, exactly as `knownAgentPubkeys.ts`
                  // prescribes: the shared baseline widened by this
                  // surface's own verified profile flag, never narrowed.
                  isAgent:
                    knownAgentPubkeys.has(normalized) ||
                    profile?.isAgent === true,
                });
                return (
                  <CodingSessionPersonRow
                    avatarUrl={profile?.avatarUrl ?? null}
                    entry={entry}
                    hasProfile={profile !== undefined}
                    isFounderViewer={isOwner}
                    key={entry.pubkey}
                    kind={
                      badgesSettled || codingSessionRowKindIsFinal(kind)
                        ? kind
                        : null
                    }
                    name={displayName(entry.pubkey)}
                    onRevoke={() => setRevokeTarget(entry)}
                    providerLabel={providerLabel}
                  />
                );
              })}
            </ul>
            {/* The `ul` above stays shrinkable on purpose — that is what
                makes it scroll at `max-h-72`. This disclosure does not: it
                is a claim about what the surface can and cannot verify, and
                squeezing it is the same mistake as squeezing a row. */}
            <p className="shrink-0 text-2xs text-muted-foreground">
              Session access only — nothing here changes project membership.
              This app can verify that a key is an agent; it can never verify
              that a key is a person, so a key with no agent evidence is shown
              as unidentified rather than as one.
            </p>
          </>
        )}
      </section>

      {isOwner ? (
        <div className="flex flex-col gap-3 border-t border-border/60 pt-4">
          <p className="text-sm font-medium">Invite</p>
          <PersonaShareRecipients
            // A seated agent is a grantee here (D7), so agents are
            // selectable by name in this one picker.
            allowAgents
            allowDirectPubkeyEntry
            // Opens on People, with the People / Agents / All control
            // showing. Without this the view derives from `allowAgents`
            // (`resolveShareRecipientKind` ⇒ "all"), which is how the one
            // picker that most needs the filter was the one surface not
            // rendering it — an agent-heavy first page over the person the
            // reader came here to invite. Agents stay one press away.
            kind="people"
            disabled={grantMutation.isPending}
            excludedPubkeys={memberPubkeys}
            limit={INVITE_LIMIT}
            onSelectionChange={setInviteUsers}
            open={active}
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
    </>
  );
}
