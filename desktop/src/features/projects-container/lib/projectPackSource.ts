/**
 * A project's persona-pack source: which git repository (and pinned commit)
 * a project's coding-session seats stage their role packs from (LANE-L23).
 *
 * Kind 30624, addressable — `d` = the project's own coordinate
 * (`30621:<owner-hex>:<slug>`). Newest wins, same as every other addressable
 * event this app reads. Author must be a founder of one of the project's
 * repositories (L18's `RepositoryFounders`) or the project owner; the relay
 * refuses others, so the client-side check in
 * {@link canSetProjectPackSource} is advisory only, exactly like
 * `ProjectCapabilities`'s own documented rule.
 */
import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_PROJECT_PACK_SOURCE } from "@/shared/constants/kinds";

import type { ProjectMember } from "./projectContainerModel";
import type { ProjectContainer } from "../hooks";
import type { Repository } from "@/features/projects/projectModels";

const SCHEMA = "buzz-project-pack-source/v1";
const MAX_NOTE_BYTES = 512;
const encoder = new TextEncoder();

/** One project's pack source, exactly as the newest signed 30624 states it. */
export type ProjectPackSource = {
  eventId: string;
  /** Lowercase 64-hex — the founder or owner who signed this source. */
  author: string;
  /** Unix seconds. */
  createdAt: number;
  /** The packs repository coordinate, `30617:<owner-hex>:<id>`. */
  repo: string;
  /** Exactly one of `ref`/`sha` is non-null — the wire's own singleton rule. */
  ref: string | null;
  sha: string | null;
  /** The base path within the repo. Defaults to `personas/roles`. */
  path: string;
  /** Optional free-text note, ≤512 bytes. */
  note: string | null;
};

/** React Query key for one project's newest pack-source head. */
export function projectPackSourceQueryKey(projectCoord: string) {
  return ["project-pack-source", projectCoord] as const;
}

/** Bare repository id used by relay-signed kind:30618 ref-state events. */
export function projectPackSourceRepoId(
  source: Pick<ProjectPackSource, "repo">,
): string {
  return source.repo.slice(source.repo.lastIndexOf(":") + 1);
}

function tagValue(tags: string[][], name: string): string | null {
  return tags.find((tag) => tag[0] === name)?.[1] ?? null;
}

const SHA_40 = /^[0-9a-f]{40}$/;
/** `30617:<64-hex>:<dtag>` — a git repository announcement coordinate. */
const REPO_COORD = /^30617:[0-9a-f]{64}:[a-zA-Z0-9._-]{1,200}$/;

/**
 * Read one signed kind:30624 into {@link ProjectPackSource}, or `null` when
 * the event does not carry the shape this reader recognises. Strict on
 * purpose, same discipline as `readPackRef`: a shape this reader does not
 * fully recognise is not shown, rather than a partial or guessed reading.
 */
export function parseProjectPackSourceEvent(
  event: RelayEvent,
): ProjectPackSource | null {
  if (event.kind !== KIND_PROJECT_PACK_SOURCE) return null;
  const repo = tagValue(event.tags, "repo");
  if (repo === null || !REPO_COORD.test(repo)) return null;
  const ref = tagValue(event.tags, "ref");
  const sha = tagValue(event.tags, "sha");
  // Exactly one of ref/sha — the wire's own rule.
  if ((ref === null) === (sha === null)) return null;
  if (sha !== null && !SHA_40.test(sha)) return null;
  const path = tagValue(event.tags, "path") ?? "personas/roles";
  if (path.length === 0 || path.length > 512) return null;

  let note: string | null = null;
  if (event.content.trim().length > 0) {
    let parsed: unknown;
    try {
      parsed = JSON.parse(event.content);
    } catch {
      return null;
    }
    if (
      typeof parsed !== "object" ||
      parsed === null ||
      Array.isArray(parsed) ||
      (parsed as Record<string, unknown>).schema !== SCHEMA
    ) {
      return null;
    }
    const record = parsed as Record<string, unknown>;
    const rawNote = record.note;
    if (rawNote !== undefined) {
      if (
        typeof rawNote !== "string" ||
        encoder.encode(rawNote).length > MAX_NOTE_BYTES
      ) {
        return null;
      }
      note = rawNote.length > 0 ? rawNote : null;
    }
  }

  return {
    eventId: event.id,
    author: event.pubkey.toLowerCase(),
    createdAt: event.created_at,
    repo,
    ref,
    sha,
    path,
    note,
  };
}

