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
 * The published associations that place an agent in `projectRef`.
 *
 * 1. Events from anyone outside `authorizedAuthors` are dropped.
 * 2. The newest event per `(author, d)` is that owner's current claim — an
 *    older claim naming this project is superseded by a newer one that does
 *    not.
 * 3. Per agent, the newest current claim across authorized authors decides.
 * 4. It counts only when its digest is this project's.
 */
export function acceptPublishedProjectAgents(input: {
  events: readonly WireEvent[];
  projectRef: string | null;
  authorizedAuthors: readonly string[];
}): PublishedAgentAssociation[] {
  const digest = projectAgentDigest(input.projectRef);
  if (digest === null) return [];
  const authorized = new Set(input.authorizedAuthors.map(normalizePubkey));

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
    .map((current) => current.association)
    .filter((association) => association.projectDigest === digest)
    .sort((a, b) => (a.pubkey < b.pubkey ? -1 : a.pubkey > b.pubkey ? 1 : 0));
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
