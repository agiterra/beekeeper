/**
 * One hire request, answered: authority, then policy, then the seat.
 *
 * This is the whole decision the founder's desktop makes when a lead asks for
 * a seat, as a pure function — so what the host will do is testable without a
 * relay, a keystore, or a provider, and so the hook that performs it holds no
 * rules of its own.
 *
 * The order is the point:
 *
 * 1. **Authority first, and silently.** A request from somebody who is neither
 *    the founder nor a granted operator is not refused with a code — it is
 *    ignored. The relay already refuses it on ingest; a host that answered it
 *    with a published refusal would be telling a stranger that this computer
 *    is listening, and would let anyone in the channel make this machine sign
 *    events on demand.
 * 2. **Policy next**, producing one of the five contract codes, which is
 *    published back to the requesting seat as a turn.
 * 3. **The seat last** — a create carrying the brief as its first turn.
 *
 * Since 2026-08-30 there is a fourth step between 2 and 3, and only for a
 * hire that asked for it: **the router**. The lead names a class and a risk,
 * never a model; this host intersects its registry with the live catalog of
 * the runtime the chosen identity runs on, picks the cheapest execution
 * target that cleared every gate, and puts the whole decision — class, tier,
 * risk, chosen, runner-up, reason, review triggers — on the seat's create.
 * A routed hire that cannot be routed is refused `HIRE_NO_ROUTE`; it is never
 * seated on a guess.
 *
 * Since 2026-09-20 an **unrouted** hire has one more input, and it is not a
 * router: the project's `team.yml` per-role `model` hint (spec § 4.2,
 * advisory by D17). Without it an unrouted hire ran the identity's own pin —
 * `opus[1m]` here, `claude-fable-5-1[1m]` on Andy's machine, seven seats deep
 * — which is the most expensive target on offer (ledger 179(b), 180). The
 * hint is honoured only when the seat's own runtime actually offers the id;
 * otherwise the pin runs, and the notice says which and why. Nothing new is
 * refused: a hint is advice, and advice a catalog cannot serve is disclosed,
 * not fatal.
 */
import {
  decideCodingSessionHire,
  formatCodingSessionHireRefusal,
  type CodingSessionHireCandidate,
  type CodingSessionHirePolicy,
  type CodingSessionHireRefusalCode,
} from "./codingSessionHirePolicy";
import {
  buildCodingSessionHireSeatPlan,
  CODING_SESSION_HIRE_ENDED_STATUSES,
  codingSessionHireSeatOrdinal,
  codingSessionHireUmbrellaProjectRef,
  isCodingSessionHireAuthorized,
  listCodingSessionHireLiveSeats,
  type CodingSessionHireAuthority,
  type CodingSessionHireSeatPlan,
  type CodingSessionHireUmbrellaLike,
} from "./codingSessionHireSeat";
import {
  resolveCodingSessionHireRouting,
  type CodingSessionRegistrySource,
} from "./codingSessionHireRouting";
import { parseModelRegistry } from "./codingSessionRouting";
import type { CodingSessionHireRequest } from "./codingSessionHireWire";

/**
 * How old a hire may be and still be answered, in seconds.
 *
 * Fifteen minutes: long enough that a host restarting, or a relay catching up
 * after a reconnect, still seats what a lead is genuinely waiting for; short
 * enough that nobody is surprised by a seat appearing for a request they made
 * before lunch. The CLI waits two minutes for an answer (`HIRE_WAIT_SECONDS`,
 * matching `CODING_SESSION_CREW_RECEIPT_TIMEOUT_MS`), so anything past this
 * window has already been reported `unconfirmed` to the lead.
 */
export const CODING_SESSION_HIRE_MAX_AGE_SECONDS = 15 * 60;

/** The exact sentence a stale hire is refused with. */
export const CODING_SESSION_HIRE_STALE_REASON =
  "this hire request is older than the host's window; hire again";

export type CodingSessionHireAnswer =
  | { kind: "ignored"; why: "unauthorized" | "unknown-umbrella" }
  | {
      kind: "refused";
      code: CodingSessionHireRefusalCode;
      reason: string;
      /** The exact 44220 text published back to the requesting seat. */
      text: string;
    }
  | { kind: "seat"; plan: CodingSessionHireSeatPlan };