/** The newest (by `created_at`) valid pack source among a set of 30624 events. */
export function newestProjectPackSource(
  events: readonly RelayEvent[],
): ProjectPackSource | null {
  let newest: ProjectPackSource | null = null;
  for (const event of events) {
    const parsed = parseProjectPackSourceEvent(event);
    if (parsed === null) continue;
    if (newest === null || parsed.createdAt > newest.createdAt) {
      newest = parsed;
    }
  }
  return newest;
}

/** Fetch and decode the newest pack source for one project, or `null`. */
export async function fetchProjectPackSource(
  projectCoord: string,
): Promise<ProjectPackSource | null> {
  const events = await relayClient.fetchEvents({
    kinds: [KIND_PROJECT_PACK_SOURCE],
    "#d": [projectCoord],
    limit: 4,
  });
  return newestProjectPackSource(events);
}

/**
 * Publish a new kind:30624 for this project, straight from the desktop —
 * `signRelayEvent` + `relayClient.publishEvent`, the same founder-act publish
 * path `publishProjectContainer` uses for kind:30621. Unlike the coding-
 * session team-transaction path (kind:44244), this event's tags and content
 * are simple and fully specified by the wire contract, so there is no Rust
 * "build" round trip to keep byte-identical — the relay is still the final
 * word on whether the signer may publish it at all.
 */
export async function publishProjectPackSource(input: {
  projectCoord: string;
  repoCoord: string;
  /** Exactly one of `ref`/`sha` must be given. */
  ref?: string | null;
  sha?: string | null;
  /** Omitted or empty publishes no `path` tag — the relay's own default applies. */
  path?: string | null;
  note?: string | null;
}): Promise<ProjectPackSource> {
  const ref = input.ref?.trim() || null;
  const sha = input.sha?.trim().toLowerCase() || null;
  if ((ref === null) === (sha === null)) {
    throw new Error(
      "A pack source names exactly one of a ref or a sha, so nothing was signed.",
    );
  }
  if (sha !== null && !SHA_40.test(sha)) {
    throw new Error(
      "A pack source's sha is 40 lowercase hex characters, so nothing was signed.",
    );
  }

  const tags: string[][] = [
    ["d", input.projectCoord],
    ["repo", input.repoCoord],
    ref !== null ? ["ref", ref] : ["sha", sha as string],
  ];
  const path = input.path?.trim();
  if (path) tags.push(["path", path]);

  const note = input.note?.trim() || null;
  if (note !== null && encoder.encode(note).length > MAX_NOTE_BYTES) {
    throw new Error(
      `A pack source's note is longer than ${MAX_NOTE_BYTES} bytes, so nothing was signed. Shorten it and try again.`,
    );
  }

  const event = await signRelayEvent({
    kind: KIND_PROJECT_PACK_SOURCE,
    content: JSON.stringify({
      schema: SCHEMA,
      ...(note !== null ? { note } : {}),
    }),
    tags,
  });

  await relayClient.publishEvent(
    event,
    "Timed out setting the pack source.",
    "Failed to set the pack source.",
  );

  const parsed = parseProjectPackSourceEvent(event);
  if (!parsed) {
    throw new Error("Failed to read back the pack source just published.");
  }
  return parsed;
}

/**
 * Whether `self` may set this project's pack source, by the same rule the
 * relay itself enforces: a founder of one of the project's repositories (its
 * signer, its NIP-34 `maintainers`, or a project-roster Owner — finding 33's
 * own definition) or the project owner. Advisory only, like every
 * `ProjectCapabilities` check — the relay re-decides this against its own
 * `RepositoryFounders` projection, and a client that got it wrong gets a
 * refusal, not a privilege.
 */
export function canSetProjectPackSource(input: {
  self: string | null;
  project: Pick<ProjectContainer, "owner"> | null;
  roster: readonly ProjectMember[];
  /** The repository this project's pack source would name, when known. */
  repo: Pick<Repository, "owner" | "maintainers"> | null;
}): boolean {
  const self = input.self?.toLowerCase() ?? null;
  if (!self) return false;
  if (input.project?.owner.toLowerCase() === self) return true;
  if (
    input.roster.some(
      (member) =>
        member.pubkey.toLowerCase() === self && member.role === "owner",
    )
  ) {
    return true;
  }
  if (!input.repo) return false;
  if (input.repo.owner.toLowerCase() === self) return true;
  return (
    input.repo.maintainers?.some(
      (maintainer) => maintainer.toLowerCase() === self,
    ) ?? false
  );
}
