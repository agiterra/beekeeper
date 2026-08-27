/**
 * Crews: a team whose seats carry roles, and the family rule that keeps a
 * verifier honest.
 *
 * A `KIND_TEAM` (30176) may carry a `crew` block — an ordered seat list and
 * the one seat a launch addresses first (plan D8). This module owns the
 * client-side reading of that block and the one check that can refuse a
 * launch *before anything is signed*: a verifier must not run on the same
 * model vendor as any builder.
 *
 * **"Family" means the model vendor, never the ACP runtime.** Two Goose seats
 * pointed at different vendors are two families; Claude Code and
 * Goose-on-Anthropic are one. A seat says which vendor it is on; when it does
 * not, the vendor is derived from the model id only where the id names it
 * unambiguously, and is otherwise `unknown`. `unknown` is refused rather than
 * guessed — a verifier the launch *hoped* was a different family is exactly
 * the comfortable guess this rule exists to prevent.
 */

/** Role slug a crew seat plays. Free-form on the wire; these are the packs. */
export const CODING_SESSION_CREW_ROLES = [
  "lead",
  "architect",
  "builder",
  "verifier",
  "runner",
  "poker",
] as const;

/** The role whose model family must differ from every builder's. */
export const CODING_SESSION_VERIFIER_ROLE = "verifier";
/** The role a verifier must not share a vendor with. */
export const CODING_SESSION_BUILDER_ROLE = "builder";

/**
 * A model vendor, lowercased. Open set — a seat may declare a vendor this
 * build has never heard of, and that is still a usable family label.
 * `"unknown"` is not a vendor: it is the absence of one.
 */
export type CodingSessionModelVendor = string;

/** The answer to "what family is this seat?", including the honest no-answer. */
export type CodingSessionSeatVendorResolution =
  | { vendor: CodingSessionModelVendor; source: "declared" | "derived" }
  | { vendor: null; source: "unknown" };

/** One seat as the team published it. */
export type CodingSessionCrewSeat = {
  personaId: string;
  role: string;
  driver?: string;
  model?: string;
  vendor?: string;
};

/** The crew block on a team. Seat order is launch order. */
export type CodingSessionCrew = {
  primary: string;
  seats: CodingSessionCrewSeat[];
};

/**
 * Model-id prefixes that name a vendor without ambiguity, in match order.
 *
 * Deliberately short. A prefix earns a row here only when no other vendor
 * ships an id that starts with it — the moment two do, the honest answer is
 * `unknown` and a declared `vendor` on the seat.
 */
export const CODING_SESSION_MODEL_VENDOR_PREFIXES: ReadonlyArray<{
  pattern: RegExp;
  vendor: CodingSessionModelVendor;
}> = [
  { pattern: /^claude-/, vendor: "anthropic" },
  { pattern: /^gpt-/, vendor: "openai" },
  // OpenAI's reasoning series: `o1`, `o3-mini`, `o4-…`. Anchored on a digit so
  // it cannot swallow every id that merely begins with the letter o.
  { pattern: /^o\d/, vendor: "openai" },
  { pattern: /^grok-/, vendor: "xai" },
  { pattern: /^gemini-/, vendor: "google" },
  { pattern: /^llama-/, vendor: "meta" },
];

/** Derive a vendor from a model id, or null when the id does not name one. */
export function deriveCodingSessionModelVendor(
  model: string | null | undefined,
): CodingSessionModelVendor | null {
  const id = (model ?? "").trim().toLowerCase();
  if (id.length === 0) return null;
  return (
    CODING_SESSION_MODEL_VENDOR_PREFIXES.find((row) => row.pattern.test(id))
      ?.vendor ?? null
  );
}

/**
 * Resolve a seat's model vendor: what it declared, else what its model id
 * unambiguously names, else nothing.
 */
export function resolveCodingSessionSeatVendor(seat: {
  model?: string | null;
  vendor?: string | null;
}): CodingSessionSeatVendorResolution {
  const declared = (seat.vendor ?? "").trim().toLowerCase();
  if (declared.length > 0) return { vendor: declared, source: "declared" };
  const derived = deriveCodingSessionModelVendor(seat.model);
  return derived === null
    ? { vendor: null, source: "unknown" }
    : { vendor: derived, source: "derived" };
}

/** A seat with everything the launch needs to create it. */
export type ResolvedCodingSessionCrewSeat = {
  personaId: string;
  role: string;
  /** 64-hex pubkey of the managed agent taking the seat. */
  actor: string;
  /** Display name for the seat, used in the roster and in failure copy. */
  actorLabel: string;
  model: string | null;
  vendor: string | null;
};

export type CodingSessionCrewFamilyVerdict =
  | { ok: true }
  | { ok: false; reason: string };

