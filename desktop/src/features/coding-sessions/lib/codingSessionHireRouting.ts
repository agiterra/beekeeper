/**
 * What a lead may say about routing, and what this host does with it.
 *
 * The split is the whole design, and it is Brian's sentence:
 *
 * > "The lead chooses the capability required. The router chooses the
 * > execution target."
 *
 * So a `session.hire` carries a **request** — a class, a risk, optionally
 * extra trait minimums, the review flags the lead raised, its by-rule
 * challenger sample, a human's override, and optionally `proposed`: the
 * decision the *requester's* own router computed, which is informational. The
 * founder's host reads its own live catalog and its own registry, routes, and
 * publishes the **record** on the seat's create, where the class, the tier,
 * the risk, the chosen target, the runner-up, the reason, the review triggers
 * and any disagreement with `proposed` are all readable from the wire.
 *
 * **The request is not the record, and reading one as the other is what broke
 * this live.** On 2026-08-30 the CLI emitted the routing *record* on a hire
 * and this parser accepted only a *request*, so every routed hire classified
 * `malformed` and was dropped without a 44220, a console line or a pixel
 * (ledger draft 97). The two shapes are pinned to one pair of fixtures —
 * `testdata/routing/hire-request-fixture.json` and
 * `testdata/routing/create-record-fixture.json` — which buzz-core's validator,
 * the CLI's emitter and this parser are all tested against.
 *
 * Three refusals live here and all three are honest ones:
 *
 * 1. **`registry not readable on this host`** (`HIRE_NO_ROUTE`). It refuses
 *    rather than routing on a copy compiled into the app. A hardcoded registry
 *    would be a routing decision nobody on this machine could check against
 *    the file the team edits, which is the same class of defect as a picker
 *    offering a model nobody serves.
 * 2. **A malformed request** (`HIRE_MALFORMED`), which always names the key
 *    that failed. A shape refusal that does not say *which* key is a shape
 *    refusal nobody can fix.
 * 3. **A model named with no override** (`HIRE_MALFORMED`). A human may name a
 *    target by hand — that is the only way `xhigh`, `max` or `ultra` is ever
 *    bought — but the router's own answer is being set aside, so the record
 *    has to say why, and the named id has to be one the catalog publishes byte
 *    for byte.
 */
import {
  HIRE_NO_ROUTE,
  routeCodingSession,
  ROUTING_REVIEW_FLAG_TRIGGERS,
  ROUTING_TRAITS,
  parseModelRegistry,
  type CodingSessionRoutingRecord,
  type RoutedExecutionTarget,
  type RoutingCatalogEntry,
  type RoutingEffort,
  type RoutingReviewFlagTrigger,
  type RoutingRisk,
  type RoutingTrait,
} from "./codingSessionRouting";

/** The refusal code a hire whose shape this host cannot read earns. */
export const HIRE_MALFORMED = "HIRE_MALFORMED" as const;

/**
 * The requester's own routing decision, carried for disclosure only.
 *
 * `bee sessions route` runs on the machine that writes the hire, against
 * whatever registry and catalog *that* machine can see. It is never the
 * answer: the seat runs here, on this host's runtime, so this host routes. It
 * is recorded because a host that silently replaced a requester's proposal
 * would be routing behind the requester's back — see `proposedDisagreement`
 * on the record.
 */
export type CodingSessionRoutingProposal = {
  chosen: RoutedExecutionTarget;
  runnerUp?: RoutedExecutionTarget | null;
  /** One sentence: why the requester's own router chose it. */
  reason: string;
  registryVersion: number;
  /** The 44222 revision the requester routed against, or null. */
  catalogRevision: number | null;
};

/**
 * The `routing` object a hire carries.
 *
 * Deliberately *not* a subset of the record — a different object with a
 * different job. `tier` and `risk.score` are absent on purpose: both are
 * derived, and a request that carried them could disagree with the host that
 * derives them without anybody noticing. Everything the router answers —
 * `chosen`, `runnerUp`, `reason`, `reviewRequired`, `reviewReasons`,
 * `registryVersion`, `catalogRevision` — belongs to the record and is refused
 * here by name.
 */
