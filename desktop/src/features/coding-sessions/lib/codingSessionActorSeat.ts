/**
 * Agent seats: the `actor` + `role` pair a coding-session create can carry.
 *
 * An execution is *seated* when its 44221 create names a managed agent's
 * public key as its `actor` and a role slug describing what that seat is for.
 * The pair travels together or not at all: an actor with no role is an
 * unlabelled seat nobody can address by role, and a role with no actor labels
 * a seat nobody holds. Both are refused here, before anything is signed.
 *
 * The secret half of the identity is NOT in this module and never reaches the
 * relay — it is staged host-locally by
 * {@link import("@/shared/api/tauri").stageCodingSessionActorSeat}.
 */
import type { CodingSessionSeatAgent } from "./codingSessionSeatAgent";

/** Role slugs offered in the create dialog. Free text is still accepted. */
export const CODING_SESSION_ROLE_SUGGESTIONS = [
  "lead",
  "architect",
  "builder",
  "verifier",
  "runner",
  "poker",
] as const;

/** Longest role slug the wire contract accepts, in UTF-8 bytes. */
export const MAX_CODING_SESSION_ROLE_BYTES = 64;

const ROLE_SLUG_REGEX = /^[a-z0-9-]+$/;
const HEX64_REGEX = /^[0-9a-f]{64}$/;

/** The seat carried by a create, once validated. */
export type CodingSessionActorSeat = {
  /** Lowercase 64-hex public key of the managed agent taking the seat. */
  actor: string;
  /** Role slug, `[a-z0-9-]+`, 1..64 bytes. */
  role: string;
};

/** Is this exactly the wire's role-slug shape? */
export function isCodingSessionRoleSlug(value: unknown): value is string {
  return (
    typeof value === "string" &&
    ROLE_SLUG_REGEX.test(value) &&
    new TextEncoder().encode(value).byteLength <= MAX_CODING_SESSION_ROLE_BYTES
  );
}

/**
 * Fold typed role text into a slug, or null when it cannot be one.
 *
 * Deliberately forgiving about the two things people type without meaning
 * anything by them — capitals and spaces — and unforgiving about everything
 * else, so a role that looks accepted in the field is the role that is
 * signed.
 */
export function normalizeCodingSessionRoleSlug(input: string): string | null {
  const slug = input
    .trim()
    .toLowerCase()
    .replaceAll(/[\s_]+/g, "-")
    .replaceAll(/-{2,}/g, "-")
    .replace(/^-+/, "")
    .replace(/-+$/, "");
  return isCodingSessionRoleSlug(slug) ? slug : null;
}

/**
 * Validate a seat draft into the exact pair a create may carry.
 *
 * Returns `{ seat: null }` when neither field was filled in — an ordinary
 * unseated create — and a named `error` when exactly one was, or when the
 * values are malformed.
 */
export function resolveCodingSessionActorSeat(input: {
  actor: string | null;
  role: string | null;
}):
  | { seat: CodingSessionActorSeat | null; error: null }
  | { seat: null; error: string } {
  const actor = input.actor?.trim().toLowerCase() ?? "";
  const roleText = input.role?.trim() ?? "";
  if (actor.length === 0 && roleText.length === 0) {
    return { seat: null, error: null };
  }
  if (actor.length === 0) {
    return {
      seat: null,
      error: "Pick the agent that takes this seat, or clear the role.",
    };
  }
  if (!HEX64_REGEX.test(actor)) {
    return {
      seat: null,
      error: "That agent's public key is not a 64-character hex key.",
    };
  }
  if (roleText.length === 0) {
    return {
      seat: null,
      error: "Give this seat a role, or clear the agent.",
    };
  }
  const role = normalizeCodingSessionRoleSlug(roleText);
  if (role === null) {
    return {
      seat: null,
      error:
        "A role is lowercase letters, numbers and hyphens — up to 64 characters.",
    };
  }
  return { seat: { actor, role }, error: null };
}

/**
 * The role a seat should show, given the agent chosen and whether the person
 * has touched the role box.
 *
 * An agent's *home* role is what the pack behind it knows, so it is the only
 * honest default: seating a builder as a builder is the case that needs no
 * explanation. A typed role always wins — this fills a box, it does not
 * govern one — and an agent with no home role fills nothing at all rather
 * than guessing a role from a name.
 */
export function defaultCodingSessionSeatRole(input: {
  agent: CodingSessionSeatAgent | null;
  /** Has the person typed in the role box since the last agent change? */
  roleTouched: boolean;
  /** The role box's current text. */
  role: string;
}): string {
  // No seat, no role: clearing the agent clears both halves together, because
  // half a seat is refused by `resolveCodingSessionActorSeat` anyway.
  if (input.agent === null) return "";
  if (input.roleTouched) return input.role;
  return input.agent.homeRole ?? "";
}

/** A line under the role box, or null when there is nothing truthful to say. */
export type CodingSessionSeatRoleNotice = {
  /** `warn` exactly when the seat's role is not the pack it will carry. */
  tone: "muted" | "warn";
  message: string;
};

/**
 * Disclose the distance between the role this seat is given and the role the
 * agent *is*.
 *
 * The wire carries the seat's role, but custody staging carries the agent's
 * **home** role pack — so a builder seated as a lead is briefed as a builder.
 * That gap is invisible unless it is said here, before anything is signed.
 *
 * An agent whose home role is unknown (`undefined`) or absent (`null`)
 * produces no notice at all. Absence is not a claim: a build that never asked
 * must not present "no home role" as a finding.
 */
export function codingSessionSeatRoleNotice(input: {
  agent: CodingSessionSeatAgent | null;
  role: string;
}): CodingSessionSeatRoleNotice | null {
  const homeRole = input.agent?.homeRole?.trim();
  if (!input.agent || !homeRole) return null;
  const typed = input.role.trim();
  if (typed.length === 0) return null;
  const role = normalizeCodingSessionRoleSlug(typed) ?? typed;
  if (role === (normalizeCodingSessionRoleSlug(homeRole) ?? homeRole)) {
    return { tone: "muted", message: "Its home role." };
  }
  return {
    tone: "warn",
    message:
      `${input.agent.name} is a ${homeRole} — seating it as ${role}; ` +
      `it will carry the ${homeRole} pack.`,
  };
}

/**
 * The disclosure a seat owes before submit when this computer holds no role
 * pack behind the agent.
 *
 * `undefined` — a backend that cannot answer — renders nothing, so a build
 * without the pack installer never accuses an agent of missing a pack it was
 * never asked about.
 */
export function codingSessionSeatPackNotice(
  agent: CodingSessionSeatAgent | null,
): string | null {
  if (!agent || agent.hasRolePack !== false) return null;
  return (
    `${agent.name} has no role pack on this computer, so this seat carries ` +
    `no role skills and runs on its persona prompt alone.`
  );
}
