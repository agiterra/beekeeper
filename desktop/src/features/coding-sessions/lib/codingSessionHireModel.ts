/**
 * The model a hired seat actually runs, against the catalog that decides it.
 *
 * A model id is not a name a lead invents — it is an id the runtime's own
 * catalog publishes (kind:44222 `allowedModels`, and the same list this
 * computer's provider answers `coding_session_provider_models` with, and the
 * same list the create picker renders). That catalog is the **only** model
 * list. A hire naming something else has exactly two honest answers:
 *
 * 1. **Offered** — the catalog has it, byte for byte. Nothing is normalized,
 *    lower-cased, suffix-stripped or family-matched, because `opus[1m]` and
 *    `opus` are different ids and `claude-sonnet-5` is not an id this catalog
 *    publishes at all.
 * 2. **Not offered** — everything else. Refused `HIRE_MODEL_NOT_OFFERED` with
 *    the offered ids in the reason, so the lead's next request can be right
 *    rather than another guess.
 *
 * And one non-answer: an **unknown** catalog. When this host has not read the
 * runtime's model list — the probe failed, or the runtime publishes none — it
 * has no basis to refuse anything, so the request passes through untouched. A
 * refusal built on a list this computer never loaded would be a claim about
 * the model dressed up as a claim about the catalog.
 *
 * **There used to be a third answer, and it was a lie.** Item 82 added a
 * Claude-family alias table (`claude-sonnet-5` → `sonnet`) and item 93 lane A
 * widened it to strip context-window suffixes (`opus` → `opus[1m]`), so a
 * request naming a model the runtime does not offer was silently answered with
 * a model it does. Brian's ruling, 2026-08-29: *"The model picker should not be
 * faked."* A seat that runs weights nobody named is the same class of defect as
 * a badge pointing at a message you cannot find, and the disclosure attached to
 * the substitution did not redeem it — it made an unoffered model look offered.
 * The table is gone; there is no code path in this module that returns an id
 * other than the one it was handed.
 *
 * Live evidence for why the check exists at all: on 2026-08-28 a lead hired a
 * builder on `claude-sonnet-5`, an id `claude-primary` has never offered — its
 * catalog is `default, claude-fable-5[1m], haiku, opus[1m], sonnet`.
 */

import {
  describeCodingSessionHireModelSource,
  type CodingSessionHireModelSource,
} from "./codingSessionHireAgentRuntime";

/** What a host decided about one requested model. */
export type CodingSessionHireModelResolution =
  /** The catalog offers exactly this id. */
  | { kind: "offered"; model: string }
  /** The catalog does not publish this id. Nothing is substituted for it. */
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
  // Exact, and only exact. The catalog is the model list.
  if (offered.includes(asked)) return { kind: "offered", model: asked };
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
 * The sentence an *identity's own* refused model earns.
 *
 * Split from the request's sentence because the two are different facts and
 * different remedies. A refused `--model` is the lead's word to take back; a
 * refused identity model is this computer's own record naming something its
 * runtime does not run, so the sentence names the identity — otherwise the
 * operator is told a model was refused and has nowhere to go and fix it.
 *
 * Live evidence: on 2026-08-28 a hire naming no model fell back to the
 * identity's `opus[1m]` and skipped this check entirely, so the host seated a
 * model its own refusal had said one line earlier was not offered.
 */
export function describeCodingSessionHireIdentityModelRefusal(
  providerInstanceRef: string,
  identityName: string,
  resolution: Extract<
    CodingSessionHireModelResolution,
    { kind: "not-offered" }
  >,
  /**
   * Which tier the id came from — `ManagedAgent.modelSource`. Omitted means
   * the backend reported none, and the sentence then says what was resolved
   * without naming a file.
   */
  modelSource?: CodingSessionHireModelSource | null,
): string {
  return (
    `${identityName}: ${describeCodingSessionHireModelSource(modelSource)} ` +
    `${resolution.requested}, which this computer's ${providerInstanceRef} ` +
    `runtime does not offer. It offers ${resolution.offered.join(", ")}. ` +
    `Name one of them with --model, or fix ${identityName}'s model on the ` +
    "Agents screen."
  );
}

/** What a create surface should do about a seated identity's own model. */
export type CodingSessionSeatIdentityModel = {
  /** The id to preselect, or `null` when this decides nothing. */
  model: string | null;
  /** What the surface owes the person about that, or `null`. */
  note: string | null;
  /**
   * The create has no model it can honestly write and must wait for a pick.
   * True only when the record names an id this runtime does not publish.
   */
  mustPick: boolean;
};

/**
 * The model a seated identity asks a *create* to run on — the create path's
 * one call into the catalog check.
 *
 * The hire host has read an identity's model against the catalog through
 * {@link resolveCodingSessionHireModel} since item 88(a); the create dialog
 * matched the same record with `allowedModels.includes(...)` and, when that
 * missed, **fell silently to the runtime default** (item 90 lane C). Both the
 * miss and the fallback were wrong in the same direction: the session ran on
 * weights the record did not name and no screen said so.
 *
 * Three answers, and none of them substitutes one id for another:
 *
 * - the catalog offers the record's id → it is preselected, nothing to say;
 * - the person has picked a model by hand, or the record names none, or the
 *   catalog was never read → this decides nothing;
 * - the catalog does not offer the record's id → **nothing is preselected**,
 *   the record's id is named out loud, and `mustPick` holds the create until
 *   a real model is chosen. The record is not edited here — a create dialog
 *   is the wrong place to rewrite an identity — so the copy says where it is
 *   edited instead.
 */
export function resolveCodingSessionSeatIdentityModel(input: {
  /** The seated identity's own model id, or null when it names none. */
  agentModel: string | null;
  /** Model ids the selected runtime actually publishes. */
  allowedModels: readonly string[];
  /** This computer's name for the runtime, used in the disclosure. */
  providerInstanceRef: string;
  /** Whether the person has picked a model by hand. */
  selectionExplicit: boolean;
}): CodingSessionSeatIdentityModel {
  const agentModel = input.agentModel?.trim() ?? "";
  if (input.selectionExplicit || agentModel.length === 0) {
    return { model: null, note: null, mustPick: false };
  }
  const resolution = resolveCodingSessionHireModel(
    agentModel,
    input.allowedModels,
  );
  if (resolution === null || resolution.kind === "unknown") {
    return { model: null, note: null, mustPick: false };
  }
  if (resolution.kind === "offered") {
    return { model: resolution.model, note: null, mustPick: false };
  }
  return {
    model: null,
    mustPick: true,
    note:
      `This identity's record names ${agentModel}, which ` +
      `${input.providerInstanceRef} does not offer. Pick a model; the record ` +
      `keeps ${agentModel} until you change it on the Agents screen.`,
  };
}