export type CodingSessionHireRoutingRequest = {
  class: string;
  /** Impact × uncertainty × irreversibility, each 1–5. No score. */
  risk: RoutingRisk;
  /** Extra trait minimums, on top of the class gates. */
  profile?: Partial<Record<RoutingTrait, number>>;
  /** A human naming the target by hand. Both keys are required together. */
  override?: {
    /** A catalog id, exactly as published. */
    model: string;
    effort?: RoutingEffort;
    because: string;
  };
  /** The lead's by-rule challenger sample (spec §8). */
  challengerSample?: boolean;
  /** The §6 flag triggers the lead raised. The numeric two are computed. */
  reviewFlags?: readonly RoutingReviewFlagTrigger[];
  /** What the requester's own router decided. Informational. */
  proposed?: CodingSessionRoutingProposal;
};

/** Exactly the keys a request may carry. Anything else is named and refused. */
export const CODING_SESSION_HIRE_ROUTING_REQUEST_KEYS = [
  "class",
  "risk",
  "profile",
  "override",
  "challengerSample",
  "reviewFlags",
  "proposed",
] as const;

/**
 * Keys that belong to the *record* and are refused on a request, each with the
 * sentence that says where they really live.
 *
 * This table is the fix for ledger draft 97: a hire carrying a record used to
 * be refused as a nameless `malformed`, which is indistinguishable from a
 * truncated payload or a hostile one. Now it is refused by name, and the name
 * points at the emitter that has to change.
 */
const RECORD_ONLY_KEYS: Readonly<Record<string, string>> = Object.freeze({
  tier: "the host derives the tier from risk (1–8 fast, 9–39 standard, 40–125 deep); a request may not assert one",
  chosen:
    "the host chooses the execution target; put a requester's own decision under `proposed`",
  runnerUp:
    "the host records the runner-up; put a requester's own decision under `proposed`",
  reason:
    "the host writes the reason; put a requester's own decision under `proposed`",
  reviewRequired:
    "the host computes reviewRequired from the risk and `reviewFlags`",
  reviewReasons:
    "the host computes reviewReasons; a request raises `reviewFlags`",
  registryVersion:
    "the host records the registry it routed against; put a requester's own under `proposed`",
  catalogRevision:
    "the host records the catalog it routed against; put a requester's own under `proposed`",
  proposedDisagreement:
    "the host writes this on the create when it disagrees with `proposed`",
});

/** A request that was read, or the exact key that stopped it being read. */
export type CodingSessionHireRoutingRequestRead =
  | { ok: true; request: CodingSessionHireRoutingRequest }
  | { ok: false; key: string; why: string };

/**
 * Read a hire's `routing`, or say which key failed and why.
 *
 * Total: it never throws and never returns a partially-read request. The
 * failing key is dotted from `routing` (`routing.risk.impact`,
 * `routing.reviewFlags[0]`) so the sentence published back to the lead names
 * something the lead can find in what it sent.
 */
export function readCodingSessionHireRoutingRequest(
  value: unknown,
): CodingSessionHireRoutingRequestRead {
  if (!isRecord(value)) {
    return { ok: false, key: "routing", why: "not a JSON object" };
  }
  for (const key of Object.keys(value)) {
    const displaced = RECORD_ONLY_KEYS[key];
    if (displaced !== undefined) {
      return { ok: false, key: `routing.${key}`, why: displaced };
    }
    if (
      !(CODING_SESSION_HIRE_ROUTING_REQUEST_KEYS as readonly string[]).includes(
        key,
      )
    ) {
      return {
        ok: false,
        key: `routing.${key}`,
        why: `unknown key; a hire's routing carries ${CODING_SESSION_HIRE_ROUTING_REQUEST_KEYS.join(", ")}`,
      };
    }
  }
  if (
    typeof value.class !== "string" ||
    !/^[a-z0-9_-]{1,64}$/.test(value.class)
  ) {
    return {
      ok: false,
      key: "routing.class",
      why: "must be a lowercase class slug of 1–64 characters, e.g. builder",
    };
  }
  const risk = readRisk(value.risk);
  if (!risk.ok) return risk;
  const request: CodingSessionHireRoutingRequest = {
    class: value.class,
    risk: risk.risk,
  };
  if (Object.hasOwn(value, "profile")) {
    const profile = readProfile(value.profile);
    if (!profile.ok) return profile;
    request.profile = profile.profile;
  }
  if (Object.hasOwn(value, "override")) {
    const override = readRequestOverride(value.override);
    if (!override.ok) return override;
    request.override = override.override;
  }
  if (Object.hasOwn(value, "challengerSample")) {
    if (typeof value.challengerSample !== "boolean") {
      return {
        ok: false,
        key: "routing.challengerSample",
        why: "must be a boolean",
      };
    }
    request.challengerSample = value.challengerSample;
  }
  if (Object.hasOwn(value, "reviewFlags")) {
    const flags = readReviewFlags(value.reviewFlags);
    if (!flags.ok) return flags;
    request.reviewFlags = flags.flags;
  }
  if (Object.hasOwn(value, "proposed")) {
    const proposed = readProposal(value.proposed);
    if (!proposed.ok) return proposed;
    request.proposed = proposed.proposal;
  }
  return { ok: true, request };
}

