/**
 * A managed agent's durable project association — the one rule every surface
 * (hire host, session setup, Agents screens) reads.
 *
 * An agent is a durable, named project participant with a primary role. The
 * association is recorded on the owner's computer (`ManagedAgent.projectRef`)
 * and published on the agent's owner-signed kind:30177 as a digest
 * (`crates/buzz-core/src/project_agent_association.rs`), so a second computer
 * and a lead's CLI can discover the roster without anybody's disk.
 *
 * What is **not** association, and must never be read as it: a matching role
 * name, a role pack installed on this computer, channel membership, having
 * once held a seat in the project, or a role file's instructions.
 */
import { sha256 } from "@noble/hashes/sha2.js";
import { bytesToHex } from "@noble/hashes/utils.js";

/** Mirrors `PROJECT_AGENT_DIGEST_DOMAIN` in buzz-core. */
export const PROJECT_AGENT_DIGEST_DOMAIN = "buzz-project-agent/v1\n";

/** kind:30177 content keys carrying the association (buzz-core constants). */
export const PROJECT_AGENT_DIGEST_CONTENT_KEY = "project_digest";
export const PROJECT_AGENT_ROLE_CONTENT_KEY = "home_role";

/**
 * `30621:<lowercase-owner-hex>:<dtag>`, or null when `value` is not a
 * well-formed project coordinate. Mirrors buzz-core
 * `normalize_project_coordinate` (after trimming).
 */
export function normalizeProjectCoordinate(
  value: string | null | undefined,
): string | null {
  if (typeof value !== "string") return null;
  const trimmed = trimAsciiWhitespace(value);
  const first = trimmed.indexOf(":");
  if (first < 0) return null;
  const second = trimmed.indexOf(":", first + 1);
  if (second < 0) return null;
  const kind = trimmed.slice(0, first);
  const owner = trimmed.slice(first + 1, second);
  const dtag = trimmed.slice(second + 1);
  if (kind !== "30621") return null;
  if (!/^[0-9a-fA-F]{64}$/.test(owner)) return null;
  const codePoints = [...dtag];
  if (codePoints.length === 0 || codePoints.length > 64) return null;
  // Mirrors Rust `char::is_control` (C0, DEL and C1). A code-point check,
  // not a regex: an escaped control range is rewritten into literal control
  // bytes by the formatter.
  if (
    codePoints.some((ch) => {
      const code = ch.codePointAt(0) ?? 0;
      return code <= 0x1f || (code >= 0x7f && code <= 0x9f);
    })
  ) {
    return null;
  }
  return `30621:${owner.toLowerCase()}:${dtag}`;
}

/**
 * Trim only ASCII whitespace (space, tab, LF, FF, CR) — Rust
 * `char::is_ascii_whitespace` — so both digests trim exactly the same bytes.
 * `String.prototype.trim` also strips U+FEFF and others Rust keeps.
 */
function trimAsciiWhitespace(value: string): string {
  return value.replace(/^[ \t\n\f\r]+|[ \t\n\f\r]+$/g, "");
}

/** Lowercase hex digest a kind:30177 carries for `coordinate`, or null. */
export function projectAgentDigest(
  coordinate: string | null | undefined,
): string | null {
  const normalized = normalizeProjectCoordinate(coordinate);
  if (normalized === null) return null;
  return bytesToHex(
    sha256(new TextEncoder().encode(PROJECT_AGENT_DIGEST_DOMAIN + normalized)),
  );
}

/** Whether two project refs name the same project. Null never matches. */
export function sameProjectRef(
  left: string | null | undefined,
  right: string | null | undefined,
): boolean {
  const a = normalizeProjectCoordinate(left);
  return a !== null && a === normalizeProjectCoordinate(right);
}

/**
 * How one local agent relates to a project (or to a projectless session when
 * `projectRef` is null).
 *
 * - `project` — associated with exactly this project.
 * - `other-project` — associated with a different project.
 * - `unassociated` — associated with no project at all.
 *
 * For a projectless session (`projectRef === null`) an unassociated agent is
 * `project` (it may take the seat) and an associated one is `other-project`.
 */
export type AgentProjectRelation = "project" | "other-project" | "unassociated";

export function agentProjectRelation(
  agent: { projectRef?: string | null },
  projectRef: string | null | undefined,
): AgentProjectRelation {
  const own = normalizeProjectCoordinate(agent.projectRef ?? null);
  const named =
    typeof projectRef === "string" && trimAsciiWhitespace(projectRef) !== "";
  const target = normalizeProjectCoordinate(projectRef ?? null);
  // A session that names a project this helper cannot read is not a
  // projectless session: it matches no agent rather than admitting every
  // unassociated one.
  if (named && target === null) return "other-project";
  if (target === null) return own === null ? "project" : "other-project";
  if (own === null) return "unassociated";
  return own === target ? "project" : "other-project";
}

/** Whether this local agent may be selected for work in `projectRef`. */
export function agentMaySeatInProject(
  agent: { projectRef?: string | null },
  projectRef: string | null | undefined,
): boolean {
  return agentProjectRelation(agent, projectRef) === "project";
}

/** The association a kind:30177 content claims, as read off the wire. */
export type PublishedAgentAssociation = {
  /** Agent pubkey (the event's `d` tag). */
  pubkey: string;
  /** The event's author: the agent's owner, whose authority must be checked. */
  ownerPubkey: string;
  name: string;
  homeRole: string | null;
  projectDigest: string | null;
  createdAt: number;
};

/**
 * Read a kind:30177 event's association claim. Null when the event is not a
 * readable 30177 (missing `d`, content not an object, no name). Never throws.
 */
export function readPublishedAgentAssociation(event: {
  kind: number;
  pubkey: string;
  content: string;
  created_at: number;
  tags: readonly (readonly string[])[];
}): PublishedAgentAssociation | null {
  if (event.kind !== 30177) return null;
  const d = event.tags.find((tag) => tag[0] === "d")?.[1];
  if (typeof d !== "string" || !/^[0-9a-f]{64}$/.test(d)) return null;
  let content: unknown;
  try {
    content = JSON.parse(event.content);
  } catch {
    return null;
  }
  if (typeof content !== "object" || content === null) return null;
  const record = content as Record<string, unknown>;
  if (typeof record.name !== "string") return null;
  const role = record[PROJECT_AGENT_ROLE_CONTENT_KEY];
  const digest = record[PROJECT_AGENT_DIGEST_CONTENT_KEY];
  return {
    pubkey: d,
    ownerPubkey: event.pubkey,
    name: record.name,
    homeRole: typeof role === "string" && role.trim() ? role.trim() : null,
    projectDigest:
      typeof digest === "string" && /^[0-9a-f]{64}$/.test(digest)
        ? digest
        : null,
    createdAt: event.created_at,
  };
}

/** Roster roles whose holders may associate agents with a project. */
export const PROJECT_AGENT_ASSOCIATION_AUTHOR_ROLES: readonly string[] = [
  "owner",
  "collaborator",
];
