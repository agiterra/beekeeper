/**
 * The words the recipient picker is allowed to use about a key.
 *
 * Every "this key is an agent" signal in this app is positive and verifiable:
 * a NIP-OA owner attestation the native side checks before setting `isAgent`,
 * a managed-agent row on this disk, a relay agent registration. There is **no
 * positive signal that a key is a person.** `isAgent: false` means only "no
 * agent evidence this client holds", and three populations collapse into it:
 * people, agents whose owner never attested, and keys with no profile at all.
 *
 * So the tab is *People* and it means "no agent evidence we hold"; a key with
 * no profile is *unidentified*, never a confirmed anything; and no string in
 * this module asserts that a key belongs to a human being. The table test
 * beside this file enforces that last rule against the whole vocabulary.
 */

/** Which slice of the directory the picker is showing. */
export type ShareRecipientKind = "all" | "people" | "agents";

/** What a row is willing to claim about a key, from the shared vocabulary. */
export type ShareRecipientRowKind = "agent" | "unidentified";

export const SHARE_RECIPIENT_KIND_TABS: readonly {
  kind: ShareRecipientKind;
  label: string;
}[] = [
  { kind: "people", label: "People" },
  { kind: "agents", label: "Agents" },
  { kind: "all", label: "All" },
];

/** The label for the group of filter buttons. */
export const SHARE_RECIPIENT_KIND_GROUP_LABEL = "Filter recipients";

/**
 * Said in the surface, not only in a comment: what the active view actually
 * selected on. "People" is an absence of agent evidence, not an identity.
 */
export const SHARE_RECIPIENT_KIND_NOTES: Readonly<
  Record<ShareRecipientKind, string>
> = {
  all: "Everyone the directory returned, agents included.",
  agents:
    "Keys with agent evidence: a signed owner attestation, or an agent this computer knows.",
  people:
    "Keys we hold no agent evidence for. Absence of evidence, not proof of identity.",
};

/**
 * The view a caller gets. `kind` is what the caller asked for; when it asks
 * for nothing the view is derived from the long-standing `allowAgents` flag,
 * so every existing call site keeps exactly today's behaviour.
 */
export function resolveShareRecipientKind(input: {
  allowAgents: boolean;
  kind?: ShareRecipientKind;
}): ShareRecipientKind {
  return input.kind ?? (input.allowAgents ? "all" : "people");
}

/** Whether a search result carries a name we could show instead of its key. */
export function hasResolvedShareRecipientProfile(user: {
  displayName: string | null;
  nip05Handle: string | null;
}): boolean {
  return Boolean(user.displayName?.trim() || user.nip05Handle?.trim());
}

/**
 * What a row may say about one candidate.
 *
 * `label` is `null` for a named key with no agent evidence — the honest
 * output, because nothing here proves who that key belongs to. `showShortKey`
 * is false only when the row's name *is* the truncated key already.
 */
export function describeShareRecipientRow(input: {
  hasProfile: boolean;
  isAgent: boolean;
  ownerLabel: string | null;
}): {
  detail: string | null;
  label: string | null;
  rowKind: ShareRecipientRowKind | null;
  showShortKey: boolean;
} {
  if (input.isAgent) {
    return {
      detail: input.ownerLabel ? `Owner ${input.ownerLabel}` : "Owner unknown",
      label: "Agent",
      rowKind: "agent",
      showShortKey: input.hasProfile,
    };
  }

  if (!input.hasProfile) {
    return {
      detail: "No profile on this relay",
      label: "Unidentified",
      rowKind: "unidentified",
      showShortKey: false,
    };
  }

  return { detail: null, label: null, rowKind: null, showShortKey: true };
}

/**
 * The empty view.
 *
 * An agent-heavy first page used to render `"No people found."` while more
 * pages sat unfetched behind it — a flat false statement. That exact string is
 * now reserved for a view that has genuinely exhausted its pages; anything
 * else says only what it can see and offers one bounded press for the next
 * page (never a loop, never an automatic fetch on switching views).
 *
 * Each view also reports the population it actually searched. The All view
 * searched both, so "no people" would understate it — that view says "No
 * matches found." instead.
 */
export function describeShareRecipientEmptyState(input: {
  hasMoreResults: boolean;
  isLoadingMore: boolean;
  kind: ShareRecipientKind;
}): {
  hint: string | null;
  loadMoreLabel: string | null;
  message: string;
} {
  if (!input.hasMoreResults) {
    return {
      hint: null,
      loadMoreLabel: null,
      message:
        input.kind === "agents"
          ? "No agents found."
          : input.kind === "all"
            ? "No matches found."
            : "No people found.",
    };
  }

  return {
    hint: "More results have not been loaded yet. Keep typing to search by name or public key, or load the next page.",
    loadMoreLabel: input.isLoadingMore
      ? "Loading more results…"
      : "Load more results",
    message:
      input.kind === "agents"
        ? "No agents in the results loaded so far."
        : input.kind === "all"
          ? "No matches in the results loaded so far."
          : "No people in the results loaded so far.",
  };
}

/** Every string this module can render, for the vocabulary table test. */
export function listShareRecipientVocabulary(): string[] {
  const strings = [
    SHARE_RECIPIENT_KIND_GROUP_LABEL,
    ...SHARE_RECIPIENT_KIND_TABS.map((tab) => tab.label),
    ...Object.values(SHARE_RECIPIENT_KIND_NOTES),
  ];

  for (const hasProfile of [true, false]) {
    for (const isAgent of [true, false]) {
      for (const ownerLabel of ["Ada", null]) {
        const row = describeShareRecipientRow({
          hasProfile,
          isAgent,
          ownerLabel,
        });
        if (row.label) strings.push(row.label);
        if (row.detail) strings.push(row.detail);
      }
    }
  }

  for (const kind of ["all", "people", "agents"] as const) {
    for (const hasMoreResults of [true, false]) {
      for (const isLoadingMore of [true, false]) {
        const empty = describeShareRecipientEmptyState({
          hasMoreResults,
          isLoadingMore,
          kind,
        });
        strings.push(empty.message);
        if (empty.hint) strings.push(empty.hint);
        if (empty.loadMoreLabel) strings.push(empty.loadMoreLabel);
      }
    }
  }

  return strings;
}