/** Closed-shape check for a hire's `routing`. The reason is in the reader. */
export function isCodingSessionHireRoutingRequest(
  value: unknown,
): value is CodingSessionHireRoutingRequest {
  return readCodingSessionHireRoutingRequest(value).ok;
}

type Refusal = { ok: false; key: string; why: string };

function readRisk(value: unknown): { ok: true; risk: RoutingRisk } | Refusal {
  if (!isRecord(value)) {
    return {
      ok: false,
      key: "routing.risk",
      why: "must be { impact, uncertainty, irreversibility }, each an integer 1–5",
    };
  }
  const keys = ["impact", "uncertainty", "irreversibility"] as const;
  for (const key of Object.keys(value)) {
    if (!(keys as readonly string[]).includes(key)) {
      // Named to the key, not to `routing.risk`: the shared fixture's
      // rejected cases carry the offending key by name
      // (testdata/routing/hire-request-fixture.json), and "routing.risk" would
      // send a reader looking at three fields that are all fine.
      return {
        ok: false,
        key: `routing.risk.${key}`,
        why:
          key === "score"
            ? "carries no score: the host computes impact × uncertainty × irreversibility, so a request and the host can never disagree about it"
            : `unknown key ${key}; risk is { impact, uncertainty, irreversibility }`,
      };
    }
  }
  for (const key of keys) {
    const factor = value[key];
    if (
      !Number.isSafeInteger(factor) ||
      (factor as number) < 1 ||
      (factor as number) > 5
    ) {
      return {
        ok: false,
        key: `routing.risk.${key}`,
        why: "must be an integer 1–5",
      };
    }
  }
  return {
    ok: true,
    risk: {
      impact: value.impact as number,
      uncertainty: value.uncertainty as number,
      irreversibility: value.irreversibility as number,
    },
  };
}

function readProfile(
  value: unknown,
): { ok: true; profile: Partial<Record<RoutingTrait, number>> } | Refusal {
  if (!isRecord(value)) {
    return {
      ok: false,
      key: "routing.profile",
      why: "must be an object of trait minimums",
    };
  }
  const traits = new Set<string>(ROUTING_TRAITS);
  for (const [trait, minimum] of Object.entries(value)) {
    if (!traits.has(trait)) {
      return {
        ok: false,
        key: `routing.profile.${trait}`,
        why: `not a registry trait; the ten are ${ROUTING_TRAITS.join(", ")}`,
      };
    }
    if (
      typeof minimum !== "number" ||
      !Number.isFinite(minimum) ||
      minimum < 1 ||
      minimum > 5
    ) {
      return {
        ok: false,
        key: `routing.profile.${trait}`,
        why: "must be a number 1–5",
      };
    }
  }
  return {
    ok: true,
    profile: { ...value } as Partial<Record<RoutingTrait, number>>,
  };
}

function readRequestOverride(value: unknown):
  | {
      ok: true;
      override: { model: string; effort?: RoutingEffort; because: string };
    }
  | Refusal {
  if (!isRecord(value)) {
    return {
      ok: false,
      key: "routing.override",
      why: "must be { model, because } with an optional effort",
    };
  }
  for (const key of Object.keys(value)) {
    if (!["model", "effort", "because"].includes(key)) {
      return {
        ok: false,
        key: `routing.override.${key}`,
        why: "unknown key; an override is { model, effort?, because }",
      };
    }
  }
  if (typeof value.model !== "string" || value.model.trim().length === 0) {
    return {
      ok: false,
      key: "routing.override.model",
      why: "an override names the catalog id it is overriding to, exactly as published",
    };
  }
  if (typeof value.because !== "string" || value.because.trim().length === 0) {
    return {
      ok: false,
      key: "routing.override.because",
      why: "an override sets the router's own answer aside, so the record has to say why",
    };
  }
  // `effort` is optional *and* nullable. buzz-core's `RoutingOverride` writes
  // the key unconditionally with `null` for "take the tier's effort"
  // (crates/buzz-core/src/coding_session_routing.rs:1005), so a parser that
  // refused an explicit null would refuse every override the CLI emits — the
  // 2026-08-30 drop again, one key further in.
  const effortStated =
    Object.hasOwn(value, "effort") &&
    value.effort !== null &&
    value.effort !== undefined;
  if (
    effortStated &&
    !["low", "medium", "high"].includes(value.effort as string)
  ) {
    return {
      ok: false,
      key: "routing.override.effort",
      why: "must be low, medium or high (or null to take the tier's); xhigh, max and ultra are not on this wire",
    };
  }
  return {
    ok: true,
    override: {
      model: value.model,
      ...(effortStated ? { effort: value.effort as RoutingEffort } : {}),
      because: value.because,
    },
  };
}

