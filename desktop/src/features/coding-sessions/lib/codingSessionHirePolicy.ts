/**
 * The founder's standing hiring policy, and the decision it produces.
 *
 * A lead asking for a seat is asking *this computer* to mint an identity,
 * stage its key material, cut it a worktree, and run a process — so the answer
 * is never "yes because an agent asked". It is a policy the operator set
 * before any lead existed, applied to facts this host can check: is hiring on,
 * is this role one the operator allowed, is the umbrella already at its seat
 * ceiling, is the runtime one the operator allowed and this host can run, and
 * is there an installed identity that *is* this role and is not already
 * sitting in this umbrella.
 *
 * Every refusal carries a code and a sentence, because the refusal is
 * published back to the lead as a turn (`hire refused: <code> — <reason>`) and
 * a lead that is told "no" without being told *which* no cannot act. A silent
 * drop would be the worst outcome of all: the lead would wait out its whole
 * turn budget for a seat that was never coming.
 *
 * Persisted in localStorage: this is a device-level policy about what this
 * machine will run, not a fact about any session, so it never becomes a signed
 * event.
 */

import {
  codingSessionHireModelNotice,
  codingSessionHireModelOf,
  describeCodingSessionHireModelRefusal,
  resolveCodingSessionHireModel,
} from "./codingSessionHireModel";

/** The stored policy, exactly as the settings panel edits it. */
export type CodingSessionHirePolicy = {
  /** Master switch. Off means every hire is refused `HIRE_OFF`. */
  enabled: boolean;
  /**
   * Roles a lead may hire. `null` — the default — means *every role this
   * computer has an installed pack for*, which is the honest open setting: it
   * cannot offer a role whose craft is not on disk.
   */
  allowedRoles: string[] | null;
  /** Live seats one umbrella may hold before a hire is refused `HIRE_LIMIT`. */
  maxSeatsPerUmbrella: number;
  /** Runtimes a lead may hire onto. `null` means every runtime this host runs. */
  allowedProviderInstanceRefs: string[] | null;
};

/**
 * On, every installed role, four seats, every provider.
 *
 * Four is the same number the session capacity ceiling ships with: a team that
 * can hire past what this computer will run would only be discovering the
 * capacity refusal one seat at a time.
 */
export const DEFAULT_CODING_SESSION_HIRE_POLICY: CodingSessionHirePolicy = {
  enabled: true,
  allowedRoles: null,
  maxSeatsPerUmbrella: 4,
  allowedProviderInstanceRefs: null,
};

/** Above this, a seat ceiling is not a ceiling any more. */
export const CODING_SESSION_HIRE_MAX_SEATS_CEILING = 32;

/** localStorage key for the stored policy. */
export const CODING_SESSION_HIRE_POLICY_STORAGE_KEY =
  "buzz.codingSessions.hirePolicy.v1";

/** The refusal codes the contract defines. Nothing else is published. */
export type CodingSessionHireRefusalCode =
  | "HIRE_OFF"
  | "HIRE_ROLE_NOT_ALLOWED"
  | "HIRE_LIMIT"
  | "HIRE_NO_IDENTITY"
  | "HIRE_PROVIDER_NOT_ALLOWED"
  | "HIRE_MODEL_NOT_OFFERED"
  | "HIRE_STALE";

/** A managed agent this computer could seat, as the decision needs it. */
export type CodingSessionHireCandidate = {
  /** Lowercase 64-hex public key of the managed agent. */
  pubkey: string;
  /** Display name, as the agents list shows it. */
  name: string;
  /** The role this identity *is*. D12: seat role is home role, always. */
  homeRole: string | null;
  /**
   * Whether this computer holds the role pack behind it. `false` means the
   * seat would run on its persona prompt alone; `undefined` means the backend
   * never answered, which is not the same claim and must not subtract a role.
   */
  hasRolePack?: boolean;
  /** The identity's own model, used when the hire names none. */
  model?: string | null;
};

