import { Search } from "lucide-react";
import * as React from "react";

import {
  describeShareRecipientEmptyState,
  describeShareRecipientRow,
  hasResolvedShareRecipientProfile,
  resolveShareRecipientKind,
  SHARE_RECIPIENT_KIND_GROUP_LABEL,
  SHARE_RECIPIENT_KIND_NOTES,
  SHARE_RECIPIENT_KIND_TABS,
  type ShareRecipientKind,
} from "@/features/agents/lib/shareRecipientVocabulary";
import { useKnownAgentPubkeys } from "@/features/agents/useKnownAgentPubkeys";
import { useIsArchivedPredicate } from "@/features/identity-archive/hooks";
import {
  useFlattenedUserSearchResults,
  useInfiniteUserSearchQuery,
  useUserSearchFetchMoreOnScroll,
} from "@/features/profile/hooks";
import {
  getKeyboardSearchSelection,
  rankUserCandidatesBySearch,
} from "@/features/profile/lib/userCandidateSearch";
import { ProfileAvatar } from "@/features/profile/ui/ProfileAvatar";
import { SelectedRecipientChip } from "@/features/profile/ui/SelectedRecipientChip";
import { useIdentityQuery } from "@/shared/api/hooks";
import type { UserSearchResult } from "@/shared/api/types";
import { cn } from "@/shared/lib/cn";
import { parsePubkeyInput } from "@/shared/lib/nostrUtils";
import { normalizePubkey, truncatePubkey } from "@/shared/lib/pubkey";
import { Popover, PopoverAnchor, PopoverContent } from "@/shared/ui/popover";
import { PubKey } from "@/shared/ui/PubKey";
import { Skeleton } from "@/shared/ui/skeleton";

const RECIPIENT_LIMIT = 8;

export function formatShareRecipientName(user: UserSearchResult) {
  return (
    user.displayName?.trim() ||
    user.nip05Handle?.trim() ||
    truncatePubkey(user.pubkey)
  );
}

/**
 * Whether we hold agent evidence for this key.
 *
 * Strictly wider than the profile flag alone: `user.isAgent` is set only from
 * a NIP-OA owner attestation the native side verified, so a managed agent on
 * this disk or a relay-registered agent whose owner never attested reads as
 * `false` there. `isKnownAgent` folds in that local baseline additively — it
 * can only widen the flag, never contradict it (see `knownAgentPubkeys.ts`).
 *
 * The converse does not exist: nothing in this app positively establishes that
 * a key is a person, so "not an agent" is an absence of evidence and the
 * vocabulary in `shareRecipientVocabulary.ts` says so out loud.
 */
export function isShareRecipientAgent(
  user: UserSearchResult,
  isKnownAgent?: (pubkey: string) => boolean,
): boolean {
  return (
    isKnownAgent?.(normalizePubkey(user.pubkey)) === true ||
    user.isAgent === true
  );
}

/**
 * Who may appear in the recipient picker.
 *
 * Agents are excluded unless the caller opts in: the persona-share surfaces
 * this was built for have no use for one, while a coding session's People
 * surface grants to seated agents by name. Everything else — yourself,
 * already-selected or excluded pubkeys, archived identities — is filtered the
 * same way regardless.
 *
 * `kind` is the caller's explicit view; omitted, it falls back to today's
 * `allowAgents` behaviour exactly. Duplicates are dropped only when their
 * normalised keys are identical — two candidates sharing a display name are
 * two different keys and both stay.
 */
export function filterShareRecipientCandidates(input: {
  allowAgents: boolean;
  currentPubkey: string | null;
  excludedPubkeys: ReadonlySet<string>;
  isArchived: (pubkey: string) => boolean;
  isKnownAgent?: (pubkey: string) => boolean;
  kind?: ShareRecipientKind;
  selectedPubkeys: ReadonlySet<string>;
  users: readonly UserSearchResult[];
}): UserSearchResult[] {
  const kind = resolveShareRecipientKind(input);
  const seen = new Set<string>();
  const kept: UserSearchResult[] = [];

  for (const user of input.users) {
    const pubkey = normalizePubkey(user.pubkey);
    if (
      seen.has(pubkey) ||
      pubkey === input.currentPubkey ||
      input.excludedPubkeys.has(pubkey) ||
      input.selectedPubkeys.has(pubkey) ||
      input.isArchived(pubkey)
    ) {
      continue;
    }

    const isAgent = isShareRecipientAgent(user, input.isKnownAgent);
    if (kind === "agents" ? !isAgent : kind === "people" ? isAgent : false) {
      continue;
    }

    seen.add(pubkey);
    kept.push(user);
  }

  return kept;
}