function readReviewFlags(
  value: unknown,
): { ok: true; flags: RoutingReviewFlagTrigger[] } | Refusal {
  if (!Array.isArray(value)) {
    return {
      ok: false,
      key: "routing.reviewFlags",
      why: `must be an array of ${ROUTING_REVIEW_FLAG_TRIGGERS.join(", ")}`,
    };
  }
  const allowed = new Set<string>(ROUTING_REVIEW_FLAG_TRIGGERS);
  const flags: RoutingReviewFlagTrigger[] = [];
  for (const [index, entry] of value.entries()) {
    if (typeof entry !== "string" || !allowed.has(entry)) {
      return {
        ok: false,
        key: `routing.reviewFlags[${index}]`,
        why: `not a §6 flag trigger; the six are ${ROUTING_REVIEW_FLAG_TRIGGERS.join(", ")}`,
      };
    }
    flags.push(entry as RoutingReviewFlagTrigger);
  }
  return { ok: true, flags };
}

function readProposal(
  value: unknown,
): { ok: true; proposal: CodingSessionRoutingProposal } | Refusal {
  if (!isRecord(value)) {
    return {
      ok: false,
      key: "routing.proposed",
      why: "must be { chosen, runnerUp?, reason, registryVersion, catalogRevision }",
    };
  }
  for (const key of Object.keys(value)) {
    if (
      ![
        "chosen",
        "runnerUp",
        "reason",
        "registryVersion",
        "catalogRevision",
      ].includes(key)
    ) {
      return {
        ok: false,
        key: `routing.proposed.${key}`,
        why: "unknown key on the requester's own decision",
      };
    }
  }
  const chosen = readTarget(value.chosen, "routing.proposed.chosen");
  if (!chosen.ok) return chosen;
  let runnerUp: RoutedExecutionTarget | null = null;
  if (Object.hasOwn(value, "runnerUp") && value.runnerUp !== null) {
    const read = readTarget(value.runnerUp, "routing.proposed.runnerUp");
    if (!read.ok) return read;
    runnerUp = read.target;
  }
  if (typeof value.reason !== "string" || value.reason.trim().length === 0) {
    return {
      ok: false,
      key: "routing.proposed.reason",
      why: "a proposal with no reason is a target nobody can argue with",
    };
  }
  if (!Number.isSafeInteger(value.registryVersion)) {
    return {
      ok: false,
      key: "routing.proposed.registryVersion",
      why: "must be an integer",
    };
  }
  if (
    value.catalogRevision !== null &&
    !(
      Number.isSafeInteger(value.catalogRevision) &&
      (value.catalogRevision as number) > 0
    )
  ) {
    return {
      ok: false,
      key: "routing.proposed.catalogRevision",
      why: "must be a positive integer, or null when the requester read none",
    };
  }
  return {
    ok: true,
    proposal: {
      chosen: chosen.target,
      runnerUp,
      reason: value.reason,
      registryVersion: value.registryVersion as number,
      catalogRevision: value.catalogRevision as number | null,
    },
  };
}