/** A seat already sitting in the umbrella a hire is aimed at. */
export type CodingSessionHireLiveSeat = {
  /** The seated actor's public key. */
  actor: string;
  /** Its role slug. */
  role: string;
};

export type CodingSessionHireDecision =
  | {
      ok: true;
      /** The identity this host will seat. */
      identity: CodingSessionHireCandidate;
      /** The role the seat takes — the request's, which is the identity's. */
      role: string;
      /** Runtime the create is published against. */
      providerInstanceRef: string;
      /** Model the create carries, or null to let the runtime choose. */
      model: string | null;
      /**
       * What the host substituted for the lead's words, when it substituted
       * anything. Null when the model is exactly the one asked for (or none
       * was asked for): a disclosure nobody needs is noise, and noise is how
       * a real disclosure gets skipped.
       */
      modelNotice: string | null;
    }
  | { ok: false; code: CodingSessionHireRefusalCode; reason: string };

export type CodingSessionHireDecisionInput = {
  request: {
    role: string;
    providerInstanceRef: string | null;
    model: string | null;
  };
  policy: CodingSessionHirePolicy;
  /** Every managed agent this computer holds. */
  candidates: readonly CodingSessionHireCandidate[];
  /** Seats already live in the umbrella the hire names. */
  liveSeats: readonly CodingSessionHireLiveSeat[];
  /**
   * Runtimes this host can create against, most-preferred first. The first
   * one the policy also allows is the default a hire naming no provider gets.
   */
  availableProviderInstanceRefs: readonly string[];
  /**
   * What each runtime on this computer says it offers, by instance ref. A ref
   * this map has no entry for is a catalog nobody read: the model passes
   * through untouched rather than being refused against a list that does not
   * exist. See `codingSessionHireModel.ts`.
   */
  modelCatalogs?: ReadonlyMap<string, readonly string[]>;
};

/**
 * The roles this computer will honour a hire for.
 *
 * With an explicit list, that list. With the default (`null`), every role some
 * installed identity *is* and holds a pack for — a computer cannot honestly
 * offer craft it does not have on disk. `hasRolePack === undefined` is a
 * backend that never answered, so it subtracts nothing.
 *
 * Sorted, so the settings panel and the refusal sentence read the same order.
 */
export function codingSessionHireAllowedRoles(
  policy: CodingSessionHirePolicy,
  candidates: readonly CodingSessionHireCandidate[],
): string[] {
  if (policy.allowedRoles !== null) return [...policy.allowedRoles];
  const roles = new Set<string>();
  for (const candidate of candidates) {
    const role = candidate.homeRole?.trim();
    if (!role || candidate.hasRolePack === false) continue;
    roles.add(role);
  }
  return [...roles].sort();
}

/**
 * Answer one hire request against the standing policy.
 *
 * Order matters and is deliberate: the operator's own switches are read
 * before this computer's inventory, so a lead is told "you may not" before it
 * is told "there is nobody", and a refusal on a role the operator disallowed
 * never discloses which identities exist behind it.
 */
