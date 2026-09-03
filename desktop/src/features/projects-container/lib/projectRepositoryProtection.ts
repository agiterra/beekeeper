/**
 * The `buzz-protect` rules that govern a repository — the announcement's own
 * rows, and every founder's signed rule record.
 *
 * Tag grammar (`crates/buzz-core/src/git_perms.rs`):
 * `["buzz-protect", "<ref-pattern>", "<rule1>", "<rule2>", ...]`.
 *
 * **Who may set a rule.** Until lane L26, only the announcement's signer:
 * kind:30617 is addressable by `(kind, pubkey, d)`, so a co-founder
 * republishing it published a second repository instead of editing the rules
 * of the one they co-founded (finding 33 R2). Now **any founder** — the
 * signer, a NIP-34 `maintainers` entry, or an Owner on the roster of the
 * project the repository back-references — may set or remove a rule by
 * signing a rule record (kind 30625,
 * `crates/buzz-core/src/repository_protection.rs`). The relay admits one only
 * from a founder, and its push gate resolves records against the
 * announcement's rows with last write wins per exact ref pattern.
 *
 * **Read-optional.** A repository with no rule record is governed by its
 * announcement exactly as it always was — the "signed before the kind
 * existed" case, which {@link resolveProtection} returns unchanged and which
 * this panel labels `announcement`.
 */
import { useQuery } from "@tanstack/react-query";

import { eventToRepository } from "@/features/projects/projectModels";
import type { Repository } from "@/features/projects/projectModels";
import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_REPO_ANNOUNCEMENT,
  KIND_REPO_PROTECTION,
} from "@/shared/constants/kinds";

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
/** Content schema of a kind:30625 rule record. */
const RULE_RECORD_SCHEMA = "buzz-repo-protection/v1";
/**
 * The rule token meaning "this pattern carries no rules".
 *
 * Removal has to be a row rather than an absence: a record only takes a
 * pattern over by naming it, so dropping the row would fall back to the
 * announcement's rule — the opposite of removing it. Mirrors
 * `PROTECTION_RULE_CLEAR` in `crates/buzz-core/src/git_perms.rs`.
 */
export const PROTECTION_RULE_CLEAR = "none";

/** Which record a governing rule came from. */
export type ProtectionSource =
  | { record: "announcement"; signedBy: string; eventId: string }
  | { record: "rule-record"; signedBy: string; eventId: string };

/** One exact ref pattern, the rules that govern it, and which record says so. */
export type ProtectionDecision = {
  refPattern: string;
  rules: readonly string[];
  /** Whether the winning record cleared the pattern rather than ruling on it. */
  cleared: boolean;
  source: ProtectionSource;
};

/** One layer of the resolution: a record, its time, and its rows. */
export type ProtectionLayer = {
  source: ProtectionSource;
  createdAt: number;
  rules: readonly ProtectionRule[];
};

/**
 * The `d` tag addressing a repository's rule records.
 *
 * Mirrors `repository_protection_d_tag`: lower-cased owner, repository id
 * verbatim, joined by the first colon.
 */
export function ruleRecordDTag(owner: string, dtag: string): string {
  return `${owner.trim().toLowerCase()}:${dtag}`;
}

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

/**
 * Whether the governing decisions leave `refs/heads/main` requiring a verdict.
 *
 * A cleared pattern is not protection: a founder said out loud that it carries
 * no rules, and the switch must show off rather than showing the rule the
 * clear replaced.
 */
export function requireVerdictFromDecisions(
  decisions: readonly ProtectionDecision[],
): boolean {
  const decision = decisions.find((d) => d.refPattern === PROTECTED_MAIN_REF);
  if (!decision || decision.cleared) return false;
  return decision.rules.includes(REQUIRE_VERDICT);
}

