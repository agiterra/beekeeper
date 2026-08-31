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

import { truncatePubkey } from "@/shared/lib/pubkey";
import {
  codingSessionHireModelOf,
  describeCodingSessionHireIdentityModelRefusal,
  describeCodingSessionHireModelRefusal,
  resolveCodingSessionHireModel,
  type CodingSessionHireModelResolution,
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
 * On, every installed role, ten seats, every provider.
 *
 * Ten is the same number the session capacity ceiling ships with: a team that
 * can hire past what this computer will run would only be discovering the
 * capacity refusal one seat at a time.
 */
export const DEFAULT_CODING_SESSION_HIRE_POLICY: CodingSessionHirePolicy = {
  enabled: true,
  allowedRoles: null,
  maxSeatsPerUmbrella: 10,
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
  | "HIRE_ROLE_BUSY"
  | "HIRE_PROVIDER_NOT_ALLOWED"
  | "HIRE_MODEL_NOT_OFFERED"
  | "HIRE_STALE"
  /**
   * The hire asked to be routed and nothing could be routed to it — no
   * registry this host can read, no live catalog, or no execution target that
   * clears the class's gates. Never a quietly weakened requirement: spec §7
   * step 12.
   */
  | "HIRE_NO_ROUTE"
  /**
   * This host could not read the hire's own shape, and says which key.
   *
   * Distinct from `HIRE_NO_ROUTE` on purpose: `NO_ROUTE` is a fact about this
   * computer's registry and catalog, `MALFORMED` is a fact about the request.
   * Answering a bad payload with `NO_ROUTE` would send a lead looking at a
   * registry that is fine. Before 2026-08-30 a malformed hire earned no code
   * at all — it was dropped with no 44220, no console line and no pixel, and a
   * lead waited fifteen minutes on a computer that had already read and
   * discarded its request (ledger draft 97).
   */
  | "HIRE_MALFORMED";

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
  /**
   * The runtime this identity's own record pins — `claude`, `codex`, `goose`
   * — or `null` when it inherits one.
   *
   * This is what decides the seat's runtime, not the umbrella's: on
   * 2026-08-28 a codex identity (`gpt-5.6-sol`) was seated on the
   * claude-agent-acp driver because the host took the umbrella's runtime and
   * passed the identity's model through it, so an OpenAI model id was handed
   * to Claude (item 88(i)).
   */
  runtime?: string | null;
  /**
   * The record's inference provider, when it names one.
   *
   * Read only as a fallback for {@link runtime}: every managed-agent record
   * this repo has been observed to hold leaves `provider` null and pins the
   * vendor in `runtime` instead.
   */
  provider?: string | null;
};

/** A seat already sitting in the umbrella a hire is aimed at. */
export type CodingSessionHireLiveSeat = {
  /** The seated actor's public key. */
  actor: string;
  /** Its role slug. */
  role: string;
  /**
   * The seat's execution generation id, when this host has observed one.
   *
   * Carried only so a busy-role refusal can name the seat the lead should
   * address instead. Absent is normal — the refusal names the actor and role
   * either way, and never invents an id.
   */
  generationId?: string | null;
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
       * What the host substituted for the lead's words about the model.
       *
       * Always `null` since Brian's 2026-08-29 ruling removed alias
       * translation: a model is either the exact id that was asked for or it
       * is refused, so there is never a substitution to disclose. The field
       * stays because the seat plan and the umbrella line that renders it
       * (`codingSessionHireSeat.ts`, `codingSessionHireAnswer.ts`) are the
       * general channel for "something about this seat's model" — it must not
       * be filled with a guess.
       */
      modelNotice: string | null;
      /**
       * What the host substituted for the runtime the hire named, when it
       * substituted anything — an identity's own runtime overriding the
       * request's. Null when the seat runs exactly where the hire asked.
       */
      providerNotice: string | null;
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
  /**
   * Runtime slug (`claude`, `codex`, `goose`) by instance ref, so an
   * identity's own runtime can be matched to a runtime this computer runs.
   * Absent means no mapping was read, and an identity's runtime then matches
   * only an instance ref equal to it — never a guess.
   */
  providerRuntimeSlugs?: ReadonlyMap<string, string>;
  /**
   * True when the hire asked to be routed, in which case the model on the
   * seat is the router's to choose.
   *
   * It changes exactly one thing: the identity's *own* model is not inherited
   * and not checked against the catalog. That check exists so a record naming
   * a model the runtime does not offer is refused instead of silently
   * replaced (item 88(a)); on a routed hire nothing is inherited, so the same
   * record would refuse a hire whose model the router had not chosen yet. A
   * model the hire itself names is still checked, because on a routed hire
   * that is a human override and an override still has to name a real id.
   */
  routed?: boolean;
  /**
   * Runtime instances preferred for this identity choice, in host order.
   *
   * This is a preference, not permission: the candidate must still own the
   * requested role, be free in this umbrella, have the best available pack,
   * and resolve to a runtime the host can actually seat.
   */
  preferredProviderInstanceRefs?: readonly string[];
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
 * Order matters and is deliberate. The operator's own switches — hiring on,
 * this role, this seat ceiling — are read before this computer's inventory,
 * so a lead is told "you may not" before it is told "there is nobody", and a
 * refusal on a role the operator disallowed never discloses which identities
 * exist behind it.
 *
 * The identity is then chosen **before** the runtime and the model, because
 * it decides both: a codex identity's runtime is the seat's runtime, and the
 * catalog a model is checked against is that runtime's. Choosing the runtime
 * first is what seated an OpenAI model id on the Claude adapter on
 * 2026-08-28 (item 88(i)).
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

  // Identity before runtime, because the identity decides the runtime: a
  // codex identity does not run on the Claude adapter whatever the hire said.
  const identity = chooseIdentity(role, input);
  if (identity === null) {
    // Busy and absent are two codes, not one sentence: see
    // describeIdentityRefusal.
    return { ok: false, ...describeIdentityRefusal(role, input) };
  }

  const provider = chooseProvider(input, identity);
  if (provider.ok === false) {
    return {
      ok: false,
      code: "HIRE_PROVIDER_NOT_ALLOWED",
      reason: provider.reason,
    };
  }

  const offered = input.modelCatalogs?.get(provider.ref) ?? [];
  // The lead's model against the runtime's own catalog.
  const requested = resolveCodingSessionHireModel(request.model, offered);
  if (requested?.kind === "not-offered") {
    return {
      ok: false,
      code: "HIRE_MODEL_NOT_OFFERED",
      reason: describeCodingSessionHireModelRefusal(provider.ref, requested),
    };
  }
  // And the identity's own through the same check, when the hire named none.
  // Skipping it is what let `opus[1m]` seat itself on 2026-08-28 one line
  // after the host refused it (item 88(a)).
  let inherited: CodingSessionHireModelResolution | null = null;
  if (requested === null && input.routed !== true) {
    inherited = resolveCodingSessionHireModel(identity.model ?? null, offered);
    if (inherited?.kind === "not-offered") {
      return {
        ok: false,
        code: "HIRE_MODEL_NOT_OFFERED",
        reason: describeCodingSessionHireIdentityModelRefusal(
          provider.ref,
          identity.name,
          inherited,
        ),
      };
    }
  }

  return {
    ok: true,
    identity,
    role,
    providerInstanceRef: provider.ref,
    // The lead's choice first (D13 makes the model the lead's call), then the
    // identity's own. Never a guess: null means "let the runtime decide", and
    // that is a different statement from naming a model nobody chose.
    model:
      codingSessionHireModelOf(requested) ??
      codingSessionHireModelOf(inherited) ??
      null,
    // Nothing is ever substituted for the model, so there is nothing to
    // disclose about it. See the field's doc.
    modelNotice: null,
    providerNotice: provider.notice,
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

/** The runtime a hire lands on, and whether the host chose it for the lead. */
type CodingSessionHireProviderChoice =
  | { ok: true; ref: string; notice: string | null }
  | { ok: false; reason: string };

/**
 * The runtime a hire lands on.
 *
 * Order: **the identity's own runtime**, then the hire's, then this
 * computer's first allowed one. The identity comes first because its runtime
 * is a fact about the agent, while the hire's is a preference of the lead's —
 * and running a codex identity's model on the Claude adapter is not a
 * degraded seat, it is a seat that cannot work (item 88(i), live).
 *
 * An identity naming a runtime this computer does not run is refused rather
 * than re-homed: seating it somewhere else would hand its model id to an
 * adapter that has never heard of it.
 */
function chooseProvider(
  input: CodingSessionHireDecisionInput,
  identity: CodingSessionHireCandidate,
): CodingSessionHireProviderChoice {
  const allowed = input.policy.allowedProviderInstanceRefs;
  const permitted = (ref: string) => allowed === null || allowed.includes(ref);
  const requested = input.request.providerInstanceRef;

  const own = (identity.runtime ?? identity.provider ?? "")
    .trim()
    .toLowerCase();
  if (own.length > 0) {
    const match = input.availableProviderInstanceRefs.find(
      (ref) =>
        permitted(ref) &&
        (ref.trim().toLowerCase() === own ||
          input.providerRuntimeSlugs?.get(ref)?.trim().toLowerCase() === own),
    );
    if (match === undefined) {
      return {
        ok: false,
        reason:
          `${identity.name} runs on ${own}, and this computer seats hires on ` +
          `${
            input.availableProviderInstanceRefs.length > 0
              ? input.availableProviderInstanceRefs.join(", ")
              : "no runtime at all"
          }. Install or sign in to ${own} here, or hire a role whose identity ` +
          "runs on one of those.",
      };
    }
    return {
      ok: true,
      ref: match,
      notice:
        requested !== null && requested !== match
          ? `The hire asked for ${requested}; ${identity.name} runs on ` +
            `${own}, so the seat runs on ${match} instead.`
          : null,
    };
  }

  if (requested !== null) {
    return input.availableProviderInstanceRefs.includes(requested) &&
      permitted(requested)
      ? { ok: true, ref: requested, notice: null }
      : { ok: false, reason: describeProviderRefusal(input) };
  }
  const fallback = input.availableProviderInstanceRefs.find(permitted);
  return fallback === undefined
    ? { ok: false, reason: describeProviderRefusal(input) }
    : { ok: true, ref: fallback, notice: null };
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
 * Why no identity took the seat — busy here, or not installed at all.
 *
 * Two different facts with two different remedies, and until 2026-08-28 they
 * shared one sentence: a lead whose only builder was seated *and idle* was
 * told to "install team roles", which was both wrong and unactionable (item
 * 88(h)). The busy sentence names the seat and the one thing that works —
 * addressing the seat that already exists. It never invents an identity.
 *
 * They now carry two codes as well (item 89). A lead acts on the code before
 * it reads the prose, so a busy role answered `HIRE_NO_IDENTITY` still
 * pointed every code-driven reader — the `bee sessions hire` remedy table
 * included — at the operator's install remedy for a role this computer holds.
 */
function describeIdentityRefusal(
  role: string,
  input: CodingSessionHireDecisionInput,
): { code: CodingSessionHireRefusalCode; reason: string } {
  const seated = input.liveSeats.filter((seat) => {
    const actor = seat.actor.trim().toLowerCase();
    return input.candidates.some(
      (candidate) =>
        candidate.homeRole?.trim() === role &&
        candidate.pubkey.trim().toLowerCase() === actor,
    );
  });
  if (seated.length === 0) {
    return {
      code: "HIRE_NO_IDENTITY",
      reason:
        `this computer holds no ${role} identity. Install team roles on the ` +
        "Agents screen, then ask again.",
    };
  }
  return {
    code: "HIRE_ROLE_BUSY",
    reason:
      `every ${role} identity this computer holds is already seated in this ` +
      `session: ${seated.map(describeLiveSeat).join(", ")}. Send your brief to ` +
      `that seat instead of hiring: bee sessions send --to ${role}`,
  };
}

/** `abcd1234…wxyz·builder (execution gen-7)` — the seat, as a lead addresses it. */
function describeLiveSeat(seat: CodingSessionHireLiveSeat): string {
  const label = `${truncatePubkey(seat.actor.trim())}·${seat.role}`;
  const generation = seat.generationId?.trim();
  return generation ? `${label} (execution ${generation})` : label;
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
 * Deterministic: a staged pack beats an unstaged one, then a caller's runtime
 * preference, then name, then pubkey — so the same request answered twice
 * picks the same identity.
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
      providerPreferenceRank(left, input) -
        providerPreferenceRank(right, input) ||
      left.name.localeCompare(right.name) ||
      left.pubkey.localeCompare(right.pubkey),
  );
  return eligible[0] ?? null;
}

function packRank(candidate: CodingSessionHireCandidate): number {
  return candidate.hasRolePack === false ? 1 : 0;
}

function providerPreferenceRank(
  candidate: CodingSessionHireCandidate,
  input: CodingSessionHireDecisionInput,
): number {
  const preferred = input.preferredProviderInstanceRefs;
  if (preferred === undefined || preferred.length === 0) return 0;
  const provider = chooseProvider(input, candidate);
  if (!provider.ok) return 1;
  const index = preferred.indexOf(provider.ref);
  return index < 0 ? preferred.length + 1 : index;
}

function readStringList(value: unknown): string[] | null {
  if (!Array.isArray(value)) return null;
  return value
    .filter((entry): entry is string => typeof entry === "string")
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
}
