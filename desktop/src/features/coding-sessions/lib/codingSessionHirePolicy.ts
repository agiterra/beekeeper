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
  agentMaySeatInProject,
  normalizeProjectCoordinate,
} from "@/shared/lib/projectAgentAssociation";
import { truncatePubkey } from "@/shared/lib/pubkey";
import type { HIRE_CHECKOUT_NOT_RECORDED } from "./codingSessionHireCheckout";
import {
  describeCodingSessionHireRuntimeSource,
  type CodingSessionHireModelSource,
  type CodingSessionHireRuntimeSource,
} from "./codingSessionHireAgentRuntime";
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

/**
 * The refusal codes the contract defines. Nothing else is published.
 *
 * Must equal buzz-core's `HIRE_REFUSAL_CODES`
 * (`crates/beekeeper-core/src/coding_session_lifecycle_command.rs`); a parity test
 * reads that file.
 */
export const CODING_SESSION_HIRE_REFUSAL_CODES = [
  "HIRE_OFF",
  "HIRE_ROLE_NOT_ALLOWED",
  "HIRE_LIMIT",
  /**
   * A **projectless** session asked for a role no agent on this computer holds
   * at all. A session in a project is answered `HIRE_NO_PROJECT_AGENT`
   * instead, because its remedy is the project's, not the computer's.
   */
  "HIRE_NO_IDENTITY",
  /**
   * The session's project has no eligible agent for the role on this computer
   * (or, for a projectless session, every agent holding the role belongs to a
   * project). Never answered by seating another project's agent: borrowing is
   * not supported.
   */
  "HIRE_NO_PROJECT_AGENT",
  "HIRE_ROLE_BUSY",
  "HIRE_PROVIDER_NOT_ALLOWED",
  "HIRE_MODEL_NOT_OFFERED",
  "HIRE_STALE",
  /**
   * The hire asked to be routed and nothing could be routed to it — no
   * registry this host can read, no live catalog, or no execution target that
   * clears the class's gates. Never a quietly weakened requirement: spec §7
   * step 12.
   */
  "HIRE_NO_ROUTE",
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
  "HIRE_MALFORMED",
  /**
   * No recorded checkout to cut the seat's worktree from. Only the operator
   * can fix this, in Project settings → This computer → Repository folder.
   * The resolution logic lives in `codingSessionHireCheckout.ts` (135(a), 136).
   */
  "HIRE_CHECKOUT_NOT_RECORDED",
  /** The tree was cut and the seat could not be staged (ledger 169). */
  "HIRE_SEAT_STAGING_FAILED",
] as const;

export type CodingSessionHireRefusalCode =
  (typeof CODING_SESSION_HIRE_REFUSAL_CODES)[number];

/**
 * Persona-id prefix of a project team setup actor. Such an agent authors a
 * project's team and is restaged only through its scoped bootstrap
 * (`project_team_setup_actor_restage.rs is_setup_actor`), so a hire never
 * seats one, whatever its home role says.
 */
export const CODING_SESSION_HIRE_SETUP_ACTOR_PERSONA_PREFIX =
  "project-team-setup:";

/** Whether this candidate is a project team setup actor. */
export function isCodingSessionHireSetupActor(candidate: {
  personaId?: string | null;
}): boolean {
  return (
    candidate.personaId?.startsWith(
      CODING_SESSION_HIRE_SETUP_ACTOR_PERSONA_PREFIX,
    ) === true
  );
}

