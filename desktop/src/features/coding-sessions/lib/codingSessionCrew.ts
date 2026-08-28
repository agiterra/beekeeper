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

import {
  CODING_SESSION_ADAPTER_DEFAULT_MODEL,
  codingSessionModelChoices,
  splitCodingSessionModelId,
} from "./codingSessionModelChoice";

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

/**
 * The answer to "what family is this seat?", including the two honest
 * no-answers: nothing said it, or two things said different things.
 */
export type CodingSessionSeatVendorResolution =
  | {
      vendor: CodingSessionModelVendor;
      /**
       * `declared` — the seat said so. `derived` — its model id names the
       * vendor. `runtime` — its ACP driver can only run one vendor, so the
       * model alias never had to name it.
       */
      source: "declared" | "derived" | "runtime";
    }
  | { vendor: null; source: "unknown" }
  | {
      vendor: null;
      source: "conflict";
      declared: CodingSessionModelVendor;
      derived: CodingSessionModelVendor;
    }
  | {
      /** The seat names the adapter's `default` alias, so no model is fixed. */
      vendor: null;
      source: "adapter-default";
      /** What the seat said anyway, for copy that can name the hope. */
      declared: CodingSessionModelVendor | null;
    };

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

/**
 * Runtimes that can only ever run one model vendor, by ACP driver slug and by
 * `providerInstanceRef`.
 *
 * The Claude Code adapter talks to Anthropic and the Codex adapter to OpenAI,
 * whatever model alias a seat carries — so on those runtimes `sonnet`, `opus`,
 * `haiku`, `fable` and even `default` name a vendor even though the *string*
 * does not. Goose and `buzz-agent` take their provider from configuration and
 * are deliberately absent: for them the vendor really is unknown.
 */
export const CODING_SESSION_RUNTIME_VENDORS: ReadonlyArray<{
  ids: readonly string[];
  vendor: CodingSessionModelVendor;
}> = [
  {
    ids: ["claude-agent-acp", "claude-code-acp", "claude-primary"],
    vendor: "anthropic",
  },
  { ids: ["codex-acp", "codex-primary"], vendor: "openai" },
];

/**
 * The one vendor a runtime can run, or null when it can run several.
 *
 * `id` is an ACP driver slug (`claude-agent-acp`) or a `providerInstanceRef`
 * (`claude-primary`) — both name the same runtime, and a caller has one or the
 * other depending on whether it holds a seat or a provider.
 */
export function codingSessionRuntimeVendor(
  id: string | null | undefined,
): CodingSessionModelVendor | null {
  const slug = splitCodingSessionModelId((id ?? "").trim().toLowerCase()).model;
  if (slug.length === 0) return null;
  return (
    CODING_SESSION_RUNTIME_VENDORS.find((row) => row.ids.includes(slug))
      ?.vendor ?? null
  );
}

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
 * Does this seat's model id name the adapter's `default` alias?
 *
 * `default` is what an adapter publishes for "you choose" — a runtime with no
 * live model discovery publishes exactly `allowedModels: ["default"]`. It is
 * not a model, so nothing about it names a vendor, and a bracketed form
 * (`default[1m]`) is the same alias with a decision packed on.
 */
export function isCodingSessionAdapterDefaultModel(
  model: string | null | undefined,
): boolean {
  const id = (model ?? "").trim().toLowerCase();
  if (id.length === 0) return false;
  return (
    splitCodingSessionModelId(id).model === CODING_SESSION_ADAPTER_DEFAULT_MODEL
  );
}

/**
 * Resolve a seat's model vendor: what it declared, else what its model id
 * unambiguously names, else nothing — and *nothing* again when the two
 * disagree.
 *
 * A declaration does not outrank the table. `vendor: "local"` on a seat whose
 * model is `claude-opus-5` is not a seat on some other family; it is a seat
 * whose two statements about itself cannot both be true, and the family rule
 * exists precisely to refuse the comfortable one. The conflict is carried in
 * the result rather than collapsed, so both the refusal and the roster can
 * name the pair.
 */
