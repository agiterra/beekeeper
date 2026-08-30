/**
 * What a lead may say about routing, and what this host does with it.
 *
 * The split is the whole design, and it is Brian's sentence:
 *
 * > "The lead chooses the capability required. The router chooses the
 * > execution target."
 *
 * So a `session.hire` carries a **request** — a class, a risk, optionally
 * extra trait minimums and a human's override — and never a chosen model. The
 * founder's host reads its own live catalog and its own registry, routes, and
 * publishes the **record** on the seat's create, where the class, the tier,
 * the risk, the chosen target, the runner-up, the reason and the review
 * triggers are all readable from the wire.
 *
 * Two refusals live here and both are honest ones:
 *
 * 1. **`registry not readable on this host`.** The desktop has no command that
 *    reads a file out of the project checkout — the only role-pack access it
 *    has is `scanProjectRolePacks` / `pickCrewRolePacksDirectory`, which
 *    return a role, a name and a directory and never file content
 *    (`desktop/src/shared/api/tauriTeams.ts:331` and `:350`). So this host
 *    cannot read `team/model-registry.yaml`, and it refuses rather than
 *    routing on a copy compiled into the app. A hardcoded registry would be a
 *    routing decision nobody on this machine could check against the file the
 *    team edits, which is the same class of defect as a picker offering a
 *    model nobody serves.
 * 2. **An override with no `because`.** A human may name a target by hand —
 *    that is the only way `xhigh`, `max` or `ultra` is ever bought — but the
 *    router's own answer is being set aside, so the record has to say why, and
 *    the named id has to be one the catalog publishes byte for byte.
 */
import {
  HIRE_NO_ROUTE,
  routeCodingSession,
  ROUTING_TRAITS,
  parseModelRegistry,
  type CodingSessionRoutingRecord,
  type RoutingCatalogEntry,
  type RoutingEffort,
  type RoutingRisk,
  type RoutingTier,
  type RoutingTrait,
} from "./codingSessionRouting";

/**
 * The `routing` object a hire carries.
 *
 * Deliberately a subset of the record: `class` and `risk` are the lead's to
 * state, `tier` is an echo the host recomputes and never trusts, `profile` is
 * the lead's extra minimums, and `override` is a human's. Everything else on
 * the record — `chosen`, `runnerUp`, `reason`, `reviewRequired`,
 * `challengerSample`, `registryVersion`, `catalogRevision` — is the router's
 * and is filled in on the create.
 */
export type CodingSessionHireRoutingRequest = {
  class: string;
  /** The lead's own read of the band. Advisory; the host recomputes it. */
  tier?: RoutingTier;
  risk: RoutingRisk;
  profile?: Partial<Record<RoutingTrait, number>>;
  override?: {
    /** A catalog id, exactly as published. Optional: `action.model` may name it. */
    model?: string;
    effort?: RoutingEffort;
    because: string;
  };
};

const HIRE_ROUTING_KEYS = ["class", "tier", "risk", "profile", "override"];

/** Closed-shape check for a hire's `routing`, matching what the relay accepts. */
export function isCodingSessionHireRoutingRequest(
  value: unknown,
): value is CodingSessionHireRoutingRequest {
  if (!isRecord(value)) return false;
  if (Object.keys(value).some((key) => !HIRE_ROUTING_KEYS.includes(key))) {
    return false;
  }
  if (
    typeof value.class !== "string" ||
    !/^[a-z0-9_-]{1,64}$/.test(value.class)
  ) {
    return false;
  }
  if (!isRoutingRisk(value.risk)) return false;
  if (
    Object.hasOwn(value, "tier") &&
    !["fast", "standard", "deep"].includes(value.tier as string)
  ) {
    return false;
  }
  if (Object.hasOwn(value, "profile") && !isRoutingProfile(value.profile)) {
    return false;
  }
  return !Object.hasOwn(value, "override") || isHireOverride(value.override);
}

