import * as React from "react";

import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import { stageCodingSessionCreateHint } from "@/shared/api/tauriCodingSessionWorkdirs";
import { createCodingSessionWorktree } from "@/shared/api/tauriCodingSessionWorktrees";
import type { CodingSessionProviderRuntime } from "@/shared/api/tauriSessionProvider";
import type { RelayEvent } from "@/shared/api/types";
import { ensureActorChannelMembership } from "../lib/actorSeatChannelMembership";
import {
  clearCodingSessionActorSeat,
  type CodingSessionSeatPackRef,
  stageCodingSessionActorSeat,
} from "../lib/codingSessionActorSeatCustody";
import { fetchCodingSessionSeatPackSource } from "../lib/codingSessionSeatPackSource";
import {
  createCodingSessionCommandId,
  type CodingSessionCommandTarget,
} from "../lib/codingSessionCommand";
import { awaitCodingSessionCreateReceipt } from "../lib/codingSessionCrewReceipt";
import {
  codingSessionHireGrantFailureNotice,
  codingSessionHireGrantFailureText,
  codingSessionHireNoticeLine,
  codingSessionHireRefusalNotice,
  planCodingSessionHireAnswer,
  type CodingSessionHireOutcomeState,
} from "../lib/codingSessionHireAnswer";
import {
  formatCodingSessionHireRefusal,
  readCodingSessionHirePolicy,
  type CodingSessionHirePolicy,
} from "../lib/codingSessionHirePolicy";
import type { CodingSessionRegistrySource } from "../lib/codingSessionHireRouting";
import {
  codingSessionHireRequesterLabel,
  codingSessionHireUmbrellaProjectRef,
  isCodingSessionHireAuthorized,
  selectUnansweredCodingSessionHires,
} from "../lib/codingSessionHireSeat";
import type { CodingSessionHireCatalogSource } from "../lib/codingSessionHireCatalog";
import {
  codingSessionHireCandidatesOf,
  type CodingSessionHireAgent,
} from "../lib/codingSessionHireCandidates";
import {
  classifyCodingSessionHireEvent,
  codingSessionHireRequesterStanding,
  type CodingSessionHireClassification,
  type CodingSessionHireRequest,
} from "../lib/codingSessionHireWire";
import { publishCodingSessionLaneMessage } from "../lib/codingSessionLanePublish";
import {
  buildCodingSessionCreateEvent,
  createCodingSessionLifecycleCommandId,
} from "../lib/codingSessionLifecycleCommand";
import {
  publishHireOutcomes,
  readCodingSessionHireOutcomes,
  resetCodingSessionHireOutcomes,
} from "../lib/codingSessionHireOutcomeStore";
import { grantSeat } from "../lib/codingSessionHireGrant";
import {
  discloseCodingSessionHire,
  outcomeOf,
  publishRefusal,
} from "../lib/codingSessionHireDisclosure";
import {
  codingSessionHireCheckoutLine,
  type CodingSessionHireCheckoutResolution,
  type CodingSessionHireCheckoutSource,
} from "../lib/codingSessionHireCheckout";
import {
  armSeatWorktreeForSharing,
  type CodingSessionHireWipShare,
} from "../lib/codingSessionWorktreeSource";
import { subscribeToObservedCodingSessionEvents } from "../lib/codingSessionObservedEvents";
import { fetchCodingSessionRosterFold } from "../lib/codingSessionRoster";
import { ensureCodingSessionOperatorGrant } from "../lib/codingSessionOperatorGrant";
import {
  publishSeatedCodingSessionCreate,
  type SeatedCodingSessionCreateDeps,
} from "../lib/codingSessionSeatedCreate";
import type { CodingSessionUmbrellaRecord } from "../lib/codingSessionTypes";

