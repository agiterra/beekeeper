/**
 * Which agents a project's *published* associations name — the second computer's
 * view of project membership (`docs/PROJECT_AGENT_HIRING_IMPL.md` § Association).
 *
 * An agent's owner publishes kind:30177 with `project_digest`. Readers accept
 * that claim only from the project's creator or a roster owner or
 * collaborator: anyone can publish a 30177 naming any digest, and a viewer's
 * claim must not put an agent on somebody else's project.
 *
 * Pure: the hook (`usePublishedProjectAgents`) does the reading.
 */
import type { ProjectMember } from "@/features/projects-container/lib/projectContainerModel";
import {
  normalizeProjectCoordinate,
  PROJECT_AGENT_ASSOCIATION_AUTHOR_ROLES,
  projectAgentDigest,
  type PublishedAgentAssociation,
  readPublishedAgentAssociation,
} from "@/shared/lib/projectAgentAssociation";
import { normalizePubkey } from "@/shared/lib/pubkey";

const HEX64 = /^[0-9a-f]{64}$/;

/** The newest-first ceiling one read asks for; a full page is disclosed. */
export const PUBLISHED_PROJECT_AGENTS_READ_LIMIT = 1_000;

/**
 * The pubkeys whose association claims count for a project: its creator, then
 * roster members whose role may associate agents. Lowercased, deduplicated,
 * sorted — a stable query key.
 */
export function projectAgentAuthorizedAuthors(
  creatorPubkey: string | null | undefined,
  roster: readonly Pick<ProjectMember, "pubkey" | "role">[],
): string[] {
  const authors = new Set<string>();
  const creator = normalizePubkey(creatorPubkey ?? "");
  if (HEX64.test(creator)) authors.add(creator);
  for (const member of roster) {
    if (!PROJECT_AGENT_ASSOCIATION_AUTHOR_ROLES.includes(member.role)) continue;
    const pubkey = normalizePubkey(member.pubkey);
    if (HEX64.test(pubkey)) authors.add(pubkey);
  }
  return [...authors].sort();
}

type WireEvent = {
  id?: string;
  kind: number;
  pubkey: string;
  content: string;
  created_at: number;
  tags: readonly (readonly string[])[];
};

function newer(
  candidate: { createdAt: number; id: string },
  existing: { createdAt: number; id: string },
): boolean {
  if (candidate.createdAt !== existing.createdAt) {
    return candidate.createdAt > existing.createdAt;
  }
  // NIP-01 replaceable tie-break: the lowest id wins.
  return candidate.id < existing.id;
}

/**
 * Whether the author of an accepted claim was proven to hold project authority.
 *
 * - `verified` — the author is the project's creator (named by the coordinate
 *   itself), or an owner or collaborator on a roster that was read
 *   successfully (the kind:39010 projection, or the head's `p` tags when no
 *   projection exists).
 * - `unverified` — the roster read failed, and the author is authorized only
 *   by the fallback member list. Such a row is never counted as a project
 *   agent.
 */
export type PublishedProjectAgentAuthority = "verified" | "unverified";

/** An accepted published association, with how its author was authorized. */
export type PublishedProjectAgent = PublishedAgentAssociation & {
  authority: PublishedProjectAgentAuthority;
};

/**
 * The published associations that place an agent in `projectRef`.
 *
 * 1. Events from anyone outside `authorizedAuthors` are dropped.
 * 2. The newest event per `(author, d)` is that owner's current claim — an
 *    older claim naming this project is superseded by a newer one that does
 *    not.
 * 3. Per agent, the newest current claim across authorized authors decides.
 * 4. It counts only when its digest is this project's.
 * 5. Its authority is `verified` when `rosterVerified` (the authors came from
 *    a roster that was read) or the author is the creator; otherwise
 *    `unverified`.
 */