/** A managed agent this computer could seat, as the decision needs it. */
export type CodingSessionHireCandidate = {
  /** Lowercase 64-hex public key of the managed agent. */
  pubkey: string;
  /** Display name, as the agents list shows it. */
  name: string;
  /** The role this identity *is*. D12: seat role is home role, always. */
  homeRole: string | null;
  /**
   * The project this agent durably belongs to (`ManagedAgent.projectRef`), or
   * null when it belongs to none. The only evidence of project membership a
   * hire reads — see `@/shared/lib/projectAgentAssociation`.
   */
  projectRef: string | null;
  /**
   * The record's persona id, read only to recognise a project team setup
   * actor ({@link isCodingSessionHireSetupActor}). Absent means not known to
   * be one.
   */
  personaId?: string | null;
  /**
   * Whether this computer holds the role pack behind it. `false` means the
   * seat would run on its persona prompt alone; `undefined` means the backend
   * never answered, which is not the same claim and must not subtract a role.
   */
  hasRolePack?: boolean;
  /** The identity's own model, used when the hire names none. */
  model?: string | null;
  /**
   * Which tier {@link model} came from — `ManagedAgent.modelSource`.
   *
   * The effective model is resolved record → persona → global on the host
   * side, so quoting it as "its record says" can name a field that is empty.
   * See {@link describeCodingSessionHireModelSource}.
   */
  modelSource?: CodingSessionHireModelSource | null;
  /**
   * The runtime this identity is **effectively** configured for — `claude`,
   * `codex`, `goose` — or `null` when nothing on its record names one.
   *
   * This is what decides the seat's runtime, not the umbrella's: on
   * 2026-08-28 a codex identity (`gpt-5.6-sol`) was seated on the
   * claude-agent-acp driver because the host took the umbrella's runtime and
   * passed the identity's model through it, so an OpenAI model id was handed
   * to Claude (item 88(i)).
   *
   * **Effective, never the raw per-instance pin.** A record that inherits its
   * harness from its persona pins nothing here, and on 2026-09-16 that read
   * as "this identity names no runtime": Kiln, shown as Codex on the Agents
   * screen, was seated on `claude-primary` and then refused for a model
   * Claude does not offer (ledger 135(b)). `codingSessionHireCandidates`
   * resolves record → harness → provider before this is read.
   */
  runtime?: string | null;
  /**
   * Which tier {@link runtime} was read from, and the raw value it held.
   *
   * Carried so a refusal can cite its evidence instead of asserting "its
   * record says" about a field that may be empty.
   */
  runtimeSource?: CodingSessionHireRuntimeSource | null;
  runtimeRead?: string | null;
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
  /**
   * The project of the umbrella the hire names
   * (`codingSessionHireUmbrellaProjectRef`), or null for a projectless
   * session. It scopes who may be seated: see {@link decideCodingSessionHire}.
   * A runtime `undefined` is read as null.
   */
  projectRef: string | null;
  /** The project's display name for a refusal sentence, when known. */
  projectLabel?: string | null;
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
 *
 * Deliberately **computer-wide**, not scoped to the hire's project: this is
 * the operator's device-level list. A role only another project's agents hold
 * therefore passes this gate and is refused `HIRE_NO_PROJECT_AGENT` by the
 * identity step, which names the project and the remedy — rather than
 * `HIRE_ROLE_NOT_ALLOWED`, which would blame the operator's policy for a
 * missing project agent. Setup actors never count: they are never hired.
 */
export function codingSessionHireAllowedRoles(
  policy: CodingSessionHirePolicy,
  candidates: readonly Pick<
    CodingSessionHireCandidate,
    "homeRole" | "hasRolePack" | "personaId"
  >[],
): string[] {
  if (policy.allowedRoles !== null) return [...policy.allowedRoles];
  const roles = new Set<string>();
  for (const candidate of candidates) {
    const role = candidate.homeRole?.trim();
    if (!role || candidate.hasRolePack === false) continue;
    if (isCodingSessionHireSetupActor(candidate)) continue;
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
 *
 * **Who may be seated is scoped by the umbrella's project** (2026-09-14):
 * Tank Loop's lead was seated the Beekeeper crew because any agent on this
 * computer with a matching home role was eligible and the name broke the tie.
 * Now an eligible identity belongs to the umbrella's project
 * (`agentMaySeatInProject`), holds the role as its home role, is not live in
 * the umbrella and is not a setup actor. A projectless umbrella seats only
 * unassociated agents. No refusal ever falls back to another agent — see
 * {@link describeIdentityRefusal} for the codes.
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
  const pool = codingSessionHireIdentityPool(role, input);
  const identity = chooseIdentity(pool, input);
  if (identity === null) {
    // Busy, absent from the project, and absent from the computer are three
    // codes, not one sentence: see describeIdentityRefusal.
    return { ok: false, ...describeIdentityRefusal(role, pool, input) };
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
          identity.modelSource ?? null,
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

/**
 * The exact sentence published back to the requesting seat as a 44220 turn.
 *
 * The parameter type still spells out {@link HIRE_CHECKOUT_NOT_RECORDED}
 * alongside {@link CodingSessionHireRefusalCode}; as of ledger 139 that code
 * is also one of `CODING_SESSION_HIRE_REFUSAL_CODES`, so the union is
 * redundant but harmless — left as-is rather than narrowed.
 */
export function formatCodingSessionHireRefusal(refusal: {
  code: CodingSessionHireRefusalCode | typeof HIRE_CHECKOUT_NOT_RECORDED;
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
 *
 * **Two refusals, not one.** "This computer does not run codex" and "the
 * operator's allowed list excludes the codex this computer is running" are
 * different facts with different remedies, and until 2026-09-16 they shared
 * one sentence that told a lead to install something already installed. Each
 * names the runtime the agent is configured for, where that was read, and the
 * two ways to change it — never "pick a different model", which is what the
 * lead was told when this path silently fell through to Claude instead
 * (ledger 135(b)).
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
    const runs = (ref: string) =>
      ref.trim().toLowerCase() === own ||
      input.providerRuntimeSlugs?.get(ref)?.trim().toLowerCase() === own;
    const match = input.availableProviderInstanceRefs.find(
      (ref) => permitted(ref) && runs(ref),
    );
    if (match === undefined) {
      // Running here but disallowed, versus not running here at all.
      const running = input.availableProviderInstanceRefs.find(runs);
      return {
        ok: false,
        reason:
          running === undefined
            ? describeRuntimeNotRun(input, identity, own)
            : describeRuntimeNotAllowed(input, identity, own, running),
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

/** How this host describes what it read an identity's runtime from. */
function describeRuntimeEvidence(
  identity: CodingSessionHireCandidate,
  runtime: string,
): string {
  const source = identity.runtimeSource ?? null;
  if (source === null) {
    return `${identity.name} is configured for ${runtime}`;
  }
  return (
    `${identity.name} is configured for ${runtime}, ` +
    `${describeCodingSessionHireRuntimeSource(source, identity.runtimeRead ?? null)}`
  );
}

/** The agent is configured for a runtime this computer is not running. */
function describeRuntimeNotRun(
  input: CodingSessionHireDecisionInput,
  identity: CodingSessionHireCandidate,
  runtime: string,
): string {
  return (
    `${describeRuntimeEvidence(identity, runtime)}, and this computer is ` +
    `running ${
      input.availableProviderInstanceRefs.length > 0
        ? input.availableProviderInstanceRefs.join(", ")
        : "no runtime at all"
    }. Install or sign in to ${runtime} here, or change ${identity.name}'s ` +
    "runtime on the Agents screen. Naming a different model does not help: " +
    "the runtime is the agent's, not the model's."
  );
}

/** The runtime is running here and the operator's list excludes it. */
function describeRuntimeNotAllowed(
  input: CodingSessionHireDecisionInput,
  identity: CodingSessionHireCandidate,
  runtime: string,
  running: string,
): string {
  const list = input.policy.allowedProviderInstanceRefs;
  return (
    `${describeRuntimeEvidence(identity, runtime)}. This computer runs it as ` +
    `${running}, but this session only seats hires on ${
      list === null || list.length === 0 ? "no runtime at all" : list.join(", ")
    }. Allow ${running} in Settings → Sessions → Hiring, or change ` +
    `${identity.name}'s runtime on the Agents screen. Naming a different ` +
    "model does not help: the runtime is the agent's, not the model's."
  );
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
 * The identities a hire for `role` is answered from, before liveness.
 *
 * - `holders` — every non-setup agent on this computer whose home role is
 *   `role`, whatever project it belongs to. Counted, never named, in a
 *   refusal: another project's agent names are not this session's to read.
 * - `project` — the holders that may be seated in this umbrella's project.
 *
 * Fail closed on unreadable coordinates: an umbrella whose project reference
 * is present but not a well-formed `30621:<owner>:<dtag>` matches no agent
 * (it must never collapse to "projectless" and open the seat to unassociated
 * agents), and an agent whose own `projectRef` is present but unreadable
 * belongs to no project a session can prove, so it is never eligible.
 */
type CodingSessionHireIdentityPool = {
  holders: CodingSessionHireCandidate[];
  project: CodingSessionHireCandidate[];
  scope: CodingSessionHireProjectScope;
};

type CodingSessionHireProjectScope =
  | { kind: "project"; projectRef: string; label: string }
  | { kind: "projectless" }
  | { kind: "unreadable"; raw: string };

function codingSessionHireProjectScope(
  input: Pick<CodingSessionHireDecisionInput, "projectRef" | "projectLabel">,
): CodingSessionHireProjectScope {
  const raw = input.projectRef?.trim() ?? "";
  if (raw.length === 0) return { kind: "projectless" };
  const projectRef = normalizeProjectCoordinate(raw);
  if (projectRef === null) return { kind: "unreadable", raw };
  const label = input.projectLabel?.trim();
  return {
    kind: "project",
    projectRef,
    label: label ? label : `project ${projectRef}`,
  };
}

function codingSessionHireIdentityPool(
  role: string,
  input: CodingSessionHireDecisionInput,
): CodingSessionHireIdentityPool {
  const scope = codingSessionHireProjectScope(input);
  const holders = input.candidates.filter(
    (candidate) =>
      candidate.homeRole?.trim() === role &&
      !isCodingSessionHireSetupActor(candidate),
  );
  const project = holders.filter((candidate) => {
    const own = candidate.projectRef?.trim() ?? "";
    if (own.length > 0 && normalizeProjectCoordinate(own) === null) {
      return false;
    }
    if (scope.kind === "unreadable") return false;
    return agentMaySeatInProject(
      candidate,
      scope.kind === "project" ? scope.projectRef : null,
    );
  });
  return { holders, project, scope };
}

/**
 * Why no identity took the seat — busy here, not this project's, or not on
 * this computer at all.
 *
 * Busy and absent were one sentence until 2026-08-28: a lead whose only
 * builder was seated *and idle* was told to "install team roles", which was
 * both wrong and unactionable (item 88(h)). They now carry two codes (item
 * 89), because a lead acts on the code before it reads the prose.
 *
 * Since 2026-09-14 absence is scoped by the umbrella's project:
 *
 * - **`HIRE_ROLE_BUSY`** — every agent that may take this seat *in this
 *   project* is already live in the umbrella. Only project-eligible identities
 *   count; a borrowed or unassociated agent sitting in the umbrella never
 *   makes a role "busy".
 * - **`HIRE_NO_PROJECT_AGENT`** — a session in a project with no eligible
 *   agent for the role here, however many agents of other projects hold it
 *   (counted, never named). And a projectless session where every holder of
 *   the role belongs to a project.
 * - **`HIRE_NO_IDENTITY`** — kept only for a projectless session and a role
 *   no agent on this computer holds at all, whose remedy is still the
 *   computer's operator installing the role.
 *
 * No branch ever falls back to another agent.
 */
function describeIdentityRefusal(
  role: string,
  pool: CodingSessionHireIdentityPool,
  input: CodingSessionHireDecisionInput,
): { code: CodingSessionHireRefusalCode; reason: string } {
  if (pool.project.length > 0) {
    const eligible = new Set(
      pool.project.map((candidate) => candidate.pubkey.trim().toLowerCase()),
    );
    const seated = input.liveSeats.filter((seat) =>
      eligible.has(seat.actor.trim().toLowerCase()),
    );
    // The projectless sentence is the one leads have read since item 89.
    const who =
      pool.scope.kind === "project"
        ? `every ${role} agent of ${pool.scope.label} on this computer is`
        : `every ${role} identity this computer holds is`;
    return {
      code: "HIRE_ROLE_BUSY",
      reason:
        `${who} already seated in this session: ` +
        `${seated.map(describeLiveSeat).join(", ")}. Send your brief to ` +
        `that seat instead of hiring: bee sessions send --to ${role}`,
    };
  }
  if (pool.scope.kind === "projectless") {
    if (pool.holders.length === 0) {
      return {
        code: "HIRE_NO_IDENTITY",
        reason:
          `this computer holds no ${role} identity. Install team roles on the ` +
          "Agents screen, then ask again.",
      };
    }
    return {
      code: "HIRE_NO_PROJECT_AGENT",
      reason:
        `this session has no project, and every ${role} agent on this ` +
        "computer belongs to a project. Start the work in that project's session.",
    };
  }
  const others = pool.holders.length;
  const borrowing =
    others === 0
      ? " Borrowing another project's agent is not supported."
      : ` ${others} other agent${others === 1 ? "" : "s"} here ` +
        `${others === 1 ? "has" : "have"} that role but ` +
        `${others === 1 ? "does" : "do"} not belong to this project, and ` +
        "borrowing is not supported.";
  const remedy =
    ` Install the project's roles or associate a ${role} agent on the ` +
    "project's Agents tab, then ask again.";
  if (pool.scope.kind === "unreadable") {
    return {
      code: "HIRE_NO_PROJECT_AGENT",
      reason:
        `this session's project reference "${pool.scope.raw}" is not a ` +
        `well-formed project coordinate, so no ${role} agent on this computer ` +
        `can be shown to belong to it.${borrowing}`,
    };
  }
  return {
    code: "HIRE_NO_PROJECT_AGENT",
    reason:
      `${pool.scope.label} has no ${role} agent on this computer.` +
      `${borrowing}${remedy}`,
  };
}

/** `abcd1234…wxyz·builder (execution gen-7)` — the seat, as a lead addresses it. */
function describeLiveSeat(seat: CodingSessionHireLiveSeat): string {
  const label = `${truncatePubkey(seat.actor.trim())}·${seat.role}`;
  const generation = seat.generationId?.trim();
  return generation ? `${label} (execution ${generation})` : label;
}

/**
 * The identity that takes the seat, from the project-scoped pool.
 *
 * Only an identity whose *home* role is the hired role, because D12 fixes seat
 * role to home role — a builder seated as an architect would carry the builder
 * pack and be briefed as one, and a builder seated as a verifier would review
 * its own work. An identity already live in this umbrella is skipped rather
 * than re-seated: two executions of one identity in one session are two
 * processes signing as the same agent, and nothing downstream can tell their
 * turns apart.
 *
 * Deterministic: a staged pack beats an unstaged one, then a caller's runtime
 * preference, then pubkey — so the same request answered twice picks the same
 * identity. The display name is deliberately **not** a tie-break: it is the
 * one field a person edits, and a rename must not change who is hired
 * (acceptance B). A name tie-break is what let "Bob" < "Builder" decide Tank
 * Loop's hire before selection was scoped by project.
 */
function chooseIdentity(
  pool: CodingSessionHireIdentityPool,
  input: CodingSessionHireDecisionInput,
): CodingSessionHireCandidate | null {
  const live = new Set(
    input.liveSeats.map((seat) => seat.actor.trim().toLowerCase()),
  );
  const eligible = pool.project.filter(
    (candidate) => !live.has(candidate.pubkey.trim().toLowerCase()),
  );
  eligible.sort(
    (left, right) =>
      packRank(left) - packRank(right) ||
      providerPreferenceRank(left, input) -
        providerPreferenceRank(right, input) ||
      left.pubkey
        .trim()
        .toLowerCase()
        .localeCompare(right.pubkey.trim().toLowerCase()),
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