export function decideCodingSessionHire(
  input: CodingSessionHireDecisionInput,
): CodingSessionHireDecision {
  const { policy, request } = input;
  if (!policy.enabled) {
    return {
      ok: false,
      code: "HIRE_OFF",
      reason:
        "hiring is switched off on this computer. The operator turns it on " +
        "in Settings → Sessions → Hiring.",
    };
  }

  const role = request.role.trim();
  const allowedRoles = codingSessionHireAllowedRoles(policy, input.candidates);
  if (!allowedRoles.includes(role)) {
    return {
      ok: false,
      code: "HIRE_ROLE_NOT_ALLOWED",
      reason:
        `this computer does not hire ${role} seats. It hires ` +
        `${allowedRoles.length > 0 ? allowedRoles.join(", ") : "no roles at all"}.`,
    };
  }

  if (input.liveSeats.length >= policy.maxSeatsPerUmbrella) {
    return {
      ok: false,
      code: "HIRE_LIMIT",
      reason:
        `this session already holds ${input.liveSeats.length} live seat` +
        `${input.liveSeats.length === 1 ? "" : "s"}, and this computer allows ` +
        `${policy.maxSeatsPerUmbrella} per session. End a seat before hiring another.`,
    };
  }

  const provider = chooseProvider(input);
  if (provider === null) {
    return {
      ok: false,
      code: "HIRE_PROVIDER_NOT_ALLOWED",
      reason: describeProviderRefusal(input),
    };
  }

  // Against the runtime's own catalog, and only for the model the *lead*
  // named: an identity's stored model is this computer's own record and is
  // not the lead's request to be refused over.
  const resolution = resolveCodingSessionHireModel(
    request.model,
    input.modelCatalogs?.get(provider) ?? [],
  );
  if (resolution?.kind === "not-offered") {
    return {
      ok: false,
      code: "HIRE_MODEL_NOT_OFFERED",
      reason: describeCodingSessionHireModelRefusal(provider, resolution),
    };
  }

  const identity = chooseIdentity(role, input);
  if (identity === null) {
    return {
      ok: false,
      code: "HIRE_NO_IDENTITY",
      reason:
        `this computer holds no ${role} identity that is free — every ` +
        `installed ${role} is already seated in this session, or none is ` +
        "installed. Install team roles on the Agents screen, then ask again.",
    };
  }

  return {
    ok: true,
    identity,
    role,
    providerInstanceRef: provider,
    // The lead's choice first (D13 makes the model the lead's call), then the
    // identity's own. Never a guess: null means "let the runtime decide", and
    // that is a different statement from naming a model nobody chose.
    model: codingSessionHireModelOf(resolution) ?? identity.model ?? null,
    modelNotice: codingSessionHireModelNotice(provider, resolution),
  };
}

/** The exact sentence published back to the requesting seat as a 44220 turn. */
export function formatCodingSessionHireRefusal(refusal: {
  code: CodingSessionHireRefusalCode;
  reason: string;
}): string {
  return `hire refused: ${refusal.code} — ${refusal.reason}`;
}

/** Serialize the policy for localStorage. */
export function serializeCodingSessionHirePolicy(
  policy: CodingSessionHirePolicy,
): string {
  return JSON.stringify(policy);
}

/**
 * Read a stored policy, falling back to the default for anything unreadable.
 *
 * Deliberately total: a corrupt preference is a preference nobody set, and a
 * host that refused to hire because its own JSON was broken would look exactly
 * like a host whose operator switched hiring off.
 */
export function parseCodingSessionHirePolicy(
  raw: string | null,
): CodingSessionHirePolicy {
  if (raw === null || raw.trim().length === 0) {
    return { ...DEFAULT_CODING_SESSION_HIRE_POLICY };
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return { ...DEFAULT_CODING_SESSION_HIRE_POLICY };
  }
  if (
    typeof parsed !== "object" ||
    parsed === null ||
    Array.isArray(parsed) ||
    typeof (parsed as { enabled?: unknown }).enabled !== "boolean"
  ) {
    return { ...DEFAULT_CODING_SESSION_HIRE_POLICY };
  }
  const record = parsed as Record<string, unknown>;
  const maxSeats = record.maxSeatsPerUmbrella;
  return {
    enabled: record.enabled === true,
    allowedRoles: readStringList(record.allowedRoles),
    maxSeatsPerUmbrella:
      typeof maxSeats === "number" &&
      Number.isSafeInteger(maxSeats) &&
      maxSeats >= 1
        ? Math.min(maxSeats, CODING_SESSION_HIRE_MAX_SEATS_CEILING)
        : DEFAULT_CODING_SESSION_HIRE_POLICY.maxSeatsPerUmbrella,
    allowedProviderInstanceRefs: readStringList(
      record.allowedProviderInstanceRefs,
    ),
  };
}

/** Read the stored policy from this device. */
export function readCodingSessionHirePolicy(): CodingSessionHirePolicy {
  if (typeof window === "undefined") {
    return { ...DEFAULT_CODING_SESSION_HIRE_POLICY };
  }
  try {
    return parseCodingSessionHirePolicy(
      window.localStorage.getItem(CODING_SESSION_HIRE_POLICY_STORAGE_KEY),
    );
  } catch {
    return { ...DEFAULT_CODING_SESSION_HIRE_POLICY };
  }
}

