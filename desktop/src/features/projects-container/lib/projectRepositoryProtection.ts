/**
 * `buzz-protect` rules on a repository's own kind:30617 announcement
 * (LANE-L23 addendum, 2026-09-03: "Project settings → Repository →
 * Protection").
 *
 * There is no separate protection event or endpoint — `bee repos protect
 * set/list` (`crates/buzz-cli/src/commands/repos.rs`) reads and rewrites tags
 * on the repository's own NIP-34 announcement:
 * `["buzz-protect", "<ref-pattern>", "<rule1>", "<rule2>", ...]`
 * (`crates/buzz-core/src/git_perms.rs`). This module mirrors that tag grammar
 * for a read-only listing, and — for exactly one rule, `require-verdict` on
 * `refs/heads/main` — the write path too.
 *
 * **Who may set a rule.** Kind:30617 is a NIP-01 addressable event
 * `(kind, pubkey, d)`; only the original signer's key can republish the
 * canonical head for that coordinate. So "who may set them" is simply the
 * announcement's own signer (`Repository.owner`, which is `event.pubkey` —
 * see `projectModels.ts`'s `eventToRepository`), never a maintainer or a
 * project-roster owner (finding 33's own distinction: maintainers may push
 * under a governed rule; only the signer may rewrite the rule itself).
 */
import { useQuery } from "@tanstack/react-query";

import { eventToRepository } from "@/features/projects/projectModels";
import type { Repository } from "@/features/projects/projectModels";
import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_REPO_ANNOUNCEMENT } from "@/shared/constants/kinds";

import type { ProjectContainer } from "../hooks";

/** One ref pattern's rules, exactly as the announcement's tags carry them. */
export type ProtectionRule = {
  refPattern: string;
  /** Raw rule tokens: `push:<role>`, `no-force-push`, `no-delete`, `require-patch`, `require-verdict`. */
  rules: readonly string[];
};

const PROTECT_TAG = "buzz-protect";
/** The one ref this panel offers a switch for. */
export const PROTECTED_MAIN_REF = "refs/heads/main";
const REQUIRE_VERDICT = "require-verdict";

/** Parse every `buzz-protect` tag off a repository announcement's raw tags. */
export function parseProtectionTags(
  tags: readonly (readonly string[])[],
): ProtectionRule[] {
  return tags
    .filter((tag) => tag[0] === PROTECT_TAG && typeof tag[1] === "string")
    .map((tag) => ({ refPattern: tag[1] as string, rules: tag.slice(2) }));
}

/** Whether `refs/heads/main` currently carries `require-verdict`. */
export function requireVerdictOnMain(
  rules: readonly ProtectionRule[],
): boolean {
  return (
    rules
      .find((rule) => rule.refPattern === PROTECTED_MAIN_REF)
      ?.rules.includes(REQUIRE_VERDICT) ?? false
  );
}

/** Fetch a repository's own newest kind:30617 announcement, or `null`. */
export async function fetchRepositoryAnnouncementEvent(input: {
  owner: string;
  dtag: string;
}): Promise<RelayEvent | null> {
  const events = await relayClient.fetchEvents({
    kinds: [KIND_REPO_ANNOUNCEMENT],
    authors: [input.owner.toLowerCase()],
    "#d": [input.dtag],
    limit: 4,
  });
  let newest: RelayEvent | null = null;
  for (const event of events) {
    if (newest === null || event.created_at > newest.created_at) newest = event;
  }
  return newest;
}

/** What one protection-toggle publish left on the wire. */
export type ProtectionTogglePublished = {
  eventId: string;
  rules: ProtectionRule[];
};

/**
 * Toggle `require-verdict` on `refs/heads/main`, by re-signing and
 * republishing the repository's own current announcement with exactly one
 * tag added, updated, or removed — every other tag (name, description,
 * clone URLs, maintainers, relays, …) carried through byte-for-byte from the
 * freshly-fetched event, never rebuilt from a stale client model.
 *
 * Same founder-act publish path as `publishProjectPackSource` —
 * `signRelayEvent` + `relayClient.publishEvent` — because a NIP-34 repo
 * announcement is exactly as simple a shape to republish as a project
 * container or a pack source: no Rust "build" round trip needed to keep two
 * decoders in byte agreement.
 */