export function resolveCodingSessionSeatVendor(seat: {
  driver?: string | null;
  model?: string | null;
  vendor?: string | null;
}): CodingSessionSeatVendorResolution {
  const declared = (seat.vendor ?? "").trim().toLowerCase();
  // The runtime outranks both the model id and the declaration, because it is
  // the only one of the three this build can check: a seat created against the
  // Claude adapter runs on Anthropic whatever its seat says or its alias
  // spells. This is also why `default` is an answer here and nowhere else —
  // the *model* is still the adapter's to pick, but the vendor is not.
  const runtime = codingSessionRuntimeVendor(seat.driver);
  if (runtime !== null) {
    const derived = deriveCodingSessionModelVendor(seat.model);
    const contradiction =
      declared.length > 0 && declared !== runtime
        ? declared
        : derived !== null && derived !== runtime
          ? derived
          : null;
    if (contradiction !== null) {
      return {
        vendor: null,
        source: "conflict",
        declared: contradiction,
        derived: runtime,
      };
    }
    return declared.length > 0
      ? { vendor: runtime, source: "declared" }
      : { vendor: runtime, source: "runtime" };
  }
  // The `default` alias outranks a declaration for the same reason the table
  // does: it is a statement about a model the adapter has not picked yet, so
  // the vendor the seat names is a hope, not something this build checked.
  if (isCodingSessionAdapterDefaultModel(seat.model)) {
    return {
      vendor: null,
      source: "adapter-default",
      declared: declared.length > 0 ? declared : null,
    };
  }
  const derived = deriveCodingSessionModelVendor(seat.model);
  if (declared.length > 0) {
    return derived !== null && derived !== declared
      ? { vendor: null, source: "conflict", declared, derived }
      : { vendor: declared, source: "declared" };
  }
  return derived === null
    ? { vendor: null, source: "unknown" }
    : { vendor: derived, source: "derived" };
}

/**
 * How this build says where a crew seat is changed.
 *
 * Every refusal below has to end in an action, and "change the seat's model"
 * is not one this build offers: team mode never reaches the model picker, and
 * `create_team` writes no crew block, so the roster is only editable where it
 * is stored.
 */
export const CODING_SESSION_CREW_EDIT_HINT =
  "This build has no team editor: change the seat in this computer's " +
  "teams.json (or on the device that published the team), then reopen this " +
  "dialog.";

/**
 * The seat's vendor as a person should read it — including the two cases where
 * there is no vendor to print.
 *
 * One function so the roster in the first turn and the roster on screen can
 * never drift into telling two different stories about the same seat.
 * `annotateSource` is the screen's extra: where a vendor was inferred rather
 * than stated, the screen says so, while the roster the lead is handed stays
 * one short line per seat.
 */
export function describeCodingSessionSeatVendor(
  seat: {
    driver?: string | null;
    model?: string | null;
    vendor?: string | null;
  },
  options?: { annotateSource?: boolean },
): string {
  const resolution = resolveCodingSessionSeatVendor(seat);
  switch (resolution.source) {
    case "declared":
      return resolution.vendor;
    case "runtime":
      return options?.annotateSource
        ? `${resolution.vendor} (the only vendor ${(seat.driver ?? "").trim()} runs)`
        : resolution.vendor;
    case "derived":
      return options?.annotateSource
        ? `${resolution.vendor} (from the model id)`
        : resolution.vendor;
    case "conflict":
      return `declared ${resolution.declared}, but ${(seat.model ?? "").trim()} is ${resolution.derived}`;
    case "adapter-default":
      return options?.annotateSource
        ? `unknown — ${CODING_SESSION_ADAPTER_DEFAULT_MODEL} lets the adapter pick the model`
        : "unknown";
    default:
      return options?.annotateSource ? "vendor not declared" : "unknown";
  }
}

/** A seat with everything the launch needs to create it. */
export type ResolvedCodingSessionCrewSeat = {
  personaId: string;
  role: string;
  /** 64-hex pubkey of the managed agent taking the seat. */
  actor: string;
  /** Display name for the seat, used in the roster and in failure copy. */
  actorLabel: string;
  /**
   * ACP driver slug the seat runs on, when the team pins one. A runtime that
   * can only run one vendor settles the seat's family whatever its model alias
   * says — see [`codingSessionRuntimeVendor`].
   */
  driver?: string | null;
  model: string | null;
  vendor: string | null;
  /**
   * Whether this computer holds the role pack behind the agent taking the
   * seat. `false` means the seat runs on its persona prompt alone;
   * `undefined` means the backend never answered, and an unanswered field is
   * not a finding about the seat.
   */
  hasRolePack?: boolean;
};