/**
 * The founder's desktop honouring `session.hire`.
 *
 * A lead asks for a seat over the relay; this is the thing that answers. It
 * reads the same 44221 stream the create observations already subscribe to —
 * through {@link subscribeToObservedCodingSessionEvents}, deliberately, so a
 * second relay connection is never opened for it — applies the operator's
 * standing policy, and either seats the identity or publishes a refusal the
 * lead and the person can both read.
 *
 * Three properties worth stating, because each is a way this could quietly go
 * wrong:
 *
 * 1. **Every hire gets exactly one answer.** Requests are deduped by
 *    `commandId` and marked answered before any effect runs, so a reconnect —
 *    which replays history through the same bus — cannot seat the same agent
 *    twice, on a second worktree, in a second process.
 * 2. **A hire this host may not honour is ignored, not refused.** Authority is
 *    the umbrella's founder or a live grant. Publishing a refusal to a
 *    stranger would let anyone in the channel make this computer sign events.
 * 3. **A refusal is said out loud, twice.** As a 44220 turn to the requesting
 *    seat, so the lead can act on it, and as a session-lane message in the
 *    umbrella, so the person who set the policy sees it enforced. A refusal
 *    only the refused agent can see is a silent drop as far as the person is
 *    concerned.
 * 4. **A seated agent is granted, or the failure to grant is said out loud.**
 *    Seating alone produces a mute seat: the relay refuses an ungranted
 *    actor's `sessions send` with "only a session founder or a granted
 *    operator may steer", so the agent does the work and its report bounces.
 *    That is exactly what happened on 2026-08-28 (item 83), because this hook
 *    seated and stopped while `codingSessionCrewLaunch` granted. The order is
 *    the launcher's: the create's receipt first, then `grant-operator` for the
 *    seat's own actor.
 *
 * Everything it touches outside itself — the event bus, the worktree, custody,
 * the keystore, the relay, the clock — arrives through {@link
 * CodingSessionHireDeps}, so the sequence this hook performs is a thing a test
 * can watch rather than a thing a comment claims.
 */
export type CodingSessionHireOutcome = {
  commandId: string;
  channelId: string;
  sessionRef: string;
  role: string;
  /**
   * What the host did. See {@link CodingSessionHireOutcomeState}.
   *
   * `malformed` is its own state, not a flavour of `refused`: the payload
   * could not be read, so the refusal names a key rather than a policy, and an
   * operator counting refusals must not have unreadable payloads folded in.
   */
  state: CodingSessionHireOutcomeState;
  /**
   * Refusal code, or the failure's own words. Null for a hire that was seated
   * *and* granted — a seated hire whose grant failed carries the grant's
   * reason here rather than reading as an unqualified success.
   */
  detail: string | null;
  /** The create's commandId, which is the hire's receipt key. */
  seatCommandId: string | null;
  /**
   * Whether this host published `grant-operator` for the seat's actor.
   *
   * False on every non-seated outcome, and on a seated one whose receipt or
   * grant failed. A false here means the agent cannot report to its lead.
   */
  granted: boolean;
  /**
   * The actor this host seated, when it seated one.
   *
   * The join a transcript needs: an execution carries `agentRef`, and this is
   * the same key, so the seat's first turn can be attributed to the lead that
   * asked for it (REVIEW-B3 F3).
   */
  seatActor: string | null;
  /**
   * What to call the seat that asked, already compared with the hire's own
   * signer — `attributed` earns the name, `disputed` says so in the label.
   * Null when this outcome seated nobody.
   */
  requesterLabel: string | null;
  /**
   * Whether the seat this host cut will share its commits, and why not when it
   * will not (REVIEW-L9 F2). Absent on outcomes that seated nobody.
   */
  wipShare?: CodingSessionHireWipShare;
  /**
   * The repository commit the seat's role pack was staged from, when one
   * vouched for it (finding 84) — `30617:<owner>:<id>` at a 40-hex sha, or
   * the shipped-defaults marker. `null` when staging named no repository (a
   * pack this computer alone vouches for, a packless seat, or an older
   * backend). Absent on outcomes that seated nobody.
   */
  packRef?: CodingSessionSeatPackRef | null;
  /**
   * The checkout this host cut the seat's worktree from, and which record
   * answered — `project` for the project's own recorded folder, `channel` for
   * a projectless session's remembered one. Absent on outcomes that seated
   * nobody. Never a guess: a hire with nothing recorded is refused
   * `HIRE_CHECKOUT_NOT_RECORDED` rather than seated without a tree.
   */
  checkout?: { path: string; source: CodingSessionHireCheckoutSource };
  /**
   * The operator whose key signed the seated create — this computer's own.
   *
   * Carried so a renderer can *require* a prompt's `operatorPubkey` to equal
   * it before attributing the turn to the hiring lead, rather than overriding
   * the signer on the strength of anything else (REVIEW-B3 N1).
   */
  hostPubkey: string | null;
};