export type CodingSessionHireAnswerInput = {
  request: CodingSessionHireRequest;
  /** The umbrella the hire names, or null when this host has never seen it. */
  umbrella:
    | (CodingSessionHireUmbrellaLike & {
        sessionRef: string | null;
        title: string;
        genesisRef: string | null;
        projectRef?: string | null;
      })
    | null;
  authority: CodingSessionHireAuthority;
  policy: CodingSessionHirePolicy;
  /**
   * Every managed agent this computer holds, each with its project
   * association. The decision seats only those that belong to the umbrella's
   * project (or, for a projectless umbrella, to no project).
   */
  candidates: readonly CodingSessionHireCandidate[];
  /** The umbrella project's display name for a refusal, when known. */
  projectLabel?: string | null;
  availableProviderInstanceRefs: readonly string[];
  /** Pubkey of the provider that will answer the create this host publishes. */
  providerAuthorityPubkey: string;
  /** Fresh 44221 command id for the seat's create. */
  commandId: string;
  /** Each runtime's offered model ids, by instance ref. See the decision. */
  modelCatalogs?: ReadonlyMap<string, readonly string[]>;
  /** Runtime slug by instance ref, so an identity's own runtime can match. */
  providerRuntimeSlugs?: ReadonlyMap<string, string>;
  /**
   * This host's copy of `team/model-registry.yaml`, or the reason it has
   * none. Only a hire that asks to be routed is affected by it.
   */
  registry?: CodingSessionRegistrySource;
  /**
   * The project's `team.yml` per-role hints, by role slug — read from the
   * same agents-repository snapshot as the registry.
   *
   * Absent means this host read no team manifest, which is a different fact
   * from a manifest that says nothing about the role: the first falls back to
   * the pin silently, the second says the team was read and had no opinion.
   */
  teamRoleHints?: ReadonlyMap<string, CodingSessionTeamRoleHint>;
  /** The 44222 revision the catalogs came from, when the host read one. */
  catalogRevision?: number | null;
  /** The seat this hire's seat will review, for the cross-provider rule. */
  routingPeer?: { className: string; provider: string } | null;
  /** The lead's by-rule challenger sample, marked on the routing record. */
  sampleChallenger?: boolean;
  /**
   * This host's clock, Unix seconds. Supplied so the staleness window is a
   * fact of the call rather than of when the module happened to run.
   */
  now?: number;
  /** How old a hire may be and still be answered. Seconds. */
  maxAgeSeconds?: number;
};

