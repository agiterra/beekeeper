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