export type CodingSessionCrewFamilyVerdict =
  | { ok: true }
  | { ok: false; reason: string };

/**
 * The launch's hard family check (plan D8, operator ruling 2026-08-26).
 *
 * Refuses when a verifier seat's vendor equals any builder seat's vendor, and
 * refuses when any verifier or builder seat's vendor cannot be established —
 * including the seat that carries the adapter's `default` alias, whose vendor
 * *nothing* has decided yet no matter what the seat declares.
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
    driver?: string | null;
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

  const named = (seat: { role: string; actorLabel?: string }) =>
    `${seat.role}${seat.actorLabel ? ` (${seat.actorLabel})` : ""}`;

  const conflicted = [...verifiers, ...builders].filter(
    (seat) => resolveCodingSessionSeatVendor(seat).source === "conflict",
  );
  if (conflicted.length > 0) {
    const said = conflicted
      .map(
        (seat) => `${named(seat)} — ${describeCodingSessionSeatVendor(seat)}`,
      )
      .join("; ")
      .replace(/declared /g, "declares ");
    return {
      ok: false,
      reason:
        `A seat's declared vendor contradicts its model id: ${said}. This ` +
        "build will not guess which of the two is true, and the verifier " +
        `rule is decided on the vendor. ${CODING_SESSION_CREW_EDIT_HINT}`,
    };
  }

  const aliased = [...verifiers, ...builders].filter(
    (seat) => resolveCodingSessionSeatVendor(seat).source === "adapter-default",
  );
  if (aliased.length > 0) {
    return {
      ok: false,
      reason:
        `Give ${aliased.map(named).join(", ")} a model id: ` +
        `"${CODING_SESSION_ADAPTER_DEFAULT_MODEL}" is the alias that lets the ` +
        "adapter pick, so every seat carrying it runs whatever that one " +
        "runtime chose — and a vendor declared on such a seat is a claim " +
        "about a model nothing has picked yet. A verifier has to run on a " +
        `different vendor than every builder. ${CODING_SESSION_CREW_EDIT_HINT}`,
    };
  }

  const undeclared = [...verifiers, ...builders].filter(
    (seat) => resolveCodingSessionSeatVendor(seat).vendor === null,
  );
  if (undeclared.length > 0) {
    return {
      ok: false,
      reason:
        `Declare the model vendor for ${undeclared.map(named).join(", ")}. A ` +
        "verifier has to run on a different vendor than the builders, and " +
        "this build cannot tell which vendor these seats are on from their " +
        `model ids. ${CODING_SESSION_CREW_EDIT_HINT}`,
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
          `reviews. ${CODING_SESSION_CREW_EDIT_HINT}`,
      };
    }
  }
  return { ok: true };
}

/**
 * Can the provider that will run this crew actually run each seat's model?
 *
 * **The family rule is only as good as this check.** Every seat's create is
 * published against the one `providerInstanceRef` the dialog selected, and the
 * provider's `apply_model` does not refuse a model its adapter has never heard
 * of — it logs "agent does not offer model … — using its default" and creates
 * the session anyway. So a verifier declared `openai` on a Claude-only runtime
 * passes the vendor check on `gpt-5.6-sol` and then *runs on Anthropic*: the
 * hard refusal never fires, and nothing re-checks once the 44223 metadata
 * reports the effective model.
 *
 * Decided on base model ids, not on the published id byte for byte: an adapter
 * packs a thinking level or a context window into brackets
 * (`claude-opus-5[1m]`), and neither changes the vendor. What must be true is
 * that the runtime offers a model of that name at all.
 *
 * An empty catalog is a refusal, not a pass. "This build could not check"
 * and "this build checked and it was fine" are different answers, and only one
 * of them may let a crew launch.
 */