/**
 * Every founder of a repository: its announcement's signer, its NIP-34
 * `maintainers`, and the project-roster Owners the caller resolved.
 *
 * Mirrors `RepositoryFounders` (`crates/buzz-core/src/repository_founders.rs`)
 * for the one question this panel asks — may this viewer set a rule.
 */
export function repositoryFounders(
  repository: Pick<Repository, "owner" | "maintainers">,
  rosterOwners: readonly string[] = [],
): string[] {
  const founders: string[] = [];
  const add = (value: string | undefined) => {
    const candidate = value?.trim().toLowerCase() ?? "";
    if (!/^[0-9a-f]{64}$/.test(candidate)) return;
    if (!founders.includes(candidate)) founders.push(candidate);
  };
  add(repository.owner);
  for (const maintainer of repository.maintainers ?? []) add(maintainer);
  for (const owner of rosterOwners) add(owner);
  return founders;
}

/**
 * Decode one signed kind:30625 into a layer, or null when it is not a rule
 * record for this repository or its author does not found it.
 *
 * The founder filter is the relay's own: a record written by yesterday's
 * founder governs nothing today.
 */
export function ruleRecordLayer(
  event: RelayEvent,
  owner: string,
  dtag: string,
  founders: readonly string[],
): ProtectionLayer | null {
  if (event.kind !== KIND_REPO_PROTECTION) return null;
  const address = event.tags.find((tag) => tag[0] === "d")?.[1];
  if (address !== ruleRecordDTag(owner, dtag)) return null;
  let schema: unknown;
  try {
    schema = (JSON.parse(event.content) as { schema?: unknown }).schema;
  } catch {
    return null;
  }
  if (schema !== RULE_RECORD_SCHEMA) return null;
  const author = event.pubkey.toLowerCase();
  if (!founders.includes(author)) return null;
  return {
    createdAt: event.created_at,
    rules: parseProtectionTags(event.tags),
    source: { eventId: event.id, record: "rule-record", signedBy: author },
  };
}

/**
 * Resolve the announcement's rows against every founder's rule record.
 *
 * Last write wins **per exact ref pattern**: for each pattern string, the
 * record with the newest `created_at` (ties broken on the greater event id, so
 * two founders acting in the same second resolve the same way here as at the
 * relay) contributes all of its rows and the rest are dropped. Mirrors
 * `resolve_protection_layers` in
 * `crates/buzz-core/src/repository_protection.rs`.
 *
 * With no records this returns exactly the announcement's own rules, every
 * decision labelled `announcement` — the "signed before the kind existed"
 * case.
 */
export function resolveProtection(input: {
  announcement: Pick<RelayEvent, "id" | "pubkey" | "created_at" | "tags">;
  records: readonly ProtectionLayer[];
}): ProtectionDecision[] {
  const layers: ProtectionLayer[] = [
    {
      createdAt: input.announcement.created_at,
      rules: parseProtectionTags(input.announcement.tags),
      source: {
        eventId: input.announcement.id,
        record: "announcement",
        signedBy: input.announcement.pubkey.toLowerCase(),
      },
    },
    ...input.records,
  ];
  const patterns = [
    ...new Set(
      layers.flatMap((layer) => layer.rules.map((rule) => rule.refPattern)),
    ),
  ].sort();
  return patterns.map((refPattern) => {
    const naming = layers
      .filter((layer) =>
        layer.rules.some((rule) => rule.refPattern === refPattern),
      )
      .sort(
        (a, b) =>
          b.createdAt - a.createdAt ||
          b.source.eventId.localeCompare(a.source.eventId),
      );
    const winner = naming[0];
    const rules = winner.rules
      .filter((rule) => rule.refPattern === refPattern)
      .flatMap((rule) => [...rule.rules]);
    // A record that carries only the clear token has removed the rule; one
    // that carries a real rule alongside it has said something, and the
    // something wins over the nothing.
    const cleared =
      rules.length > 0 && rules.every((rule) => rule === PROTECTION_RULE_CLEAR);
    return { cleared, refPattern, rules, source: winner.source };
  });
}