/** Decide, without doing anything, what this host owes one hire request. */
export function planCodingSessionHireAnswer(
  input: CodingSessionHireAnswerInput,
): CodingSessionHireAnswer {
  const { request, umbrella } = input;
  // An umbrella this host has no facts about is not one it can seat into: it
  // cannot count the live seats the ceiling is about, and it cannot read the
  // title the seat inherits. Ignored rather than refused, because "I have not
  // observed it yet" is a statement about this host's loading, not about the
  // request.
  if (
    umbrella === null ||
    umbrella.sessionRef !== request.action.sessionRef ||
    umbrella.genesisRef !== request.action.genesisRef
  ) {
    return { kind: "ignored", why: "unknown-umbrella" };
  }
  if (
    !isCodingSessionHireAuthorized(request.requesterPubkey, input.authority)
  ) {
    return { kind: "ignored", why: "unauthorized" };
  }

  // Age is checked after authority and before policy: a host that has been
  // shut for an hour comes back to a channel full of requests nobody is
  // waiting on any more, and seating them would hand a lead a team it asked
  // for in another context entirely. Refused rather than dropped, because a
  // lead that heard nothing cannot tell this host from a dead one.
  const now = input.now ?? Math.floor(Date.now() / 1000);
  const maxAge = input.maxAgeSeconds ?? CODING_SESSION_HIRE_MAX_AGE_SECONDS;
  if (now - request.createdAt > maxAge) {
    const refusal = {
      code: "HIRE_STALE" as const,
      reason: CODING_SESSION_HIRE_STALE_REASON,
    };
    return {
      kind: "refused",
      ...refusal,
      text: formatCodingSessionHireRefusal(refusal),
    };
  }

  const liveSeats = listCodingSessionHireLiveSeats(umbrella);
  // The umbrella's own project when a caller supplies one, else the one its
  // executions carry — the fold has no project field of its own. One value
  // for both the decision (who may be seated) and the create (where the seat
  // is filed), so the two can never name different projects.
  const projectRef =
    umbrella.projectRef ?? codingSessionHireUmbrellaProjectRef(umbrella);
  const registry = input.registry ?? {
    kind: "unreadable" as const,
    why: "this host was given no registry source",
  };
  const routingPeer =
    input.routingPeer === undefined
      ? inferCodingSessionHireRoutingPeer({
          request,
          umbrella,
          registry,
        })
      : input.routingPeer;
  const preferredProviderInstanceRefs = preferredRoutingProviders({
    input,
    registry,
    peer: routingPeer,
  });
  const decision = decideCodingSessionHire({
    request: {
      role: request.action.role,
      providerInstanceRef: request.action.providerInstanceRef,
      model: request.action.model,
    },
    policy: input.policy,
    projectRef,
    ...(input.projectLabel ? { projectLabel: input.projectLabel } : {}),
    candidates: input.candidates,
    liveSeats,
    availableProviderInstanceRefs: input.availableProviderInstanceRefs,
    ...(input.modelCatalogs ? { modelCatalogs: input.modelCatalogs } : {}),
    ...(input.providerRuntimeSlugs
      ? { providerRuntimeSlugs: input.providerRuntimeSlugs }
      : {}),
    ...(request.action.routing === undefined ? {} : { routed: true }),
    ...(preferredProviderInstanceRefs.length === 0
      ? {}
      : { preferredProviderInstanceRefs }),
  });
  if (!decision.ok) {
    return {
      kind: "refused",
      code: decision.code,
      reason: decision.reason,
      text: formatCodingSessionHireRefusal(decision),
    };
  }

  // The router runs after the identity and the runtime are settled, because
  // the identity decides the runtime and the runtime decides the catalog: a
  // target chosen against another vendor's offer is a target this host would
  // then have to quietly ignore.
  const routing = resolveCodingSessionHireRouting({
    request: request.action.routing,
    requestedModel: request.action.model,
    registry,
    providerInstanceRef: decision.providerInstanceRef,
    offeredModels: input.modelCatalogs?.get(decision.providerInstanceRef) ?? [],
    catalogRevision: input.catalogRevision ?? null,
    ...(routingPeer ? { peer: routingPeer } : {}),
    ...(input.sampleChallenger === true ? { sampleChallenger: true } : {}),
  });
  if (routing.kind === "refused") {
    const refusal = { code: routing.code, reason: routing.reason };
    return {
      kind: "refused",
      ...refusal,
      text: formatCodingSessionHireRefusal(refusal),
    };
  }

  // Only for an unrouted hire, and only when the lead named no model: a
  // routed decision is the router's, and a named model is the lead's (D13).
  const teamHint =
    routing.kind === "routed"
      ? { model: decision.model, notice: null }
      : resolveCodingSessionHireTeamModelHint({
          role: decision.role,
          requestedModel: request.action.model,
          pin: decision.model,
          providerInstanceRef: decision.providerInstanceRef,
          providerRuntimeSlug:
            input.providerRuntimeSlugs?.get(decision.providerInstanceRef) ??
            null,
          offeredModels:
            input.modelCatalogs?.get(decision.providerInstanceRef) ?? [],
          hints: input.teamRoleHints,
        });

  return {
    kind: "seat",
    plan: buildCodingSessionHireSeatPlan({
      commandId: input.commandId,
      channelId: request.channelId,
      sessionRef: request.action.sessionRef,
      genesisRef: request.action.genesisRef,
      projectRef,
      // Inherited, so the hired seat lands under the session's own name rather
      // than starting a second one beside it.
      title: umbrella.title.trim().length > 0 ? umbrella.title : null,
      brief: request.action.brief,
      role: decision.role,
      identity: decision.identity,
      providerInstanceRef: decision.providerInstanceRef,
      providerAuthorityPubkey: input.providerAuthorityPubkey,
      // A routed seat runs the target the router chose; an unrouted one runs
      // the team's advice for its role when the runtime offers it, and the
      // identity's own pin otherwise. None of the three is a substitution
      // nobody named: a routed seat publishes the routing record with its
      // create, and an unrouted one carries `modelNotice` — which the
      // umbrella renders — naming the source that chose.
      model:
        routing.kind === "routed"
          ? routing.record.chosen.model
          : teamHint.model,
      modelNotice: decision.modelNotice ?? teamHint.notice,
      providerNotice: decision.providerNotice,
      routing: routing.kind === "routed" ? routing.record : null,
      seatOrdinal: codingSessionHireSeatOrdinal(liveSeats, decision.role),
    }),
  };
}

/**
 * One role's advisory hints from the project's `team.yml` (spec § 4.2).
 *
 * Mirrors the host's `TeamRoleHint`. Either field may be absent, and absent
 * is not a guess: it is the team declining to have an opinion.
 */