/**
 * The pure tag transform {@link setRequireVerdictOnMain} publishes — exported
 * so the rewrite rule (replace-in-place, add, or drop-when-empty) is testable
 * without reaching the relay.
 */
export function applyRequireVerdictOnMain(
  tags: readonly (readonly string[])[],
  enabled: boolean,
): string[][] {
  const next = tags.map((tag) => [...tag]);
  const index = next.findIndex(
    (tag) => tag[0] === PROTECT_TAG && tag[1] === PROTECTED_MAIN_REF,
  );
  if (enabled) {
    if (index >= 0) {
      const existing = next[index].slice(2);
      if (!existing.includes(REQUIRE_VERDICT)) {
        next[index] = [
          PROTECT_TAG,
          PROTECTED_MAIN_REF,
          ...existing,
          REQUIRE_VERDICT,
        ];
      }
    } else {
      next.push([PROTECT_TAG, PROTECTED_MAIN_REF, REQUIRE_VERDICT]);
    }
  } else if (index >= 0) {
    const remaining = next[index]
      .slice(2)
      .filter((rule) => rule !== REQUIRE_VERDICT);
    if (remaining.length > 0) {
      next[index] = [PROTECT_TAG, PROTECTED_MAIN_REF, ...remaining];
    } else {
      next.splice(index, 1);
    }
  }
  return next;
}

export async function setRequireVerdictOnMain(input: {
  owner: string;
  dtag: string;
  enabled: boolean;
}): Promise<ProtectionTogglePublished> {
  const current = await fetchRepositoryAnnouncementEvent(input);
  if (!current) {
    throw new Error(
      "This repository's own announcement could not be read, so nothing was signed.",
    );
  }
  const tags = applyRequireVerdictOnMain(current.tags, input.enabled);

  const event = await signRelayEvent({
    kind: KIND_REPO_ANNOUNCEMENT,
    content: current.content,
    tags,
  });
  await relayClient.publishEvent(
    event,
    "Timed out setting the protection rule.",
    "Failed to set the protection rule.",
  );
  return { eventId: event.id, rules: parseProtectionTags(event.tags) };
}

/** Parse a `30617:<owner-hex>:<dtag>` coordinate into its two halves. */
function parseRepoCoord(coord: string): { owner: string; dtag: string } | null {
  const parts = coord.split(":");
  if (parts.length !== 3 || parts[0] !== String(KIND_REPO_ANNOUNCEMENT)) {
    return null;
  }
  return { owner: parts[1], dtag: parts[2] };
}

/** Query key for one project's linked repositories, by their coordinates. */
export function projectRepositoriesQueryKey(repoAddrs: readonly string[]) {
  return ["project-repository-protection", [...repoAddrs].sort()] as const;
}

/**
 * This project's own linked repositories, as full {@link Repository} objects
 * (owner, maintainers, and the raw tags this module reads `buzz-protect`
 * off) — fetched directly by coordinate (one `relayClient.fetchEvents` per
 * repo, same as `fetchRepositoryAnnouncementEvent` above), not through the
 * heavier sidebar-scale `useProjectsQuery`/`CommunitiesProvider` machinery
 * this panel has no other need for (it is "small" by the addendum's own
 * word).
 */
export function useProjectRepositories(project: ProjectContainer | null): {
  repositories: Repository[];
  isLoading: boolean;
} {
  const repoAddrs = project?.repoAddrs ?? [];
  const query = useQuery({
    enabled: repoAddrs.length > 0,
    queryFn: async () => {
      const coords = repoAddrs
        .map(parseRepoCoord)
        .filter(
          (coord): coord is { owner: string; dtag: string } => coord !== null,
        );
      const events = await Promise.all(
        coords.map((coord) => fetchRepositoryAnnouncementEvent(coord)),
      );
      const repositories: Repository[] = [];
      for (const event of events) {
        if (!event) continue;
        const repository = eventToRepository(event);
        if (repository) repositories.push(repository);
      }
      return repositories;
    },
    queryKey: projectRepositoriesQueryKey(repoAddrs),
  });
  return { repositories: query.data ?? [], isLoading: query.isLoading };
}
