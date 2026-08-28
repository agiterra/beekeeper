/**
 * The model a hired seat actually runs, against the catalog that decides it.
 *
 * A model id is not a name a lead invents — it is an id the runtime's own
 * catalog publishes (kind:44222 `allowedModels`, and the same list this
 * computer's provider answers `coding_session_provider_models` with). A hire
 * naming something else has three possible honest answers, and this module is
 * all three:
 *
 * 1. **Offered** — the catalog has it. Used byte for byte; nothing is
 *    normalized, because `opus[1m]` and `opus` are different ids.
 * 2. **Translated** — the lead wrote a *vendor* model name for the Claude
 *    family (`claude-sonnet-5`, `claude-opus-4-1`) where the catalog offers
 *    the family alias (`sonnet`, `opus[1m]`). That is the one guess worth
 *    making, because those two vocabularies name the same thing and every
 *    lead has read the vendor one. It is a guess, so it is **disclosed** —
 *    see {@link codingSessionHireModelNotice} — never applied silently.
 * 3. **Not offered** — anything else. Refused `HIRE_MODEL_NOT_OFFERED` with
 *    the offered ids in the reason, so the lead's next request can be right
 *    rather than another guess.
 *
 * And one non-answer: an **unknown** catalog. When this host has not read the
 * runtime's model list — the probe failed, or the runtime publishes none — it
 * has no basis to refuse anything, so the request passes through untouched. A
 * refusal built on a list this computer never loaded would be a claim about
 * the model dressed up as a claim about the catalog.
 *
 * Live evidence for why this exists: on 2026-08-28 a lead hired a builder on
 * `claude-sonnet-5`, an id `claude-primary` has never offered — its catalog is
 * `default, claude-fable-5[1m], haiku, opus[1m], sonnet`.
 */

/** The Claude family aliases a catalog may publish, longest-lived first. */
export const CODING_SESSION_HIRE_CLAUDE_FAMILIES = [
  "sonnet",
  "opus",
  "haiku",
] as const;

/** What a host decided about one requested model. */
export type CodingSessionHireModelResolution =
  /** The catalog offers exactly this id. */
  | { kind: "offered"; model: string }
  /** A vendor alias mapped onto the catalog's own id. Disclosed. */
  | { kind: "translated"; model: string; requested: string }
  /** The catalog offers neither the id nor a family it could translate. */
  | { kind: "not-offered"; requested: string; offered: readonly string[] }
  /** No catalog was read, so nothing can be said about this id. */
  | { kind: "unknown"; model: string };

/**
 * Resolve the model a hire asked for against one runtime's catalog.
 *
 * `null` in, `null` out: a hire naming no model is asking for the identity's
 * own, and this has no opinion about that.
 */
export function resolveCodingSessionHireModel(
  requested: string | null,
  offered: readonly string[],
): CodingSessionHireModelResolution | null {
  if (requested === null) return null;
  const asked = requested.trim();
  if (asked.length === 0) return null;
  // Nothing was read, so nothing is refused. See the module doc.
  if (offered.length === 0) return { kind: "unknown", model: asked };
  if (offered.includes(asked)) return { kind: "offered", model: asked };
  const family = claudeFamilyOf(asked);
  if (family !== null) {
    const translated = offered.find(
      (candidate) => familyAliasOf(candidate) === family,
    );
    if (translated !== undefined) {
      return { kind: "translated", model: translated, requested: asked };
    }
  }
  return { kind: "not-offered", requested: asked, offered: [...offered] };
}

/** The model this resolution puts on the seat's create, if any. */
export function codingSessionHireModelOf(
  resolution: CodingSessionHireModelResolution | null,
): string | null {
  if (resolution === null || resolution.kind === "not-offered") return null;
  return resolution.model;
}

/**
 * The sentence a refused model earns, carrying the offered ids.
 *
 * The list is the whole point: a lead told only "no" retries with another
 * invented id, and a lead told what is offered names one of them.
 */
export function describeCodingSessionHireModelRefusal(
  providerInstanceRef: string,
  resolution: Extract<
    CodingSessionHireModelResolution,
    { kind: "not-offered" }
  >,
): string {
  return (
    `this computer's ${providerInstanceRef} runtime does not offer the model ` +
    `${resolution.requested}. It offers ${resolution.offered.join(", ")}.`
  );
}

/**
 * What the host says out loud when it seated a model nobody asked for.
 *
 * A translation is the host substituting its own judgement for the lead's
 * words, so it is stated where both the lead and the person can read it —
 * never left implicit in a create nobody compares against the request. `null`
 * when there is nothing to disclose.
 */
export function codingSessionHireModelNotice(
  providerInstanceRef: string,
  resolution: CodingSessionHireModelResolution | null,
): string | null {
  if (resolution === null || resolution.kind !== "translated") return null;
  return (
    `The hire asked for ${resolution.requested}; this computer's ` +
    `${providerInstanceRef} runtime does not offer that id, so the seat runs ` +
    `${resolution.model} instead.`
  );
}

/** `claude-sonnet-5` → `sonnet`; anything outside the family → null. */
function claudeFamilyOf(model: string): string | null {
  const match = /^claude-(sonnet|opus|haiku)(?:-.*)?$/i.exec(model.trim());
  return match?.[1]?.toLowerCase() ?? null;
}

/** `opus[1m]` → `opus`; `sonnet` → `sonnet`; `default` → `default`. */
function familyAliasOf(offered: string): string {
  const trimmed = offered.trim().toLowerCase();
  const bracket = trimmed.indexOf("[");
  return bracket === -1 ? trimmed : trimmed.slice(0, bracket);
}