export type CodingSessionTeamRoleHint = {
  /** `roles.<role>.runtime` — a runtime slug (`claude`, `codex`). */
  runtime: string | null;
  /** `roles.<role>.model` — `provider:model-id`. */
  model: string | null;
};

/** Which source chose an unrouted seat's model, and what to say about it. */
export type CodingSessionHireTeamModelChoice = {
  /** The model the create carries; `null` lets the runtime choose. */
  model: string | null;
  /** The sentence the umbrella shows, or null when there is nothing to say. */
  notice: string | null;
};

/**
 * The model an **unrouted** hire runs, and where it came from.
 *
 * The order, and why each step is where it is:
 *
 * 1. **A model the lead named wins.** D13 makes the model the lead's call,
 *    and `decideCodingSessionHire` has already checked it against the
 *    runtime's own catalog. Nothing here may second-guess it.
 * 2. **Then the team's advice for the role**, when the seat's runtime offers
 *    that exact id. This is the step that did not exist: an unrouted hire had
 *    only the identity's pin, so seven seats on Andy's run took
 *    `claude-fable-5-1[1m]` under an `opus[1m]` lead because that is what
 *    their identities pinned (ledger 179(b)).
 * 3. **Then the identity's own pin**, exactly as before.
 *
 * A hint is advice, so it **refuses nothing**. A hint the catalog does not
 * offer, or one addressed to another vendor, falls through to the pin and
 * says so — a hint that silently did nothing would be worse than no hint,
 * because the team would read its own `team.yml` and believe it.
 *
 * The runtime hint is deliberately *not* honoured as a provider switch. The
 * identity decides the runtime — a codex identity does not run on the Claude
 * adapter whatever anybody asked for (item 88(i), live 2026-08-28) — so when
 * `runtime` names something else this says so and changes nothing.
 */
export function resolveCodingSessionHireTeamModelHint(input: {
  role: string;
  /** The model the lead named, if any. */
  requestedModel: string | null;
  /** What the decision already settled on: the identity's pin, or null. */
  pin: string | null;
  providerInstanceRef: string;
  /** The runtime slug behind {@link providerInstanceRef}, when known. */
  providerRuntimeSlug: string | null;
  offeredModels: readonly string[];
  hints: ReadonlyMap<string, CodingSessionTeamRoleHint> | undefined;
}): CodingSessionHireTeamModelChoice {
  const pin = { model: input.pin, notice: null };
  // The lead named one, or this host read no team: nothing to add.
  if (input.requestedModel !== null && input.requestedModel.trim().length > 0) {
    return pin;
  }
  if (input.hints === undefined) return pin;
  const hint = input.hints.get(input.role);
  if (hint === undefined) return pin;

  const runtimeNote = describeTeamRuntimeHint(hint, input.providerRuntimeSlug);
  const asked = hint.model?.trim() ?? "";
  if (asked.length === 0) {
    return { model: input.pin, notice: runtimeNote };
  }
  // `provider:model-id`, per team.yml's own documentation. The vendor half is
  // checked rather than stripped: a hint written for another vendor is advice
  // about a seat this is not.
  const separator = asked.indexOf(":");
  const vendor = separator < 0 ? null : asked.slice(0, separator);
  const modelId = separator < 0 ? asked : asked.slice(separator + 1);
  if (!input.offeredModels.includes(modelId)) {
    return {
      model: input.pin,
      notice: joinNotices(
        `team.yml asks this ${input.role} to run ${asked}, which ` +
          `${input.providerInstanceRef} does not offer; ran ` +
          `${input.pin ?? "the runtime's own default"} instead`,
        runtimeNote,
      ),
    };
  }
  if (
    vendor !== null &&
    input.providerRuntimeSlug !== null &&
    vendor !== input.providerRuntimeSlug &&
    !input.providerInstanceRef.startsWith(`${vendor}-`)
  ) {
    return {
      model: input.pin,
      notice: joinNotices(
        `team.yml asks this ${input.role} to run ${asked}, which names ` +
          `${vendor} and this seat runs on ${input.providerInstanceRef}; ran ` +
          `${input.pin ?? "the runtime's own default"} instead`,
        runtimeNote,
      ),
    };
  }
  if (modelId === input.pin) {
    // The team and the identity agree. Say nothing about the model — there is
    // nothing a person would act on — but keep any runtime note.
    return { model: input.pin, notice: runtimeNote };
  }
  return {
    model: modelId,
    notice: joinNotices(
      `ran ${modelId} because the project's team.yml names it for the ` +
        `${input.role} role, not this identity's pin ` +
        `(${input.pin ?? "none"})`,
      runtimeNote,
    ),
  };
}