/**
 * The launch's hard family check (plan D8, operator ruling 2026-08-26).
 *
 * Refuses when a verifier seat's vendor equals any builder seat's vendor, and
 * refuses when any verifier or builder seat's vendor cannot be established.
 * Hard, not advisory: the caller must not publish anything when this returns
 * `ok: false`.
 *
 * The check needs a pair to have an opinion — a crew with no verifier, or one
 * with no builder, has no family separation to violate, and an undeclared
 * vendor on some *other* role is not this rule's business.
 */
export function checkCodingSessionCrewFamilies(
  seats: ReadonlyArray<{
    role: string;
    model?: string | null;
    vendor?: string | null;
    actorLabel?: string;
  }>,
): CodingSessionCrewFamilyVerdict {
  const verifiers = seats.filter(
    (seat) => seat.role === CODING_SESSION_VERIFIER_ROLE,
  );
  const builders = seats.filter(
    (seat) => seat.role === CODING_SESSION_BUILDER_ROLE,
  );
  if (verifiers.length === 0 || builders.length === 0) return { ok: true };

  const undeclared = [...verifiers, ...builders].filter(
    (seat) => resolveCodingSessionSeatVendor(seat).vendor === null,
  );
  if (undeclared.length > 0) {
    const named = undeclared
      .map(
        (seat) =>
          `${seat.role}${seat.actorLabel ? ` (${seat.actorLabel})` : ""}`,
      )
      .join(", ");
    return {
      ok: false,
      reason:
        `Declare the model vendor for ${named}. A verifier has to run on a ` +
        "different vendor than the builders, and this build cannot tell " +
        "which vendor these seats are on from their model ids.",
    };
  }

  for (const verifier of verifiers) {
    const verifierVendor = resolveCodingSessionSeatVendor(verifier).vendor;
    const clash = builders.find(
      (builder) =>
        resolveCodingSessionSeatVendor(builder).vendor === verifierVendor,
    );
    if (clash) {
      return {
        ok: false,
        reason:
          `The verifier and the builder are both on ${verifierVendor}. A ` +
          "verifier must be a different model vendor than every builder it " +
          "reviews — change one seat's model or vendor and launch again.",
      };
    }
  }
  return { ok: true };
}

/**
 * The roster block carried in the primary seat's first turn.
 *
 * A lead that has to ask who else is in the room has already lost a turn to
 * it, so the roster is stated once, in the launch, in the same order the
 * seats were created.
 */
export function codingSessionCrewRosterText(input: {
  seats: ReadonlyArray<ResolvedCodingSessionCrewSeat>;
  primaryPersonaId: string;
}): string {
  const lines = input.seats.map((seat) => {
    const vendor = resolveCodingSessionSeatVendor(seat).vendor ?? "unknown";
    const model = seat.model ? ` · ${seat.model}` : "";
    const you = seat.personaId === input.primaryPersonaId ? " — you" : "";
    return `- ${seat.role}: ${seat.actorLabel} (${vendor}${model})${you}`;
  });
  return ["[Crew]", ...lines].join("\n");
}

/** The first turn the primary seat receives: the goal, then the roster. */
export function codingSessionCrewFirstTurnText(input: {
  goal: string;
  seats: ReadonlyArray<ResolvedCodingSessionCrewSeat>;
  primaryPersonaId: string;
}): string {
  return `${input.goal.trim()}\n\n${codingSessionCrewRosterText({
    seats: input.seats,
    primaryPersonaId: input.primaryPersonaId,
  })}`;
}

/**
 * Read a crew block off a raw team record.
 *
 * Strict: a malformed crew is no crew, so a team that looks launchable in the
 * picker is a team the launch can actually walk. Seat order is preserved.
 */
export function parseCodingSessionCrew(
  value: unknown,
): CodingSessionCrew | null {
  if (!isRecord(value)) return null;
  const primary = value.primary;
  const seats = value.seats;
  if (typeof primary !== "string" || primary.trim().length === 0) return null;
  if (!Array.isArray(seats) || seats.length === 0) return null;
  const parsed: CodingSessionCrewSeat[] = [];
  for (const raw of seats) {
    if (!isRecord(raw)) return null;
    const personaId = raw.personaId;
    const role = raw.role;
    if (typeof personaId !== "string" || personaId.trim().length === 0) {
      return null;
    }
    if (typeof role !== "string" || role.trim().length === 0) return null;
    parsed.push({
      personaId,
      role,
      ...(typeof raw.driver === "string" ? { driver: raw.driver } : {}),
      ...(typeof raw.model === "string" ? { model: raw.model } : {}),
      ...(typeof raw.vendor === "string" ? { vendor: raw.vendor } : {}),
    });
  }
  if (!parsed.some((seat) => seat.personaId === primary)) return null;
  return { primary, seats: parsed };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