function isRoutingRisk(value: unknown): value is RoutingRisk {
  if (!isRecord(value)) return false;
  const keys = ["impact", "uncertainty", "irreversibility"];
  if (Object.keys(value).length !== keys.length) return false;
  return keys.every((key) => {
    const factor = value[key];
    return (
      Number.isSafeInteger(factor) &&
      (factor as number) >= 1 &&
      (factor as number) <= 5
    );
  });
}

function isRoutingProfile(value: unknown): boolean {
  if (!isRecord(value)) return false;
  const traits = new Set<string>(ROUTING_TRAITS);
  return Object.entries(value).every(
    ([trait, minimum]) =>
      traits.has(trait) &&
      typeof minimum === "number" &&
      Number.isFinite(minimum) &&
      minimum >= 1 &&
      minimum <= 5,
  );
}

function isHireOverride(value: unknown): boolean {
  if (!isRecord(value)) return false;
  if (
    Object.keys(value).some(
      (key) => !["model", "effort", "because"].includes(key),
    )
  ) {
    return false;
  }
  if (typeof value.because !== "string" || value.because.trim().length === 0) {
    return false;
  }
  if (
    Object.hasOwn(value, "model") &&
    (typeof value.model !== "string" || value.model.trim().length === 0)
  ) {
    return false;
  }
  return (
    !Object.hasOwn(value, "effort") ||
    ["low", "medium", "high"].includes(value.effort as string)
  );
}

/**
 * Where this host's copy of the shared registry came from.
 *
 * `unreadable` is a first-class answer, not an error path: it is what this
 * desktop honestly holds today, and the surfaces that show it say so rather
 * than showing a registry nobody loaded.
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
  | { kind: "refused"; code: typeof HIRE_NO_ROUTE; reason: string };

export type ResolveCodingSessionHireRoutingInput = {
  /** The hire's own `routing`, or undefined on a pre-amendment hire. */
  request: CodingSessionHireRoutingRequest | undefined;
  /** The model the hire named, if any — read as an override, never as a route. */
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
  /** The lead's by-rule challenger sample. */
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
  const request = input.request;
  if (request === undefined) return { kind: "not-requested" };
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
  const override = readOverride(request, input.requestedModel);
  if (override.kind === "invalid") {
    return { kind: "refused", code: HIRE_NO_ROUTE, reason: override.why };
  }
  const decision = routeCodingSession({
    registry: parsed.registry,
    catalog,
    catalogRevision: input.catalogRevision,
    className: request.class,
    risk: request.risk,
    ...(request.profile ? { profile: request.profile } : {}),
    ...(input.peer ? { peer: input.peer } : {}),
    ...(input.sampleChallenger === true ? { sampleChallenger: true } : {}),
    ...(override.kind === "present" ? { override: override.value } : {}),
  });
  if (!decision.ok) {
    return { kind: "refused", code: decision.code, reason: decision.reason };
  }
  return { kind: "routed", record: decision.record };
}

/**
 * The override a routed hire carries, if it carries one.
 *
 * The model may be named in either place — `action.model`, which is where a
 * lead already names one, or `routing.override.model` — but the *reason* has
 * only one home, and without it there is no override, only a hire quietly
 * ignoring its own router.
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
  const named = (request.override?.model ?? requestedModel ?? "").trim();
  const because = (request.override?.because ?? "").trim();
  if (named.length === 0) {
    if (because.length === 0) return { kind: "absent" };
    return {
      kind: "invalid",
      why:
        "the hire gives a reason to override the router but names no model. " +
        "An override is a model and a reason, or neither.",
    };
  }
  if (because.length === 0) {
    return {
      kind: "invalid",
      why:
        `the hire names the model ${named} but says nothing about why. A ` +
        "routed hire that names a model is overriding the router, and an " +
        "override needs routing.override.because.",
    };
  }
  return {
    kind: "present",
    value: {
      model: named,
      ...(request.override?.effort ? { effort: request.override.effort } : {}),
      because,
    },
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