/** One clause when `team.yml`'s runtime hint is not the runtime being used. */
function describeTeamRuntimeHint(
  hint: CodingSessionTeamRoleHint,
  providerRuntimeSlug: string | null,
): string | null {
  const asked = hint.runtime?.trim() ?? "";
  if (asked.length === 0 || providerRuntimeSlug === null) return null;
  if (asked === providerRuntimeSlug) return null;
  return (
    `team.yml prefers the ${asked} runtime for this role, but the ` +
    `identity runs on ${providerRuntimeSlug} and the identity decides the ` +
    "runtime"
  );
}

/** Both sentences when there are two, one when there is one, else null. */
function joinNotices(first: string, second: string | null): string {
  return second === null ? first : `${first}; ${second}`;
}

function inferCodingSessionHireRoutingPeer(input: {
  request: CodingSessionHireRequest;
  umbrella: CodingSessionHireUmbrellaLike;
  registry: CodingSessionRegistrySource;
}): { className: string; provider: string } | null {
  const routing = input.request.action.routing;
  if (routing === undefined || input.registry.kind !== "readable") return null;
  const parsed = parseModelRegistry(input.registry.text);
  if (!parsed.ok) return null;
  const peerClass = parsed.registry.classes[routing.class]?.crossProviderOf;
  if (peerClass === undefined) return null;

  const providers = new Set<string>();
  for (const execution of input.umbrella.executions) {
    const generation = execution.activeGeneration;
    if (
      generation.role !== peerClass ||
      CODING_SESSION_HIRE_ENDED_STATUSES.includes(generation.status)
    ) {
      continue;
    }
    const provider = generation.provider?.trim();
    if (provider) providers.add(provider);
  }
  // One role spanning multiple vendors does not identify which seat this
  // verifier reviews. The request needs an explicit peer in that case; never
  // pick whichever execution happened to fold first.
  if (providers.size !== 1) return null;
  const provider = providers.values().next().value;
  return provider === undefined ? null : { className: peerClass, provider };
}

function preferredRoutingProviders(input: {
  input: CodingSessionHireAnswerInput;
  registry: CodingSessionRegistrySource;
  peer: { className: string; provider: string } | null;
}): string[] {
  const request = input.input.request.action;
  if (request.routing === undefined || input.peer === null) return [];
  const allowed = input.input.policy.allowedProviderInstanceRefs;
  return input.input.availableProviderInstanceRefs.filter((provider) => {
    if (allowed !== null && !allowed.includes(provider)) return false;
    if (
      providerVendor(provider) === providerVendor(input.peer?.provider ?? "")
    ) {
      return false;
    }
    return (
      resolveCodingSessionHireRouting({
        request: request.routing,
        requestedModel: request.model,
        registry: input.registry,
        providerInstanceRef: provider,
        offeredModels: input.input.modelCatalogs?.get(provider) ?? [],
        catalogRevision: input.input.catalogRevision ?? null,
        peer: input.peer,
        ...(input.input.sampleChallenger === true
          ? { sampleChallenger: true }
          : {}),
      }).kind === "routed"
    );
  });
}

function providerVendor(providerInstanceRef: string): string {
  const separator = providerInstanceRef.lastIndexOf("-");
  return separator <= 0
    ? providerInstanceRef
    : providerInstanceRef.slice(0, separator);
}

/**
 * The umbrella's line for a hire this host answered with somebody else's
 * model.
 *
 * Same reasoning as the refusal notice below: a substitution recorded only in
 * the seat's create is one nobody reads. The lead asked for a model, the host
 * ran a different one, and both the lead and the person get told in the
 * timeline where the work is.
 */
export function codingSessionHireModelNoticeLine(input: {
  role: string;
  notice: string;
}): string {
  return codingSessionHireNoticeLine(input);
}

/**
 * The umbrella's line for anything the host decided on the lead's behalf.
 *
 * One shape for every substitution — model or runtime — so a person reading
 * the timeline sees them in the same voice and a second kind of disclosure
 * never has to invent a second format.
 */
export function codingSessionHireNoticeLine(input: {
  role: string;
  notice: string;
}): string {
  return `Hired a ${input.role} — ${input.notice}`;
}