export function PersonaShareRecipients({
  allowAgents = false,
  allowDirectPubkeyEntry = false,
  disabled,
  excludedPubkeys = [],
  kind,
  limit = RECIPIENT_LIMIT,
  onSelectionChange,
  open,
  selectedUsers,
  testIdPrefix = "persona-share",
}: {
  /**
   * Include agent identities in the search results.
   *
   * Off everywhere by default: sharing a persona with an agent is not a thing
   * a person means to do, and an agent row in that picker was noise. A coding
   * session's People surface is the exception — an agent seated on a crew
   * *is* a grantee there (design D7), so it must be selectable by name rather
   * than only by pasting a raw pubkey.
   */
  allowAgents?: boolean;
  /** Offer a synthetic "by public key" result when the search text parses as
   * a hex pubkey or npub — for inviting someone with no kind:0 profile on the
   * relay yet (mirrors `ChannelMemberInviteCard`'s direct-invite behavior). */
  allowDirectPubkeyEntry?: boolean;
  disabled: boolean;
  excludedPubkeys?: readonly string[];
  /**
   * Which slice of the directory to open on, and an opt-in to the People /
   * Agents / All filter above the results.
   *
   * The control renders *only* when this is passed, so the five surfaces that
   * never asked for it keep their markup. Omitted, the view is derived from
   * `allowAgents` (`allowAgents ? "all" : "people"`), which is what every
   * caller gets today. Switching the filter is presentation: it changes what
   * is shown, never what the query asks for.
   */
  kind?: ShareRecipientKind;
  /** Maximum number of selectable recipients. */
  limit?: number;
  onSelectionChange: (users: UserSearchResult[]) => void;
  open: boolean;
  selectedUsers: UserSearchResult[];
  testIdPrefix?: string;
}) {
  const [searchQuery, setSearchQuery] = React.useState("");
  const [isPickerOpen, setIsPickerOpen] = React.useState(false);
  // `null` means "follow the caller"; a value means the reader pressed a
  // filter button. Derived rather than synced, so a caller changing `kind`
  // still moves the view and closing the picker forgets the override.
  const [kindOverride, setKindOverride] =
    React.useState<ShareRecipientKind | null>(null);
  const recipientFieldRef = React.useRef<HTMLDivElement>(null);
  const searchInputRef = React.useRef<HTMLInputElement>(null);
  const deferredSearchQuery = React.useDeferredValue(searchQuery.trim());
  const identityQuery = useIdentityQuery();
  const isArchived = useIsArchivedPredicate();
  const activeKind =
    kindOverride ?? resolveShareRecipientKind({ allowAgents, kind });
  // Content-stable context (no new query observers), so this is safe as a
  // memo dependency in a render-hot picker.
  const knownAgentPubkeys = useKnownAgentPubkeys();
  const isKnownAgent = React.useCallback(
    (pubkey: string) => knownAgentPubkeys.has(normalizePubkey(pubkey)),
    [knownAgentPubkeys],
  );
  const selectedPubkeys = React.useMemo(
    () => new Set(selectedUsers.map((user) => normalizePubkey(user.pubkey))),
    [selectedUsers],
  );
  const excludedPubkeySet = React.useMemo(
    () => new Set(excludedPubkeys.map(normalizePubkey)),
    [excludedPubkeys],
  );
  const userSearchQuery = useInfiniteUserSearchQuery(deferredSearchQuery, {
    allowEmpty: true,
    enabled: open && selectedUsers.length < limit,
    limit: 50,
  });
  const userSearchResults = useFlattenedUserSearchResults(userSearchQuery.data);
  const currentPubkey = identityQuery.data?.pubkey
    ? normalizePubkey(identityQuery.data.pubkey)
    : null;
  const searchResults = React.useMemo(() => {
    const candidates = filterShareRecipientCandidates({
      allowAgents,
      currentPubkey,
      excludedPubkeys: excludedPubkeySet,
      isArchived,
      isKnownAgent,
      kind: activeKind,
      selectedPubkeys,
      users: userSearchResults,
    });

    return rankUserCandidatesBySearch({
      allowEmptyQuery: true,
      candidates,
      getLabel: formatShareRecipientName,
      limit: 50,
      query: deferredSearchQuery,
    });
  }, [
    activeKind,
    allowAgents,
    currentPubkey,
    deferredSearchQuery,
    excludedPubkeySet,
    isArchived,
    isKnownAgent,
    selectedPubkeys,
    userSearchResults,
  ]);
  // Display names collide; keys do not. Every named row carries its short key
  // so two "Builder"s are told apart, and an agent row names its owner when
  // one of the loaded profiles resolves that key. Lookup only — no fetch.
  const loadedNames = React.useMemo(() => {
    const names = new Map<string, string>();
    for (const user of userSearchResults) {
      const name = user.displayName?.trim() || user.nip05Handle?.trim();
      if (name) names.set(normalizePubkey(user.pubkey), name);
    }
    return names;
  }, [userSearchResults]);
  // Someone without a kind:0 profile is invisible to search — offer a
  // synthetic result so they can still be added by pubkey/npub. Suppressed
  // against `searchResults` (the ranked, *displayed* list), not the raw
  // `userSearchResults` — the fuzzy name ranker can legitimately drop a
  // pubkey-matched hit (a raw hex string rarely fuzzy-matches a display
  // name), and checking the raw list would then hide the person from both
  // the ranked results and this synthetic fallback.
  //
  // It is deliberately *not* filtered by the active view: the reader typed
  // this key, so it stays on offer. Its `isAgent: false` asserts nothing — the
  // row is labelled from the evidence we actually hold, which for a key with
  // no profile is "Unidentified", never a person.
  const directPubkeyUser = React.useMemo<UserSearchResult | null>(() => {
    if (!allowDirectPubkeyEntry) return null;
    const pubkey = parsePubkeyInput(deferredSearchQuery);
    if (
      pubkey === null ||
      pubkey === currentPubkey ||
      excludedPubkeySet.has(pubkey) ||
      selectedPubkeys.has(pubkey) ||
      searchResults.some((user) => normalizePubkey(user.pubkey) === pubkey)
    ) {
      return null;
    }
    return {
      pubkey,
      displayName: null,
      avatarUrl: null,
      nip05Handle: null,
      ownerPubkey: null,
      isAgent: false,
    };
  }, [
    allowDirectPubkeyEntry,
    currentPubkey,
    deferredSearchQuery,
    excludedPubkeySet,
    searchResults,
    selectedPubkeys,
  ]);
  const isSearchSettling =
    userSearchQuery.isLoading || searchQuery.trim() !== deferredSearchQuery;
  const visibleSearchResults = isSearchSettling
    ? []
    : directPubkeyUser
      ? [directPubkeyUser, ...searchResults]
      : searchResults;
  const handleDirectoryScroll = useUserSearchFetchMoreOnScroll(
    userSearchQuery,
    selectedUsers.length < limit,
  );
  // An exhausted-view string ("No people found." and its siblings) is a claim
  // about the whole directory, so it is only allowed once the pages have
  // actually run out. Each view names the population it searched.
  const emptyState = describeShareRecipientEmptyState({
    hasMoreResults: userSearchQuery.hasNextPage === true,
    isLoadingMore: userSearchQuery.isFetchingNextPage === true,
    kind: activeKind,
  });

  React.useEffect(() => {
    if (!open) {
      setSearchQuery("");
      setIsPickerOpen(false);
      setKindOverride(null);
    }
  }, [open]);

  function selectUser(user: UserSearchResult) {
    if (selectedUsers.length >= limit) return;
    onSelectionChange([...selectedUsers, user]);
    setSearchQuery("");
    setIsPickerOpen(true);
    searchInputRef.current?.focus({ preventScroll: true });
  }

  function removeUser(pubkey: string) {
    onSelectionChange(
      selectedUsers.filter(
        (user) => normalizePubkey(user.pubkey) !== normalizePubkey(pubkey),
      ),
    );
    searchInputRef.current?.focus({ preventScroll: true });
  }

  return (
    <div className="min-w-0 flex-1">
      <Popover
        modal={false}
        onOpenChange={setIsPickerOpen}
        open={isPickerOpen && !disabled}
      >
        <PopoverAnchor asChild>
          {/* biome-ignore lint/a11y/noStaticElementInteractions: the nested input is the keyboard-accessible focus target */}
          {/* biome-ignore lint/a11y/useKeyWithClickEvents: clicking the shell focuses the nested search input */}
          <div
            className="grid min-h-10 min-w-0 cursor-text grid-cols-[minmax(0,1fr)_auto] items-start gap-x-3 rounded-md border border-input bg-background px-2 py-1.5 focus-within:ring-1 focus-within:ring-ring"
            data-testid={`${testIdPrefix}-recipient-field`}
            onClick={() => {
              if (disabled) return;
              setIsPickerOpen(true);
              searchInputRef.current?.focus({ preventScroll: true });
            }}
            ref={recipientFieldRef}
          >
            <div
              className="flex min-w-0 flex-wrap items-center gap-1.5"
              data-testid={`${testIdPrefix}-recipient-input-region`}
            >
              {selectedUsers.length === 0 ? (
                <Search className="h-4 w-4 shrink-0 text-muted-foreground/55" />
              ) : null}
              {selectedUsers.map((user) => (
                <SelectedRecipientChip
                  disabled={disabled}
                  inspectable={false}
                  key={user.pubkey}
                  label={formatShareRecipientName(user)}
                  onRemove={() => removeUser(user.pubkey)}
                  poofOnRemove={false}
                  testIds={{
                    chip: `${testIdPrefix}-recipient-chip-${user.pubkey}`,
                  }}
                  user={user}
                />
              ))}
              <input
                aria-autocomplete="list"
                aria-controls={`${testIdPrefix}-recipient-results`}
                aria-expanded={isPickerOpen && !disabled}
                aria-label="Share with"
                autoCapitalize="none"
                autoComplete="off"
                autoCorrect="off"
                className="h-7 min-w-16 flex-1 border-0 bg-transparent p-0 text-sm outline-hidden placeholder:text-muted-foreground/55"
                data-testid={`${testIdPrefix}-recipient-search`}
                disabled={disabled || selectedUsers.length >= limit}
                onChange={(event) => {
                  setSearchQuery(event.target.value);
                  setIsPickerOpen(true);
                }}
                onFocus={() => setIsPickerOpen(true)}
                onKeyDown={(event) => {
                  if (event.key === "Escape") {
                    event.preventDefault();
                    setIsPickerOpen(false);
                    return;
                  }

                  if (
                    event.key === "Backspace" &&
                    searchQuery.length === 0 &&
                    selectedUsers.length > 0
                  ) {
                    event.preventDefault();
                    const lastUser = selectedUsers[selectedUsers.length - 1];
                    if (lastUser) removeUser(lastUser.pubkey);
                    return;
                  }

                  if (event.key !== "Enter") return;
                  const selection = getKeyboardSearchSelection({
                    currentQuery: searchQuery,
                    rankedQuery: deferredSearchQuery,
                    results: visibleSearchResults,
                  });
                  if (!selection) return;
                  event.preventDefault();
                  selectUser(selection);
                }}
                placeholder={
                  selectedUsers.length >= limit
                    ? "Recipient limit reached"
                    : selectedUsers.length === 0
                      ? "Search people"
                      : ""
                }
                ref={searchInputRef}
                role="combobox"
                spellCheck={false}
                type="text"
                value={searchQuery}
              />
            </div>
          </div>
        </PopoverAnchor>
        <PopoverContent
          align="start"
          className="w-(--radix-popover-trigger-width) overflow-hidden p-0"
          data-testid={`${testIdPrefix}-recipient-popover`}
          onCloseAutoFocus={(event) => event.preventDefault()}
          onInteractOutside={(event) => {
            const target = event.detail.originalEvent.target;
            if (
              target instanceof Element &&
              recipientFieldRef.current?.contains(target)
            ) {
              event.preventDefault();
            }
          }}
          onOpenAutoFocus={(event) => event.preventDefault()}
          sideOffset={6}
        >
          {kind ? (
            <div className="border-b border-border/60 px-2 py-2">
              <fieldset
                aria-label={SHARE_RECIPIENT_KIND_GROUP_LABEL}
                className="flex items-center gap-1"
                data-testid={`${testIdPrefix}-recipient-kind-filter`}
              >
                {SHARE_RECIPIENT_KIND_TABS.map((tab) => {
                  const isActive = tab.kind === activeKind;
                  return (
                    <button
                      aria-pressed={isActive}
                      className={cn(
                        "rounded-md px-2 py-1 text-xs transition-colors focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring",
                        isActive
                          ? "bg-muted font-semibold text-foreground underline underline-offset-4"
                          : "font-normal text-muted-foreground hover:bg-muted/50",
                      )}
                      data-testid={`${testIdPrefix}-recipient-kind-${tab.kind}`}
                      key={tab.kind}
                      onClick={() => {
                        // Presentation only: no refetch, no change to what the
                        // query asks for, and no page is fetched here.
                        setKindOverride(tab.kind);
                        searchInputRef.current?.focus({ preventScroll: true });
                      }}
                      type="button"
                    >
                      {tab.label}
                    </button>
                  );
                })}
              </fieldset>
              <p
                className="mt-1.5 px-1 text-2xs text-muted-foreground"
                data-testid={`${testIdPrefix}-recipient-kind-note`}
              >
                {SHARE_RECIPIENT_KIND_NOTES[activeKind]}
              </p>
            </div>
          ) : null}
          <div
            className="max-h-64 overflow-y-auto overscroll-contain py-1"
            data-testid={`${testIdPrefix}-recipient-results`}
            id={`${testIdPrefix}-recipient-results`}
            onScroll={handleDirectoryScroll}
            onTouchMoveCapture={(event) => event.stopPropagation()}
            onWheelCapture={(event) => event.stopPropagation()}
            role="listbox"
          >
            {isSearchSettling ? (
              <div
                aria-label="Loading people"
                className="space-y-3 px-3 py-3"
                role="status"
              >
                {["w-36", "w-28", "w-40"].map((width) => (
                  <div className="flex items-center gap-3" key={width}>
                    <Skeleton className="h-8 w-8 shrink-0 rounded-full" />
                    <Skeleton className={`h-4 ${width}`} />
                  </div>
                ))}
              </div>
            ) : visibleSearchResults.length > 0 ? (
              visibleSearchResults.map((user) => {
                const name = formatShareRecipientName(user);
                const ownerPubkey = user.ownerPubkey
                  ? normalizePubkey(user.ownerPubkey)
                  : null;
                const row = describeShareRecipientRow({
                  hasProfile: hasResolvedShareRecipientProfile(user),
                  isAgent: isShareRecipientAgent(user, isKnownAgent),
                  ownerLabel: ownerPubkey
                    ? (loadedNames.get(ownerPubkey) ??
                      truncatePubkey(ownerPubkey))
                    : null,
                });
                return (
                  <button
                    aria-label={`Add ${name}, key ${truncatePubkey(user.pubkey)}${row.label ? `, ${row.label.toLowerCase()}` : ""}`}
                    className="flex min-h-11 w-full items-center gap-3 px-3 py-2 text-left transition-colors hover:bg-muted/50 focus-visible:bg-muted/50 focus-visible:outline-hidden"
                    data-testid={`${testIdPrefix}-recipient-option-${user.pubkey}`}
                    key={user.pubkey}
                    onClick={() => selectUser(user)}
                    role="option"
                    type="button"
                  >
                    <ProfileAvatar
                      avatarUrl={user.avatarUrl}
                      className="h-8 w-8 text-xs shadow-none"
                      iconClassName="h-4 w-4"
                      label={name}
                    />
                    <span className="flex min-w-0 flex-1 flex-col">
                      <span className="truncate text-sm font-medium">
                        {name}
                      </span>
                      {row.showShortKey || row.label ? (
                        <span
                          className="flex min-w-0 items-center gap-1.5 text-2xs text-muted-foreground"
                          data-testid={`${testIdPrefix}-recipient-option-meta-${user.pubkey}`}
                        >
                          {row.showShortKey ? (
                            <PubKey
                              className="shrink-0 text-2xs"
                              interactive={false}
                              pubkey={user.pubkey}
                            />
                          ) : null}
                          {row.label ? (
                            <span className="shrink-0 rounded-sm border border-border/70 px-1 font-medium">
                              {row.label}
                            </span>
                          ) : null}
                          {row.detail ? (
                            <span className="truncate">{row.detail}</span>
                          ) : null}
                        </span>
                      ) : null}
                    </span>
                    {directPubkeyUser?.pubkey === user.pubkey ? (
                      <span className="shrink-0 text-xs text-muted-foreground">
                        by public key
                      </span>
                    ) : null}
                  </button>
                );
              })
            ) : (
              <div
                className="px-3 py-3"
                data-testid={`${testIdPrefix}-recipient-empty`}
              >
                <p className="text-sm text-muted-foreground">
                  {emptyState.message}
                </p>
                {emptyState.hint ? (
                  <p className="mt-1 text-xs text-muted-foreground">
                    {emptyState.hint}
                  </p>
                ) : null}
                {emptyState.loadMoreLabel ? (
                  <button
                    className="mt-2 rounded-md border border-input px-2 py-1 text-xs font-medium transition-colors hover:bg-muted focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60"
                    data-testid={`${testIdPrefix}-recipient-load-more`}
                    disabled={userSearchQuery.isFetchingNextPage}
                    onClick={() => {
                      // Exactly one page per press. Never a loop, and never
                      // triggered by switching the filter.
                      if (
                        userSearchQuery.hasNextPage &&
                        !userSearchQuery.isFetchingNextPage
                      ) {
                        void userSearchQuery.fetchNextPage();
                      }
                    }}
                    type="button"
                  >
                    {emptyState.loadMoreLabel}
                  </button>
                ) : null}
              </div>
            )}
          </div>
        </PopoverContent>
      </Popover>
    </div>
  );
}