export type { CodingSessionHireAgent } from "../lib/codingSessionHireCandidates";

type HirePublisher = {
  publishEvent: (
    event: RelayEvent,
    timeoutMessage: string,
    sendErrorMessage: string,
  ) => Promise<RelayEvent>;
};

/** Everything outside this hook that answering a hire has to reach. */
export type CodingSessionHireDeps = {
  /** The observed-44221 fan-out bus. Never a second relay connection. */
  subscribe: (listener: (events: readonly RelayEvent[]) => void) => () => void;
  fetchRosterFold: typeof fetchCodingSessionRosterFold;
  createWorktree: typeof createCodingSessionWorktree;
  stageCreateHint: typeof stageCodingSessionCreateHint;
  /** The three custody/membership steps `publishSeatedCodingSessionCreate` runs. */
  seatDeps: SeatedCodingSessionCreateDeps;
  signer: typeof signRelayEvent;
  publisher: HirePublisher;
  /**
   * Wait for the provider's 44224 receipt for the seat this host published.
   *
   * The same gate a team launch uses, with the same timeout: the grant below
   * is not published until the seat is confirmed, because granting authority
   * to an identity whose create the provider refused would put a live grant
   * on a seat that does not exist.
   */
  awaitSeatReceipt: (input: {
    channelId: string;
    commandId: string;
    providerAuthorityPubkey: string;
  }) => Promise<CodingSessionCommandTarget>;
  /** Ensure a receipt-backed `grant-operator` for a provider or seat actor. */
  ensureOperatorGrant: (input: {
    channelId: string;
    genesisRef: string;
    granteePubkey: string;
  }) => Promise<void>;
  newSeatCommandId: () => string;
  newTurnCommandId: () => string;
  /** This host's clock, Unix seconds. The staleness window is read from it. */
  now: () => number;
  /**
   * Wait between grant attempts. Optional: a caller that omits it gets real
   * time, and a test that supplies one records the delays instead of serving
   * them.
   */
  sleep?: (milliseconds: number) => Promise<void>;
  /**
   * Milliseconds, for the grant retry's wall-clock budget. Optional, and
   * separate from {@link CodingSessionHireDeps.now} (which is Unix seconds):
   * a test that fakes {@link CodingSessionHireDeps.sleep} fakes this too, or
   * the budget it is trying to exercise never binds.
   */
  monotonicNow?: () => number;
};

/** The real thing: this computer's bus, disk, keystore, relay and clock. */
export const DEFAULT_CODING_SESSION_HIRE_DEPS: CodingSessionHireDeps = {
  subscribe: subscribeToObservedCodingSessionEvents,
  fetchRosterFold: fetchCodingSessionRosterFold,
  createWorktree: createCodingSessionWorktree,
  stageCreateHint: stageCodingSessionCreateHint,
  seatDeps: {
    ensureMembership: ensureActorChannelMembership,
    stageSeat: stageCodingSessionActorSeat,
    clearSeat: clearCodingSessionActorSeat,
    // The umbrella's project 30624, read the way the launch dialog's preview
    // reads it, so a hired seat is staged from the project's repository.
    fetchPackSource: fetchCodingSessionSeatPackSource,
  },
  signer: signRelayEvent,
  publisher: relayClient,
  awaitSeatReceipt: (input) => awaitCodingSessionCreateReceipt(input),
  ensureOperatorGrant: async (input) => {
    await ensureCodingSessionOperatorGrant(input);
  },
  newSeatCommandId: createCodingSessionLifecycleCommandId,
  newTurnCommandId: createCodingSessionCommandId,
  now: () => Math.floor(Date.now() / 1000),
  sleep: (milliseconds) =>
    new Promise((resolve) => {
      globalThis.setTimeout(resolve, milliseconds);
    }),
};