/** Every stored rule record addressing this repository. */
export async function fetchRuleRecords(input: {
  owner: string;
  dtag: string;
}): Promise<RelayEvent[]> {
  return relayClient.fetchEvents({
    kinds: [KIND_REPO_PROTECTION],
    "#d": [ruleRecordDTag(input.owner, input.dtag)],
    limit: 64,
  });
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
  /**
   * Which record the rule landed in. Not decoration: "your rule is live" and
   * "your rule is live in a second record the relay resolves against the
   * announcement" are different facts, and the second is the one a person
   * needs when they go looking for it.
   */
  record: "announcement" | "rule-record";
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
  /**
   * The viewer's own pubkey. When it is not the announcement's signer, the
   * toggle signs a **rule record** instead of republishing an announcement it
   * cannot address — which is what a co-founder's toggle used to do wrong.
   */
  viewerPubkey?: string | null;
}): Promise<ProtectionTogglePublished> {
  const current = await fetchRepositoryAnnouncementEvent(input);
  if (!current) {
    throw new Error(
      "This repository's own announcement could not be read, so nothing was signed.",
    );
  }
  const viewer = input.viewerPubkey?.toLowerCase() ?? null;
  const signer = current.pubkey.toLowerCase();
  if (viewer !== null && viewer !== signer) {
    return publishRuleRecord({ ...input, viewer });
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
  return {
    eventId: event.id,
    record: "announcement",
    rules: parseProtectionTags(event.tags),
  };
}

/**
 * The `buzz-protect` rows a founder's next rule record carries: their current
 * rows with `refs/heads/main` replaced by the rule (or by the clear token).
 *
 * Exported so the rewrite is testable without reaching the relay, and so the
 * one rule this panel writes is spelled in exactly one place.
 */
export function applyRuleRecordRows(
  current: readonly (readonly string[])[],
  enabled: boolean,
): string[][] {
  const rows = current
    .filter((tag) => tag[0] === PROTECT_TAG && tag[1] !== PROTECTED_MAIN_REF)
    .map((tag) => [...tag]);
  rows.push([
    PROTECT_TAG,
    PROTECTED_MAIN_REF,
    enabled ? REQUIRE_VERDICT : PROTECTION_RULE_CLEAR,
  ]);
  return rows;
}

/**
 * Sign and publish this founder's own rule record.
 *
 * `created_at` is advanced past whatever currently wins the pattern, because
 * the resolution is last-write-wins and a record stamped behind the
 * announcement would be published, accepted, and silently ignored.
 */
async function publishRuleRecord(input: {
  owner: string;
  dtag: string;
  enabled: boolean;
  viewer: string;
}): Promise<ProtectionTogglePublished> {
  const records = await fetchRuleRecords(input);
  const mine = records
    .filter((event) => event.pubkey.toLowerCase() === input.viewer)
    .sort((a, b) => b.created_at - a.created_at)[0];
  const newest = records.reduce(
    (max, event) => Math.max(max, event.created_at),
    0,
  );
  const announcement = await fetchRepositoryAnnouncementEvent(input);
  const head = Math.max(newest, announcement?.created_at ?? 0);
  const createdAt = Math.max(head + 1, Math.floor(Date.now() / 1000));
  const event = await signRelayEvent({
    kind: KIND_REPO_PROTECTION,
    content: JSON.stringify({ schema: RULE_RECORD_SCHEMA }),
    createdAt,
    tags: [
      ["d", ruleRecordDTag(input.owner, input.dtag)],
      ...applyRuleRecordRows(mine?.tags ?? [], input.enabled),
    ],
  });
  await relayClient.publishEvent(
    event,
    "Timed out setting the protection rule.",
    "Failed to set the protection rule.",
  );
  return {
    eventId: event.id,
    record: "rule-record",
    rules: parseProtectionTags(event.tags),
  };
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