function readTarget(
  value: unknown,
  key: string,
): { ok: true; target: RoutedExecutionTarget } | Refusal {
  if (!isRecord(value)) {
    return { ok: false, key, why: "must be { provider, model, effort }" };
  }
  for (const field of Object.keys(value)) {
    if (!["provider", "model", "effort"].includes(field)) {
      return {
        ok: false,
        key: `${key}.${field}`,
        why: "unknown key on a target",
      };
    }
  }
  for (const field of ["provider", "model"] as const) {
    const held = value[field];
    if (typeof held !== "string" || held.trim().length === 0) {
      return {
        ok: false,
        key: `${key}.${field}`,
        why: "must be a non-empty string",
      };
    }
  }
  if (!["low", "medium", "high"].includes(value.effort as string)) {
    return {
      ok: false,
      key: `${key}.effort`,
      why: "must be low, medium or high; xhigh, max and ultra are human-override only",
    };
  }
  return {
    ok: true,
    target: {
      provider: value.provider as string,
      model: value.model as string,
      effort: value.effort as RoutingEffort,
    },
  };
}

/**
 * Where this host's copy of the shared registry came from.
 *
 * `unreadable` is a first-class answer, not an error path: it is what a
 * desktop with no registry reader honestly holds, and the surfaces that show
 * it say so rather than showing a registry nobody loaded.
 */
export type CodingSessionRegistrySource =
  | { kind: "readable"; text: string; label: string }
  | { kind: "unreadable"; why: string };

/** The one sentence a host with no registry refuses a routed hire with. */
export const CODING_SESSION_ROUTING_REGISTRY_UNREADABLE =
  "registry not readable on this host";

export type CodingSessionHireRoutingOutcome =
  | { kind: "not-requested" }
  | { kind: "routed"; record: CodingSessionRoutingRecord }
  | {
      kind: "refused";
      code: typeof HIRE_NO_ROUTE | typeof HIRE_MALFORMED;
      reason: string;
    };

export type ResolveCodingSessionHireRoutingInput = {
  /** The hire's own `routing`, or undefined on a pre-amendment hire. */
  request: CodingSessionHireRoutingRequest | undefined;
  /** The model the hire named, if any — read only as the override's echo. */
  requestedModel: string | null;
  /** This host's registry, or the reason it has none. */
  registry: CodingSessionRegistrySource;
  /**
   * The catalog the seat's own runtime publishes.
   *
   * Restricted to that one runtime on purpose. The identity decides the
   * runtime — a codex identity does not run on the Claude adapter whatever
   * anybody asked for (item 88(i), live 2026-08-28) — so the router chooses
   * *within* it. A router free to pick another vendor here would produce a
   * decision the host then had to quietly ignore.
   */
  providerInstanceRef: string;
  offeredModels: readonly string[];
  /** The 44222 revision behind `offeredModels`, or null when none was read. */
  catalogRevision: number | null;
  /** The seat this one reviews, for the cross-provider rule. */
  peer?: { className: string; provider: string } | null;
  /** A by-rule challenger sample asserted by the caller, when the request does not. */
  sampleChallenger?: boolean;
};

/**
 * Route one hire, or say why it cannot be routed.
 *
 * `not-requested` is the pre-amendment path and changes nothing: a hire that
 * says nothing about routing is seated exactly as it was before, on the
 * identity's own model. Only a hire that *asks* to be routed can be refused
 * for a registry this host cannot read — a host that refused every hire
 * because of a file nobody had asked it to use would be a regression dressed
 * up as rigour.
 */