export type UseCodingSessionHireInput = {
  /** Session channels this operator is reading. */
  channelIds: readonly string[];
  /** This operator's public key. Hires are honoured only in their umbrellas. */
  operatorPubkey: string | null;
  /** Umbrellas observed in those channels, with founder and executions. */
  umbrellas: readonly CodingSessionUmbrellaRecord[];
  /** Every managed agent this computer holds, as seating candidates. */
  agents: readonly CodingSessionHireAgent[];
  /** The provider identity that will sign this host's seats, when there is one. */
  providerAuthorityPubkey: string | null;
  /**
   * This computer's runtimes: which exist, which are signed in, and which
   * runtime slug each instance ref is.
   *
   * **Not** a model catalog. `list_runtimes()` hardcodes every row's
   * `allowedModels` to `["default"]`
   * (`desktop/src-tauri/src/session_provider/runtimes.rs:269`), so reading a
   * catalog off this table is reading a placeholder — which is exactly how a
   * hire for `sonnet` was refused with "It offers default" while the
   * runtime's published catalog held five ids (item 88(a), live 2026-08-28).
   * The catalogs arrive separately, in {@link modelCatalogs}.
   */
  runtimes: readonly CodingSessionProviderRuntime[];
  /**
   * What each runtime actually offers, by instance ref — the
   * provider-signed kind:44222 catalog for this hire's channel.
   *
   * A ref with no entry is a catalog this host never read, and refuses
   * nothing: a refusal built on a list nobody loaded would be a claim about
   * the model dressed up as a claim about the catalog.
   */
  modelCatalogs?: ReadonlyMap<string, readonly string[]>;
  /**
   * Resolve the signed 44222 source for the request's own channel and project.
   * The host uses this instead of one app-global catalog so the model list and
   * recorded revision are one reproducible fact.
   */
  catalogForHire?: (input: {
    channelId: string;
    projectRef: string | null;
  }) => CodingSessionHireCatalogSource | null;
  /**
   * This host's copy of the shared model registry (`team/model-registry.yaml`),
   * or the reason it has none.
   *
   * The real host uses {@link registryForProject}; this scalar remains the
   * pure runner's test seam. Neither path ever routes against a copy compiled
   * into the app.
   */
  registry?: CodingSessionRegistrySource;
  /** Read the registry from the umbrella's project checkout for each hire. */
  registryForProject?: (
    projectRef: string | null,
  ) => Promise<CodingSessionRegistrySource>;
  /** The 44222 revision behind {@link modelCatalogs}, when one was read. */
  catalogRevision?: number | null;
  /**
   * Where this hire's worktree is cut from — the project's recorded checkout,
   * this session's remembered folder, or a refusal. Host-local.
   *
   * Takes the umbrella's project, not only the channel. Reading the channel
   * alone (and falling back to the most recently used directory) is what cut
   * two Tank Loop seats from the Beekeeper repository on 2026-09-16; the rule
   * now lives in {@link resolveCodingSessionHireCheckout} and is never `mru`.
   */
  checkoutForHire: (input: {
    channelId: string;
    projectRef: string | null;
  }) => CodingSessionHireCheckoutResolution;
  /** Targets to answer a requesting seat's refusal turn to, by pubkey. */
  targetForActor: (
    channelId: string,
    actorPubkey: string,
  ) => CodingSessionCommandTarget | null;
  /** The standing policy. Defaults to the one stored on this device. */
  policy?: CodingSessionHirePolicy;
  /** Off by default in tests and pop-outs; the founder's shell turns it on. */
  enabled?: boolean;
  /** Injected in tests; the real bus, disk, keystore, relay and clock by default. */
  deps?: CodingSessionHireDeps;
};

export {
  publishCodingSessionHireOutcomes,
  readCodingSessionHireOutcomes,
  resetCodingSessionHireOutcomes,
  subscribeToCodingSessionHireOutcomes,
  useCodingSessionHireOutcomes,
} from "../lib/codingSessionHireOutcomeStore";

