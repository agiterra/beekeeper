import { Search } from "lucide-react";
import * as React from "react";

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
import { parsePubkeyInput } from "@/shared/lib/nostrUtils";
import { normalizePubkey, truncatePubkey } from "@/shared/lib/pubkey";
import { Popover, PopoverAnchor, PopoverContent } from "@/shared/ui/popover";
import { Skeleton } from "@/shared/ui/skeleton";

const RECIPIENT_LIMIT = 8;

export function formatShareRecipientName(user: UserSearchResult) {
  return (
    user.displayName?.trim() ||
    user.nip05Handle?.trim() ||
    truncatePubkey(user.pubkey)
  );
}

export function PersonaShareRecipients({
  allowDirectPubkeyEntry = false,
  disabled,
  excludedPubkeys = [],
  onSelectionChange,
  open,
  selectedUsers,
  testIdPrefix = "persona-share",
}: {
  /** Offer a synthetic "by public key" result when the search text parses as
   * a hex pubkey or npub — for inviting someone with no kind:0 profile on the
   * relay yet (mirrors `ChannelMemberInviteCard`'s direct-invite behavior). */
  allowDirectPubkeyEntry?: boolean;
  disabled: boolean;
  excludedPubkeys?: readonly string[];
  onSelectionChange: (users: UserSearchResult[]) => void;
  open: boolean;
  selectedUsers: UserSearchResult[];
  testIdPrefix?: string;
}) {
  const [searchQuery, setSearchQuery] = React.useState("");
  const [isPickerOpen, setIsPickerOpen] = React.useState(false);
  const recipientFieldRef = React.useRef<HTMLDivElement>(null);
  const searchInputRef = React.useRef<HTMLInputElement>(null);
  const deferredSearchQuery = React.useDeferredValue(searchQuery.trim());
  const identityQuery = useIdentityQuery();
  const isArchived = useIsArchivedPredicate();
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
    enabled: open && selectedUsers.length < RECIPIENT_LIMIT,
    limit: 50,
  });
  const userSearchResults = useFlattenedUserSearchResults(userSearchQuery.data);
  const currentPubkey = identityQuery.data?.pubkey
    ? normalizePubkey(identityQuery.data.pubkey)
    : null;
  const searchResults = React.useMemo(() => {
    const candidates = userSearchResults.filter((user) => {
      const pubkey = normalizePubkey(user.pubkey);
      return (
        !user.isAgent &&
        pubkey !== currentPubkey &&
        !excludedPubkeySet.has(pubkey) &&
        !selectedPubkeys.has(pubkey) &&
        !isArchived(pubkey)
      );
    });

    return rankUserCandidatesBySearch({
      allowEmptyQuery: true,
      candidates,
      getLabel: formatShareRecipientName,
      limit: 50,
      query: deferredSearchQuery,
    });
  }, [
    currentPubkey,
    deferredSearchQuery,
    excludedPubkeySet,
    isArchived,
    selectedPubkeys,
    userSearchResults,
  ]);
  // Someone without a kind:0 profile is invisible to search — offer a
  // synthetic result so they can still be added by pubkey/npub. Suppressed
  // against `searchResults` (the ranked, *displayed* list), not the raw
  // `userSearchResults` — the fuzzy name ranker can legitimately drop a
  // pubkey-matched hit (a raw hex string rarely fuzzy-matches a display
  // name), and checking the raw list would then hide the person from both
  // the ranked results and this synthetic fallback.
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
    selectedUsers.length < RECIPIENT_LIMIT,
  );

  React.useEffect(() => {
    if (!open) {
      setSearchQuery("");
      setIsPickerOpen(false);
    }
  }, [open]);

  function selectUser(user: UserSearchResult) {
    if (selectedUsers.length >= RECIPIENT_LIMIT) return;
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
                disabled={disabled || selectedUsers.length >= RECIPIENT_LIMIT}
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
                  selectedUsers.length >= RECIPIENT_LIMIT
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
              visibleSearchResults.map((user) => (
                <button
                  aria-label={`Add ${formatShareRecipientName(user)}`}
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
                    label={formatShareRecipientName(user)}
                  />
                  <span className="min-w-0 flex-1 truncate text-sm font-medium">
                    {formatShareRecipientName(user)}
                  </span>
                  {directPubkeyUser?.pubkey === user.pubkey ? (
                    <span className="shrink-0 text-xs text-muted-foreground">
                      by public key
                    </span>
                  ) : null}
                </button>
              ))
            ) : (
              <p className="px-3 py-3 text-sm text-muted-foreground">
                No people found.
              </p>
            )}
          </div>
        </PopoverContent>
      </Popover>
    </div>
  );
}
