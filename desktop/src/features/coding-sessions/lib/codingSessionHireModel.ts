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
   * True only when the record names an id this runtime does not publish *and*
   * the runtime offers neither a same-family model nor a default.
   */
  mustPick: boolean;
  /**
   * The record's own id when {@link model} is a disclosed stand-in for it, or
   * `null` when `model` is the record's id (or nothing). Never set without a
   * {@link note} that names both ids.
   */
  substitutedFor: string | null;
};

/** Model families whose ids share a word across catalogs and versions. */
const CODING_SESSION_MODEL_FAMILIES = ["opus", "sonnet", "haiku", "fable"];

/** `opus[1m]` → `opus`: the id without any trailing `[...]` option groups. */
function codingSessionModelBase(id: string): string {
  let rest = id.trim();
  while (rest.endsWith("]")) {
    const open = rest.lastIndexOf("[");
    if (open <= 0) break;
    rest = rest.slice(0, open);
  }
  return rest.toLowerCase();
}

/** The family word an id carries (`claude-opus-4` → `opus`), else its base. */
function codingSessionModelFamily(id: string): string {
  const base = codingSessionModelBase(id);
  const tokens = base.split(/[-_.\s]+/);
  return (
    CODING_SESSION_MODEL_FAMILIES.find((family) => tokens.includes(family)) ??
    base
  );
}

/**
 * The offered id nearest to one the catalog does not publish, and why.
 *
 * Nearest means, in order: the same id with different `[...]` options
 * (`opus[1m]` → `opus`, preferring the bare one), then the same model family
 * (`claude-opus-4` → `opus`), then the runtime's own default when the catalog
 * publishes it. `null` when none of the three exists. Catalog order breaks
 * ties. This is a *preselection* for a person to see and change — never a
 * silent rewrite; the hire host does not call it (see the module doc).
 */
export function nearestOfferedCodingSessionModel(input: {
  recorded: string;
  allowedModels: readonly string[];
  defaultModel?: string | null;
}): { model: string; basis: "family" | "default" } | null {
  const base = codingSessionModelBase(input.recorded);
  const family = codingSessionModelFamily(input.recorded);
  const offered = input.allowedModels.filter(
    (id) => id.trim().length > 0 && id !== input.recorded,
  );
  const bare = offered.find((id) => id.toLowerCase() === base);
  if (bare) return { model: bare, basis: "family" };
  const sameBase = offered.find((id) => codingSessionModelBase(id) === base);
  if (sameBase) return { model: sameBase, basis: "family" };
  const sameFamily = offered.find(
    (id) => codingSessionModelFamily(id) === family,
  );
  if (sameFamily) return { model: sameFamily, basis: "family" };
  const fallback = input.defaultModel?.trim() ?? "";
  if (fallback.length > 0 && offered.includes(fallback)) {
    return { model: fallback, basis: "default" };
  }
  return null;
}

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
 * Then the fix overcorrected: a record naming `opus[1m]` on a runtime that
 * offers `opus` preselected nothing and held Start behind a red refusal, and
 * control run 4 (ledger 255(e), 2026-09-23) spent most of its 22 setup
 * minutes there. So the not-offered case now *preselects* the nearest offered
 * id and says so, naming both ids — a visible, changeable stand-in, which is
 * not the silent substitution the 2026-08-29 ruling forbids.
 *
 * - the catalog offers the record's id → it is preselected, nothing to say;
 * - the person has picked a model by hand, or the record names none, or the
 *   catalog was never read → this decides nothing;
 * - the catalog does not offer the record's id → the same-family id, else the
 *   runtime default, is preselected with a one-line notice naming the
 *   record's id and the stand-in;
 * - and only when there is neither → nothing is preselected and `mustPick`
 *   holds the create until a real model is chosen.
 *
 * The record is not edited here; the surface offers that as its own action.
 */
export function resolveCodingSessionSeatIdentityModel(input: {
  /** The seated identity's own model id, or null when it names none. */
  agentModel: string | null;
  /** Model ids the selected runtime actually publishes. */
  allowedModels: readonly string[];
  /** The runtime's own default id, when it reports one. */
  defaultModel?: string | null;
  /** This computer's name for the runtime, used in the disclosure. */
  providerInstanceRef: string;
  /** Whether the person has picked a model by hand. */
  selectionExplicit: boolean;
}): CodingSessionSeatIdentityModel {
  const nothing: CodingSessionSeatIdentityModel = {
    model: null,
    note: null,
    mustPick: false,
    substitutedFor: null,
  };
  const agentModel = input.agentModel?.trim() ?? "";
  if (input.selectionExplicit || agentModel.length === 0) return nothing;
  const resolution = resolveCodingSessionHireModel(
    agentModel,
    input.allowedModels,
  );
  if (resolution === null || resolution.kind === "unknown") return nothing;
  if (resolution.kind === "offered") {
    return { ...nothing, model: resolution.model };
  }
  const nearest = nearestOfferedCodingSessionModel({
    recorded: agentModel,
    allowedModels: input.allowedModels,
    defaultModel: input.defaultModel ?? null,
  });
  if (nearest === null) {
    return {
      ...nothing,
      mustPick: true,
      note:
        `This identity's record names ${agentModel}, which ` +
        `${input.providerInstanceRef} does not offer. Pick a model; the record ` +
        `keeps ${agentModel} until you change it on the Agents screen.`,
    };
  }
  const why =
    nearest.basis === "family"
      ? `${nearest.model}, the nearest model it offers,`
      : `${nearest.model}, its default (it offers nothing of the same family),`;
  return {
    model: nearest.model,
    mustPick: false,
    substitutedFor: agentModel,
    note:
      `This identity's record names ${agentModel}, which ` +
      `${input.providerInstanceRef} does not offer, so ${why} is preselected ` +
      "instead. You can pick another.",
  };
}

/**
 * Whether a create may offer to write its model back onto the seated
 * identity's record, and what that write would say.
 *
 * Only when the record names an id this runtime does not publish, the model
 * the create will run is one it does, and the two differ — so a record that
 * already names a runnable model, an unread catalog, or a create with no model
 * yet never offers a write. `null` means offer nothing.
 */
export function codingSessionSeatRecordModelUpdate(input: {
  /** The id the identity's record names today, or null. */
  recordedModel: string | null;
  /** The id this create will run the identity on, or null. */
  nextModel: string | null;
  /** Model ids the selected runtime actually publishes. */
  allowedModels: readonly string[];
}): { recordedModel: string; nextModel: string } | null {
  const recorded = input.recordedModel?.trim() ?? "";
  const next = input.nextModel?.trim() ?? "";
  if (recorded.length === 0 || next.length === 0 || recorded === next) {
    return null;
  }
  if (input.allowedModels.length === 0) return null;
  if (input.allowedModels.includes(recorded)) return null;
  if (!input.allowedModels.includes(next)) return null;
  return { recordedModel: recorded, nextModel: next };
}