/** Watch for hires and honour them. Returns what it has answered, newest last. */
export function useCodingSessionHire(input: UseCodingSessionHireInput): {
  outcomes: CodingSessionHireOutcome[];
  policy: CodingSessionHirePolicy;
} {
  const [outcomes, setOutcomes] = React.useState<CodingSessionHireOutcome[]>(
    [],
  );
  const policy = input.policy ?? readCodingSessionHirePolicy();
  const deps = input.deps ?? DEFAULT_CODING_SESSION_HIRE_DEPS;

  // Read through a ref so the subscription stays mounted while the catalog,
  // the agent list and the policy all keep changing under it. A resubscribe
  // per keystroke would drop hires arriving in the gap.
  const latest = React.useRef({ input, policy, deps });
  latest.current = { input, policy, deps };

  const answered = React.useRef(new Set<string>());
  const enabled = input.enabled ?? true;

  React.useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    resetCodingSessionHireOutcomes();
    const record = (outcome: CodingSessionHireOutcome) => {
      if (cancelled) return;
      setOutcomes((previous) => [...previous, outcome]);
      // Published as well as held, so the umbrella strip can show what this
      // host has answered without being handed the hook's state. An outcome
      // nobody can see is the silence this whole path exists to end.
      publishHireOutcomes([...readCodingSessionHireOutcomes(), outcome]);
    };

    const receive = (events: readonly RelayEvent[]) => {
      const current = latest.current;
      const allowed = new Set(current.input.channelIds);
      if (allowed.size === 0) return;
      const requests: CodingSessionHireRequest[] = [];
      for (const event of events) {
        const classified = classifyCodingSessionHireEvent(event, allowed);
        if (classified.kind === "hire") {
          requests.push(classified);
          continue;
        }
        // A hire this host cannot read is still a hire somebody is waiting on.
        // `irrelevant` is not one of them — that is another action on the same
        // kind, and counting a create as a malformed hire would make every
        // ordinary session look like an attack on this store.
        if (classified.kind !== "malformed") continue;
        const key =
          classified.address === null
            ? `malformed:${event.id}`
            : `malformed:${classified.address.channelId}:${classified.address.commandId}`;
        if (answered.current.has(key)) continue;
        answered.current.add(key);
        // Logged before anything that can fail, and always — the 44220 below
        // needs authority and a target, and neither is a reason for this
        // machine to have no record of what it threw away.
        console.warn(
          `[coding-sessions] hire refused: HIRE_MALFORMED — ${classified.failingKey}: ${classified.reason}`,
        );
        void refuseMalformed(classified, record).catch((error: unknown) => {
          record({
            commandId: classified.address?.commandId ?? event.id,
            channelId: classified.address?.channelId ?? "",
            sessionRef: classified.address?.sessionRef ?? "",
            role: classified.address?.role ?? "",
            state: "error",
            detail: error instanceof Error ? error.message : String(error),
            seatCommandId: null,
            granted: false,
            seatActor: null,
            requesterLabel: null,
            hostPubkey: null,
          });
        });
      }
      // Newest first: a host that has been shut sees a whole backlog at once,
      // and the seat ceiling is finite.
      for (const request of selectUnansweredCodingSessionHires(
        requests,
        answered.current,
      )) {
        // Marked before the work, not after: an effect that throws must not
        // leave a hire eligible to be seated a second time on the next replay.
        answered.current.add(request.commandId);
        void honour(request, record).catch((error: unknown) => {
          record({
            commandId: request.commandId,
            channelId: request.channelId,
            sessionRef: request.action.sessionRef,
            role: request.action.role,
            state: "error",
            detail: error instanceof Error ? error.message : String(error),
            seatCommandId: null,
            granted: false,
            seatActor: null,
            requesterLabel: null,
            hostPubkey: null,
          });
        });
      }
    };

    const unsubscribe = latest.current.deps.subscribe(receive);
    return () => {
      cancelled = true;
      unsubscribe();
      resetCodingSessionHireOutcomes();
    };

    /**
     * Answer a hire whose shape this host could not read.
     *
     * Recorded and logged unconditionally; *published* only into an umbrella
     * this operator founded, and only to a requester that umbrella already
     * trusts. The authority rule is the same one `planCodingSessionHireAnswer`
     * applies and it is not weakened here: a malformed payload from a stranger
     * must not be able to make this computer sign an event, or even confirm
     * that it is listening.
     */
    async function refuseMalformed(
      classified: Extract<
        CodingSessionHireClassification,
        { kind: "malformed" }
      >,
      report: (outcome: CodingSessionHireOutcome) => void,
    ): Promise<void> {
      const current = latest.current;
      const address = classified.address;
      const reason = `${classified.failingKey}: ${classified.reason}`;
      if (address === null) {
        report({
          commandId: "",
          channelId: "",
          sessionRef: "",
          role: "",
          state: "malformed",
          detail: `${reason} — and the envelope named no channel, command id or signer, so there was nobody to tell`,
          seatCommandId: null,
          granted: false,
          seatActor: null,
          requesterLabel: null,
          hostPubkey: null,
        });
        return;
      }
      const outcome: CodingSessionHireOutcome = {
        commandId: address.commandId,
        channelId: address.channelId,
        sessionRef: address.sessionRef ?? "",
        role: address.role ?? "",
        state: "malformed",
        detail: reason,
        seatCommandId: null,
        granted: false,
        seatActor: null,
        requesterLabel: null,
        hostPubkey: null,
      };
      const operator = current.input.operatorPubkey;
      const umbrella =
        address.sessionRef === null
          ? null
          : (current.input.umbrellas.find(
              (entry) => entry.sessionRef === address.sessionRef,
            ) ?? null);
      if (
        operator === null ||
        umbrella === null ||
        umbrella.founderPubkey !== operator
      ) {
        report(outcome);
        return;
      }
      const fold = umbrella.genesisRef
        ? await current.deps.fetchRosterFold(
            address.channelId,
            umbrella.genesisRef,
          )
        : null;
      const grantedOperators = fold
        ? [...fold.accepted.entries()]
            .filter(([, role]) => role === "operator")
            .map(([pubkey]) => pubkey)
        : [];
      if (
        !isCodingSessionHireAuthorized(address.requesterPubkey, {
          founderPubkey: umbrella.founderPubkey,
          grantedOperators,
        })
      ) {
        report(outcome);
        return;
      }
      const text = formatCodingSessionHireRefusal({
        code: "HIRE_MALFORMED",
        reason,
      });
      await discloseCodingSessionHire(
        {
          channelId: address.channelId,
          sessionRef: address.sessionRef ?? "",
          requesterPubkey: address.requesterPubkey,
          text,
          notice: codingSessionHireRefusalNotice({
            role: address.role ?? "seat",
            // A malformed hire's action never parsed, so there is no
            // `requestedBy` to compare — only the signer, which is a fact.
            // "unclaimed" is the honest standing here, and it is not the same
            // as a hire that named nobody on purpose.
            requesterLabel: codingSessionHireRequesterLabel({
              standing: {
                kind: "unclaimed",
                requesterPubkey: address.requesterPubkey,
              },
              nameFor: (pubkey) =>
                current.input.agents.find((agent) => agent.pubkey === pubkey)
                  ?.name ?? null,
            }),
            text,
          }),
        },
        current.input,
        current.deps,
      );
      report(outcome);
    }

    async function honour(
      request: CodingSessionHireRequest,
      report: (outcome: CodingSessionHireOutcome) => void,
    ): Promise<void> {
      const current = latest.current;
      const hireDeps = current.deps;
      const operator = current.input.operatorPubkey;
      const umbrella =
        current.input.umbrellas.find(
          (entry) => entry.sessionRef === request.action.sessionRef,
        ) ?? null;
      // Only umbrellas this operator founded. A hire into somebody else's
      // session is theirs to answer on their own computer.
      if (
        operator === null ||
        umbrella === null ||
        umbrella.founderPubkey !== operator
      ) {
        report(outcomeOf(request, "ignored", "not this operator's session"));
        return;
      }
      const providerAuthorityPubkey = current.input.providerAuthorityPubkey;
      if (providerAuthorityPubkey === null) {
        report(
          outcomeOf(
            request,
            "ignored",
            "no provider identity on this computer",
          ),
        );
        return;
      }
      const fold = umbrella.genesisRef
        ? await hireDeps.fetchRosterFold(request.channelId, umbrella.genesisRef)
        : null;
      const grantedOperators = fold
        ? [...fold.accepted.entries()]
            .filter(([, role]) => role === "operator")
            .map(([pubkey]) => pubkey)
        : [];
      const projectRef = codingSessionHireUmbrellaProjectRef(umbrella);
      const catalogSource = current.input.catalogForHire?.({
        channelId: request.channelId,
        projectRef,
      });
      const modelCatalogs =
        catalogSource?.modelCatalogs ??
        current.input.modelCatalogs ??
        new Map();
      const registry = current.input.registryForProject
        ? await current.input.registryForProject(projectRef)
        : (current.input.registry ?? {
            kind: "unreadable" as const,
            why: "this host was given no registry source for the umbrella's project",
          });

      const answer = planCodingSessionHireAnswer({
        request,
        umbrella,
        authority: {
          founderPubkey: umbrella.founderPubkey,
          grantedOperators,
        },
        policy: current.policy,
        // Each with its project: the decision seats only the umbrella's own.
        candidates: codingSessionHireCandidatesOf(current.input.agents),
        availableProviderInstanceRefs: current.input.runtimes
          .filter((runtime) => runtime.authState === "ready")
          .map((runtime) => runtime.instanceRef),
        // The provider's own catalogs, never the runtime table's placeholder
        // list. An instance ref missing from this map is a catalog nobody
        // read, and refuses nothing.
        modelCatalogs,
        // Which runtime slug each instance ref is, so an identity that runs
        // on codex is seated on codex.
        providerRuntimeSlugs: new Map(
          current.input.runtimes.map((runtime) => [
            runtime.instanceRef,
            runtime.runtime,
          ]),
        ),
        providerAuthorityPubkey,
        registry,
        catalogRevision:
          catalogSource?.catalogRevision ??
          current.input.catalogRevision ??
          null,
        commandId: hireDeps.newSeatCommandId(),
        now: hireDeps.now(),
      });

      if (answer.kind === "ignored") {
        report(outcomeOf(request, "ignored", answer.why));
        return;
      }
      if (answer.kind === "refused") {
        await publishRefusal(request, answer, current.input, hireDeps);
        report(outcomeOf(request, "refused", answer.code));
        return;
      }

      const plan = answer.plan;
      // Which repository this seat works in, decided before anything is
      // signed. A hire with nothing recorded is refused here rather than
      // seated with no tree: an agent that looks hired and has nowhere to
      // build is told to work and cannot, and the fallback that used to stand
      // in for this — the most recently used directory — cut two seats from
      // the wrong repository on 2026-09-16 (ledger 135(a)).
      const resolved = current.input.checkoutForHire({
        channelId: request.channelId,
        projectRef,
      });
      if (resolved.kind === "unrecorded") {
        // The same `hire refused: <CODE> — <reason>` shape every other
        // refusal takes, so `bee sessions hire` recognizes it structurally
        // and prints the whole reason — including its remedy — even though
        // the code is not yet in buzz-core's list.
        const text = formatCodingSessionHireRefusal({
          code: resolved.code,
          reason: resolved.reason,
        });
        await discloseCodingSessionHire(
          {
            channelId: request.channelId,
            sessionRef: request.action.sessionRef,
            requesterPubkey: request.requesterPubkey,
            text,
            notice: codingSessionHireRefusalNotice({
              role: request.action.role,
              requesterLabel: codingSessionHireRequesterLabel({
                standing: codingSessionHireRequesterStanding(request),
                nameFor: (pubkey) =>
                  current.input.agents.find((agent) => agent.pubkey === pubkey)
                    ?.name ?? null,
              }),
              text,
            }),
          },
          current.input,
          hireDeps,
        );
        report(outcomeOf(request, "refused", resolved.code));
        return;
      }
      const checkout = resolved.path;
      // The tree exists before the create is signed, because its path *is* the
      // working directory the create names. A seat is never signed against a
      // directory that does not exist yet — and never against the operator's
      // own checkout (item 80a).
      const created = await hireDeps.createWorktree({
        workdir: checkout,
        name: plan.worktreeName,
        source: null,
        // The hire targets a mission that already exists, so — unlike the
        // lead's own worktree at launch — both halves of the L11 record's
        // key are already known. Passing them here is what lets the host
        // record this worktree durably instead of only staging it as a
        // one-shot hint the create's own settling never promotes (finding
        // 60).
        sessionRef: plan.sessionRef,
        seatLabel: plan.seatLabel,
      });
      await hireDeps.stageCreateHint({
        commandId: plan.commandId,
        path: created.path,
      });
      // Its commits reach Pulse from a hook in its own worktree, never from
      // the seat being asked to report. Best-effort — an install that fails
      // must not cost the hire the seat it just cut — but never silent.
      const wipShare = await armSeatWorktreeForSharing(
        created,
        plan,
        request.eventId,
      );
      // What staging put on disk for this seat, for the outcome: null until
      // the host answers, and still null when no repository vouched for it.
      let stagedPackRef: CodingSessionSeatPackRef | null = null;
      await publishSeatedCodingSessionCreate({
        channelId: plan.channelId,
        commandId: plan.commandId,
        seat: { actor: plan.actor, role: plan.role },
        seatLabel: plan.seatLabel,
        // The same coordinate the create is signed with (`projectRef` below),
        // so the hired seat's pack is the project's own (finding 84).
        projectRef: plan.projectRef,
        deps: hireDeps.seatDeps,
        onSeatStaged: ({ packRef }) => {
          stagedPackRef = packRef;
        },
        publish: async () => {
          const event = await hireDeps.signer(
            buildCodingSessionCreateEvent({
              channelId: plan.channelId,
              commandId: plan.commandId,
              projectRef: plan.projectRef,
              repoRef: null,
              sessionRef: plan.sessionRef,
              genesisRef: plan.genesisRef,
              actor: plan.actor,
              role: plan.role,
              providerInstanceRef: plan.providerInstanceRef,
              providerAuthorityPubkey: plan.providerAuthorityPubkey,
              model: plan.model,
              title: plan.title,
              initialTurn: plan.initialTurn,
              // The hire this create answers, by event id. Without it a seated
              // create is a seat nobody can attribute to a request: the
              // founder's Desktop signed it, so the brief read as the
              // founder's own words and the transcript said
              // `Your Desktop (hire host)` because nothing on the wire
              // recorded which lead had asked (batch 1, item 10).
              hireRef: request.eventId,
              // The router's decision, verbatim and on the wire. A seat whose
              // model cannot be explained from the events is a seat running
              // weights nobody can account for.
              routing: plan.routing,
            }),
          );
          await hireDeps.publisher.publishEvent(
            event,
            "Timed out while seating the hired agent.",
            "Failed to seat the hired agent.",
          );
        },
      });
      // Anything this host substituted for the lead's words — the runtime, the
      // model, or both — is said out loud in the umbrella, where both the lead
      // and the person can read it. A substitution nobody is told about is the
      // host quietly running something other than what was asked for.
      // Where the tree came from, said out loud in the umbrella. A person
      // reading "Hired a builder" has no way to tell which repository it is
      // working in, and on 2026-09-16 that silence is what let two seats run
      // a whole session against the wrong codebase.
      await publishCodingSessionLaneMessage(
        {
          channelId: plan.channelId,
          sessionRef: plan.sessionRef,
          content: codingSessionHireCheckoutLine({
            role: plan.role,
            path: checkout,
            source: resolved.source,
            passedOver: resolved.passedOver,
          }),
        },
        { publisher: hireDeps.publisher, signer: hireDeps.signer },
      ).catch(() => {});
      for (const notice of [plan.providerNotice, plan.modelNotice]) {
        if (notice === null) continue;
        await publishCodingSessionLaneMessage(
          {
            channelId: plan.channelId,
            sessionRef: plan.sessionRef,
            content: codingSessionHireNoticeLine({
              role: plan.role,
              notice,
            }),
          },
          { publisher: hireDeps.publisher, signer: hireDeps.signer },
        ).catch(() => {});
      }
      // The grant, last and never optional. A seat with no `grant-operator`
      // is a mute seat: it will work, and the relay will refuse its report.
      // The receipt is waited on first for the same reason the launcher waits
      // — a grant on a create the provider refused is authority over nothing.
      const grantFailure = await grantSeat(plan, hireDeps);
      if (grantFailure !== null) {
        const text = codingSessionHireGrantFailureText(grantFailure);
        await discloseCodingSessionHire(
          {
            channelId: plan.channelId,
            sessionRef: plan.sessionRef,
            requesterPubkey: request.requesterPubkey,
            text,
            notice: codingSessionHireGrantFailureNotice({
              role: plan.role,
              text,
            }),
          },
          current.input,
          hireDeps,
        );
      }
      report({
        ...outcomeOf(request, "seated", grantFailure),
        seatCommandId: plan.commandId,
        granted: grantFailure === null,
        seatActor: plan.actor,
        hostPubkey: operator,
        wipShare,
        checkout: { path: checkout, source: resolved.source },
        packRef: stagedPackRef,
        // Compared with the hire's own signer here, because the relay does not
        // (POLICY.md §5). This is what the seat's first turn is attributed to.
        requesterLabel: codingSessionHireRequesterLabel({
          standing: codingSessionHireRequesterStanding(request),
          nameFor: (pubkey) =>
            current.input.agents.find((agent) => agent.pubkey === pubkey)
              ?.name ?? null,
        }),
      });
    }
  }, [enabled]);

  return { outcomes, policy };
}