/**
 * The line the umbrella shows for a refusal.
 *
 * Published as an ordinary session-lane message from the operator, so it lands
 * in the umbrella's own timeline where the work is, not only in the private
 * turn the requesting seat receives. A refusal only the refused agent can see
 * is a refusal the person never learns about — and the person is the one who
 * set the policy.
 */
export function codingSessionHireRefusalNotice(input: {
  role: string;
  requesterLabel: string;
  text: string;
}): string {
  return `${input.requesterLabel} asked to hire a ${input.role} — ${input.text}`;
}

/**
 * The sentence a seat that was created but never granted is reported with.
 *
 * A hired agent that holds no `grant-operator` is not a degraded seat, it is a
 * mute one: the relay refuses its `sessions send` with "only a session founder
 * or a granted operator may steer", so it can do the whole job and never
 * deliver a word of it. That happened on 2026-08-28 — a builder worked for
 * 1,009 s, committed, and its report bounced (item 83). The lead has to be
 * told in the same breath as "seated", because "seated" alone reads as a seat
 * it can expect an answer from.
 */
export function codingSessionHireGrantFailureText(reason: string): string {
  const said = reason.trim();
  return `seated, but not granted: ${
    said.length > 0 ? said : "the grant did not go out"
  } — it cannot report until granted`;
}

/** The umbrella's line for the same failure, so the person sees it too. */
export function codingSessionHireGrantFailureNotice(input: {
  role: string;
  text: string;
}): string {
  return `Hired a ${input.role} — ${input.text}`;
}

/**
 * What this host did with one hire.
 *
 * Declared here rather than beside the hook so the tally below and the hook
 * cannot drift into two different vocabularies for the same five facts.
 *
 * - `seated` — an identity was created, granted and is running.
 * - `refused` — a contract code went back to the requesting seat.
 * - `malformed` — the payload could not be read; the failing key went back.
 * - `ignored` — not this operator's umbrella, or no provider identity here.
 *   Deliberately quiet on the wire: answering a stranger would tell them this
 *   computer is listening and will sign events on request.
 * - `error` — the answer itself failed to go out. Not a refusal: nobody was
 *   told, which is why it is counted separately and never folded into one.
 */
export type CodingSessionHireOutcomeState =
  | "seated"
  | "refused"
  | "malformed"
  | "ignored"
  | "error";

/** The three counts the umbrella strip shows, plus what to say on hover. */
export type CodingSessionHireTally = {
  /** Hires that produced a seat. */
  answered: number;
  /** Hires refused with a contract code. */
  refused: number;
  /** Hires whose payload this host could not read. */
  malformed: number;
  /** Hires whose *answer* failed to go out. Never folded into `refused`. */
  failed: number;
  /** The newest reason among them, for the hover. Null when there is none. */
  lastReason: string | null;
};

/**
 * Count what the host has answered, newest outcome last.
 *
 * `ignored` is counted by nothing on purpose: a hire into somebody else's
 * umbrella is not this host's to answer, and putting it in a "refused" count
 * would make an operator hunt for a refusal that was never owed.
 */
export function summarizeCodingSessionHireOutcomes(
  outcomes: readonly {
    state: CodingSessionHireOutcomeState;
    detail: string | null;
  }[],
): CodingSessionHireTally {
  const tally: CodingSessionHireTally = {
    answered: 0,
    refused: 0,
    malformed: 0,
    failed: 0,
    lastReason: null,
  };
  for (const outcome of outcomes) {
    if (outcome.state === "seated") tally.answered += 1;
    else if (outcome.state === "refused") tally.refused += 1;
    else if (outcome.state === "malformed") tally.malformed += 1;
    else if (outcome.state === "error") tally.failed += 1;
    else continue;
    const said = outcome.detail?.trim();
    if (said !== undefined && said.length > 0) tally.lastReason = said;
  }
  return tally;
}

/**
 * The umbrella strip's one hire line, or null when there is nothing to say.
 *
 * `failed` is appended only when it is non-zero, because a count of zero for a
 * thing that has never happened is noise — but a non-zero one may never be
 * hidden: an answer that did not go out is a lead still waiting.
 */
export function formatCodingSessionHireTally(
  tally: CodingSessionHireTally,
): string | null {
  if (
    tally.answered === 0 &&
    tally.refused === 0 &&
    tally.malformed === 0 &&
    tally.failed === 0
  ) {
    return null;
  }
  const line =
    `hires: ${tally.answered} answered · ${tally.refused} refused · ` +
    `${tally.malformed} malformed`;
  return tally.failed === 0
    ? line
    : `${line} · ${tally.failed} failed to answer`;
}