/** Store the policy on this device. */
export function writeCodingSessionHirePolicy(
  policy: CodingSessionHirePolicy,
): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(
      CODING_SESSION_HIRE_POLICY_STORAGE_KEY,
      serializeCodingSessionHirePolicy(policy),
    );
  } catch {
    // Storage full or blocked: the policy still governs this run.
  }
}

/**
 * Coerce typed input into a storable seat ceiling.
 *
 * Empty, non-numeric and below-one input keeps the previous value: clearing a
 * field must never be read as "allow no seats at all", which would switch
 * hiring off without saying so.
 */
export function parseCodingSessionHireMaxSeatsInput(
  raw: string,
  previous: number,
): number {
  const parsed = Number.parseInt(raw.trim(), 10);
  if (!Number.isFinite(parsed) || parsed < 1) return previous;
  return Math.min(parsed, CODING_SESSION_HIRE_MAX_SEATS_CEILING);
}

/** The runtime a hire lands on, or null when there is none it may have. */
function chooseProvider(input: CodingSessionHireDecisionInput): string | null {
  const allowed = input.policy.allowedProviderInstanceRefs;
  const permitted = (ref: string) => allowed === null || allowed.includes(ref);
  const requested = input.request.providerInstanceRef;
  if (requested !== null) {
    return input.availableProviderInstanceRefs.includes(requested) &&
      permitted(requested)
      ? requested
      : null;
  }
  return input.availableProviderInstanceRefs.find(permitted) ?? null;
}

function describeProviderRefusal(
  input: CodingSessionHireDecisionInput,
): string {
  const requested = input.request.providerInstanceRef;
  if (input.availableProviderInstanceRefs.length === 0) {
    return "this computer is running no coding-session provider, so there is nothing to seat a hire on.";
  }
  if (requested === null) {
    return (
      "no runtime this computer runs is on the operator's allowed list, so " +
      "there is nothing to seat a hire on."
    );
  }
  return (
    `this computer will not seat a hire on ${requested}. It offers ` +
    `${input.availableProviderInstanceRefs.join(", ")}, and the operator's ` +
    `allowed list is ${
      input.policy.allowedProviderInstanceRefs === null
        ? "every one of them"
        : input.policy.allowedProviderInstanceRefs.join(", ")
    }.`
  );
}

/**
 * The identity that takes the seat.
 *
 * Only an identity whose *home* role is the hired role, because D12 fixes seat
 * role to home role — a builder seated as an architect would carry the builder
 * pack and be briefed as one. An identity already live in this umbrella is
 * skipped rather than re-seated: two executions of one identity in one session
 * are two processes signing as the same agent, and nothing downstream can tell
 * their turns apart.
 *
 * Deterministic: a staged pack beats an unstaged one, then name, then pubkey —
 * so the same request answered twice picks the same identity.
 */
function chooseIdentity(
  role: string,
  input: CodingSessionHireDecisionInput,
): CodingSessionHireCandidate | null {
  const live = new Set(
    input.liveSeats.map((seat) => seat.actor.trim().toLowerCase()),
  );
  const eligible = input.candidates.filter(
    (candidate) =>
      candidate.homeRole?.trim() === role &&
      !live.has(candidate.pubkey.trim().toLowerCase()),
  );
  eligible.sort(
    (left, right) =>
      packRank(left) - packRank(right) ||
      left.name.localeCompare(right.name) ||
      left.pubkey.localeCompare(right.pubkey),
  );
  return eligible[0] ?? null;
}

function packRank(candidate: CodingSessionHireCandidate): number {
  return candidate.hasRolePack === false ? 1 : 0;
}

function readStringList(value: unknown): string[] | null {
  if (!Array.isArray(value)) return null;
  return value
    .filter((entry): entry is string => typeof entry === "string")
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
}