export function acceptPublishedProjectAgents(input: {
  events: readonly WireEvent[];
  projectRef: string | null;
  authorizedAuthors: readonly string[];
  /** The roster behind `authorizedAuthors` was read successfully. */
  rosterVerified: boolean;
}): PublishedProjectAgent[] {
  const digest = projectAgentDigest(input.projectRef);
  if (digest === null) return [];
  const authorized = new Set(input.authorizedAuthors.map(normalizePubkey));
  const creator = normalizeProjectCoordinate(input.projectRef)?.split(":")[1];

  type Current = { association: PublishedAgentAssociation; id: string };
  const byAuthorAgent = new Map<string, Current>();
  for (const event of input.events) {
    const author = normalizePubkey(event.pubkey);
    if (!authorized.has(author)) continue;
    const association = readPublishedAgentAssociation(event);
    if (!association) continue;
    const next: Current = {
      association: { ...association, ownerPubkey: author },
      id: event.id ?? "",
    };
    const key = `${author}|${association.pubkey}`;
    const existing = byAuthorAgent.get(key);
    if (
      !existing ||
      newer(
        { createdAt: association.createdAt, id: next.id },
        {
          createdAt: existing.association.createdAt,
          id: existing.id,
        },
      )
    ) {
      byAuthorAgent.set(key, next);
    }
  }

  const byAgent = new Map<string, Current>();
  for (const current of byAuthorAgent.values()) {
    const key = current.association.pubkey;
    const existing = byAgent.get(key);
    if (
      !existing ||
      newer(
        { createdAt: current.association.createdAt, id: current.id },
        { createdAt: existing.association.createdAt, id: existing.id },
      )
    ) {
      byAgent.set(key, current);
    }
  }

  return [...byAgent.values()]
    .map(
      (current): PublishedProjectAgent => ({
        ...current.association,
        authority:
          input.rosterVerified || current.association.ownerPubkey === creator
            ? "verified"
            : "unverified",
      }),
    )
    .filter((association) => association.projectDigest === digest)
    .sort((a, b) => (a.pubkey < b.pubkey ? -1 : a.pubkey > b.pubkey ? 1 : 0));
}

/**
 * Whether other computers' published agents may be read for a project.
 *
 * Associations are published only for public projects, so a private project
 * is never queried: an agent claiming its digest would be a claim no owner
 * made on purpose, and the query itself would name the project's authors.
 */
export function readsPublishedProjectAgents(
  project: { visibility: "public" | "private" } | null,
): boolean {
  return project !== null && project.visibility !== "private";
}

/** Whether the viewer may associate an agent with this project, and why not. */
export type ProjectAgentAssociateAccess =
  | { kind: "allowed" }
  | { kind: "denied"; reason: string };

/**
 * The viewer may associate an agent only when their own claim would be
 * accepted by every reader: they are the creator or a roster owner or
 * collaborator. Unknown is disclosed as unknown, never guessed as allowed.
 */
export function projectAgentAssociateAccess(input: {
  selfPubkey: string | null;
  creatorPubkey: string | null;
  roster: readonly Pick<ProjectMember, "pubkey" | "role">[];
  projectName: string;
  rosterLoading: boolean;
  rosterError: string | null;
  identityError: string | null;
}): ProjectAgentAssociateAccess {
  if (input.identityError) {
    return {
      kind: "denied",
      reason: `Your identity could not be read, so your access to ${input.projectName} is unknown: ${input.identityError}`,
    };
  }
  const self = input.selfPubkey ? normalizePubkey(input.selfPubkey) : null;
  if (!self) {
    return {
      kind: "denied",
      reason: `Checking your access to ${input.projectName}…`,
    };
  }
  if (!input.creatorPubkey) {
    return {
      kind: "denied",
      reason: `${input.projectName} has no published owner, so nobody's association would be accepted.`,
    };
  }
  if (normalizePubkey(input.creatorPubkey) === self) return { kind: "allowed" };
  if (input.rosterLoading) {
    return {
      kind: "denied",
      reason: `Checking your access to ${input.projectName}…`,
    };
  }
  if (input.rosterError) {
    return {
      kind: "denied",
      reason: `${input.projectName}'s members could not be read, so your access is unknown: ${input.rosterError}`,
    };
  }
  const authors = projectAgentAuthorizedAuthors(
    input.creatorPubkey,
    input.roster,
  );
  if (authors.includes(self)) return { kind: "allowed" };
  return {
    kind: "denied",
    reason: `Only ${input.projectName}'s owners and collaborators can associate agents with it.`,
  };
}