export function resolveCodingSessionHireRouting(
  input: ResolveCodingSessionHireRoutingInput,
): CodingSessionHireRoutingOutcome {
  const given = input.request;
  if (given === undefined) return { kind: "not-requested" };
  // Read again even when a caller hands in a typed object: this is exported,
  // and a router that trusts its own type annotation is a router one unchecked
  // caller away from throwing inside an event handler. Same reader, same
  // sentence, so a request refused here is refused for the reason the wire
  // would have given.
  const read = readCodingSessionHireRoutingRequest(given);
  if (!read.ok) {
    return {
      kind: "refused",
      code: HIRE_MALFORMED,
      reason: `${read.key}: ${read.why}`,
    };
  }
  const request = read.request;
  const override = readOverride(request, input.requestedModel);
  if (override.kind === "invalid") {
    return { kind: "refused", code: HIRE_MALFORMED, reason: override.why };
  }
  if (input.registry.kind === "unreadable") {
    return {
      kind: "refused",
      code: HIRE_NO_ROUTE,
      reason: `${CODING_SESSION_ROUTING_REGISTRY_UNREADABLE}. ${input.registry.why}`,
    };
  }
  const parsed = parseModelRegistry(input.registry.text);
  if (!parsed.ok) {
    return {
      kind: "refused",
      code: HIRE_NO_ROUTE,
      reason: `${CODING_SESSION_ROUTING_REGISTRY_UNREADABLE}. ${parsed.why}`,
    };
  }
  const catalog: RoutingCatalogEntry[] = input.offeredModels.map((model) => ({
    providerInstanceRef: input.providerInstanceRef,
    model,
  }));
  if (catalog.length === 0) {
    return {
      kind: "refused",
      code: HIRE_NO_ROUTE,
      reason:
        `this computer has read no model catalog for ${input.providerInstanceRef}, ` +
        "so there is no live offer to route against.",
    };
  }
  const sampleChallenger =
    request.challengerSample === true || input.sampleChallenger === true;
  const decision = routeCodingSession({
    registry: parsed.registry,
    catalog,
    catalogRevision: input.catalogRevision,
    className: request.class,
    risk: request.risk,
    ...(request.profile ? { profile: request.profile } : {}),
    ...(request.reviewFlags && request.reviewFlags.length > 0
      ? { review: reviewFlagsObject(request.reviewFlags) }
      : {}),
    ...(input.peer ? { peer: input.peer } : {}),
    ...(sampleChallenger ? { sampleChallenger: true } : {}),
    ...(override.kind === "present" ? { override: override.value } : {}),
  });
  if (!decision.ok) {
    return { kind: "refused", code: decision.code, reason: decision.reason };
  }
  const disagreement = describeProposedDisagreement(
    request.proposed,
    decision.record.chosen,
  );
  return {
    kind: "routed",
    record:
      disagreement === null
        ? decision.record
        : { ...decision.record, proposedDisagreement: disagreement },
  };
}

/** The §6 flag list, as the router's own flag object. */
function reviewFlagsObject(
  flags: readonly RoutingReviewFlagTrigger[],
): Record<RoutingReviewFlagTrigger, boolean> {
  const object = {} as Record<RoutingReviewFlagTrigger, boolean>;
  for (const flag of flags) object[flag] = true;
  return object;
}

/**
 * One sentence when this host's answer is not the one the requester proposed.
 *
 * Written onto the record so the difference travels with the seat. A host that
 * quietly replaced a proposal would leave the requester reading its own local
 * decision in its own logs while a different model did the work — the same
 * shape of lie as a "default" label hiding the real model.
 */
export function describeProposedDisagreement(
  proposed: CodingSessionRoutingProposal | undefined,
  chosen: RoutedExecutionTarget,
): string | null {
  if (proposed === undefined) return null;
  if (
    proposed.chosen.provider === chosen.provider &&
    proposed.chosen.model === chosen.model &&
    proposed.chosen.effort === chosen.effort
  ) {
    return null;
  }
  return (
    `the request proposed ${proposed.chosen.provider}/${proposed.chosen.model} ` +
    `(${proposed.chosen.effort}); this host routed ${chosen.provider}/` +
    `${chosen.model} (${chosen.effort}) against its own registry and live ` +
    "catalog, and the seat runs what this host routed."
  );
}

/**
 * The override a routed hire carries, if it carries one.
 *
 * `action.model` at the top of the hire is an *echo* of `override.model`, not
 * a second place to name a target: the contract sets it only when an override
 * is present, and then the two are equal. A model with no override is
 * therefore a malformed hire — never a route the host quietly obeys, and never
 * `HIRE_NO_ROUTE`, which would blame the registry for a mistake in the
 * request.
 */
function readOverride(
  request: CodingSessionHireRoutingRequest,
  requestedModel: string | null,
):
  | { kind: "absent" }
  | {
      kind: "present";
      value: { model: string; effort?: RoutingEffort; because: string };
    }
  | { kind: "invalid"; why: string } {
  const named = (requestedModel ?? "").trim();
  const override = request.override;
  if (override === undefined) {
    if (named.length === 0) return { kind: "absent" };
    return {
      kind: "invalid",
      why:
        `the hire sets action.model to ${named} but carries no ` +
        "routing.override: model without override.because. A routed hire that " +
        "names a model is overriding the router, and an override is a model " +
        "and a reason or neither.",
    };
  }
  if (named.length > 0 && named !== override.model) {
    return {
      kind: "invalid",
      why:
        `the hire sets action.model to ${named} and routing.override.model to ` +
        `${override.model}; on a routed hire the two are the same id or ` +
        "action.model is null.",
    };
  }
  return { kind: "present", value: override };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