export function checkCodingSessionCrewSeatModels(
  seats: ReadonlyArray<{
    role: string;
    driver?: string | null;
    model?: string | null;
    vendor?: string | null;
    actorLabel?: string;
  }>,
  provider: {
    allowedModels: readonly string[];
    /**
     * `providerInstanceRef` every seat will be created against. When it names a
     * runtime that can run only one vendor, a seat declaring another vendor is
     * refused: the declaration would otherwise be a claim about a runtime the
     * seat is not going to run on.
     */
    instanceRef?: string | null;
    /** Runtime name, for a refusal a person can act on. */
    label?: string | null;
  },
): CodingSessionCrewFamilyVerdict {
  const decided = [...seats].filter(
    (seat) =>
      seat.role === CODING_SESSION_VERIFIER_ROLE ||
      seat.role === CODING_SESSION_BUILDER_ROLE,
  );

  const runtime = (provider.label ?? "").trim();
  const named = (seat: { role: string; actorLabel?: string }) =>
    `${seat.role}${seat.actorLabel ? ` (${seat.actorLabel})` : ""}`;
  const on = runtime.length > 0 ? `${runtime} ` : "";

  // Every seat, not only the two the vendor rule decides: a lead whose roster
  // line reads `anthropic` while it is created against a Codex provider is a
  // screen stating a vendor nothing will run, which is the same lie whatever
  // the seat's role.
  const providerVendor = codingSessionRuntimeVendor(provider.instanceRef);
  if (providerVendor !== null) {
    const elsewhere = seats.filter((seat) => {
      const vendor = resolveCodingSessionSeatVendor(seat).vendor;
      return vendor !== null && vendor !== providerVendor;
    });
    if (elsewhere.length > 0) {
      const said = elsewhere
        .map(
          (seat) =>
            `${named(seat)} — ${resolveCodingSessionSeatVendor(seat).vendor}`,
        )
        .join("; ");
      return {
        ok: false,
        reason:
          `Every seat is created against the ${on}provider selected for this ` +
          `team, which runs ${providerVendor} and nothing else — but ${said}. ` +
          "A seat that runs on a vendor other than the one it declares makes " +
          `the verifier rule a check of something nothing ran. ${CODING_SESSION_CREW_EDIT_HINT}`,
      };
    }
  }

  // The catalog half is only the vendor rule's business: a lead's model that
  // this runtime replaces with its default costs nobody a check they thought
  // they had.
  if (decided.length === 0) return { ok: true };

  if (provider.allowedModels.length === 0) {
    return {
      ok: false,
      reason:
        `This build cannot see which models the ${on}provider offers, so it ` +
        "cannot tell whether each seat would run on the vendor its seat " +
        "declares. The verifier rule is decided on the vendor, and a check " +
        "this build could not make is not a check it passed. " +
        CODING_SESSION_CREW_EDIT_HINT,
    };
  }

  const offered = new Set(
    codingSessionModelChoices([...provider.allowedModels]).models,
  );
  const unrunnable = decided.filter((seat) => {
    const id = (seat.model ?? "").trim();
    if (id.length === 0) return true;
    return !offered.has(splitCodingSessionModelId(id).model);
  });
  if (unrunnable.length === 0) return { ok: true };

  const said = unrunnable
    .map((seat) => {
      const id = (seat.model ?? "").trim();
      return `${named(seat)} — ${id.length > 0 ? id : "no model"}`;
    })
    .join("; ");
  return {
    ok: false,
    reason:
      `The ${on}provider selected for this team does not offer ${said}. ` +
      "Every seat runs on that one provider, and a model it does not have " +
      "is silently replaced by its default — so the seat would run on a " +
      "vendor other than the one its seat declares, and the verifier rule " +
      `would have checked a model nothing ran. ${CODING_SESSION_CREW_EDIT_HINT}`,
  };
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
    const resolution = resolveCodingSessionSeatVendor(seat);
    // A conflict already names the model inside its own phrase; repeating it
    // would read as two seats' worth of model.
    const vendor = describeCodingSessionSeatVendor(seat);
    const model =
      seat.model && resolution.source !== "conflict" ? ` · ${seat.model}` : "";
    const you = seat.personaId === input.primaryPersonaId ? " — you" : "";
    return `- ${seat.role}: ${seat.actorLabel} (${vendor}${model})${you}`;
  });
  return ["[Team]", ...lines].join("\n");
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
