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
  stageCodingSessionActorSeat,
} from "../lib/codingSessionActorSeatCustody";
import {
  createCodingSessionCommandId,
  publishCodingSessionCommand,
  type CodingSessionCommandTarget,
} from "../lib/codingSessionCommand";
import {
  codingSessionHireModelNoticeLine,
  codingSessionHireRefusalNotice,
  planCodingSessionHireAnswer,
  type CodingSessionHireAnswer,
} from "../lib/codingSessionHireAnswer";
import {
  readCodingSessionHirePolicy,
  type CodingSessionHirePolicy,
} from "../lib/codingSessionHirePolicy";
import { selectUnansweredCodingSessionHires } from "../lib/codingSessionHireSeat";
import {
  classifyCodingSessionHireEvent,
  type CodingSessionHireRequest,
} from "../lib/codingSessionHireWire";
import { publishCodingSessionLaneMessage } from "../lib/codingSessionLanePublish";
import {
  buildCodingSessionCreateEvent,
  createCodingSessionLifecycleCommandId,
} from "../lib/codingSessionLifecycleCommand";
import { subscribeToObservedCodingSessionEvents } from "../lib/codingSessionObservedEvents";
import { fetchCodingSessionRosterFold } from "../lib/codingSessionRoster";
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
  /** What the host did. `error` means the answer itself failed to go out. */
  state: "seated" | "refused" | "ignored" | "error";
  /** Refusal code, or the failure's own words. Null for a seated hire. */
  detail: string | null;
  /** The create's commandId, which is the hire's receipt key. */
  seatCommandId: string | null;
};

/** A managed agent, narrowed to what seating one needs. */
export type CodingSessionHireAgent = {
  pubkey: string;
  name: string;
  homeRole: string | null;
  hasRolePack?: boolean;
  model: string | null;
};

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
  newSeatCommandId: () => string;
  newTurnCommandId: () => string;
  /** This host's clock, Unix seconds. The staleness window is read from it. */
  now: () => number;
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
  },
  signer: signRelayEvent,
  publisher: relayClient,
  newSeatCommandId: createCodingSessionLifecycleCommandId,
  newTurnCommandId: createCodingSessionCommandId,
  now: () => Math.floor(Date.now() / 1000),
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
  /** This computer's runtimes — their auth state and their model catalogs. */
  runtimes: readonly CodingSessionProviderRuntime[];
  /** Where each seat's worktree is cut from, by channel. Host-local. */
  checkoutForChannel: (channelId: string) => string | null;
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
    const record = (outcome: CodingSessionHireOutcome) => {
      if (cancelled) return;
      setOutcomes((previous) => [...previous, outcome]);
    };

    const receive = (events: readonly RelayEvent[]) => {
      const current = latest.current;
      const allowed = new Set(current.input.channelIds);
      if (allowed.size === 0) return;
      const requests: CodingSessionHireRequest[] = [];
      for (const event of events) {
        const classified = classifyCodingSessionHireEvent(event, allowed);
        if (classified.kind !== "hire") continue;
        requests.push(classified);
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
          });
        });
      }
    };

    const unsubscribe = latest.current.deps.subscribe(receive);
    return () => {
      cancelled = true;
      unsubscribe();
    };

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

      const answer = planCodingSessionHireAnswer({
        request,
        umbrella,
        authority: {
          founderPubkey: umbrella.founderPubkey,
          grantedOperators,
        },
        policy: current.policy,
        candidates: current.input.agents.map((agent) => ({
          pubkey: agent.pubkey,
          name: agent.name,
          homeRole: agent.homeRole,
          ...(agent.hasRolePack === undefined
            ? {}
            : { hasRolePack: agent.hasRolePack }),
          model: agent.model,
        })),
        availableProviderInstanceRefs: current.input.runtimes
          .filter((runtime) => runtime.authState === "ready")
          .map((runtime) => runtime.instanceRef),
        // Every runtime's own catalog, including the ones that are not ready:
        // a model is checked against the runtime that would run it, and an
        // empty list means "not read", which refuses nothing.
        modelCatalogs: new Map(
          current.input.runtimes.map((runtime) => [
            runtime.instanceRef,
            runtime.allowedModels,
          ]),
        ),
        providerAuthorityPubkey,
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
      // The tree exists before the create is signed, because its path *is* the
      // working directory the create names. A seat is never signed against a
      // directory that does not exist yet — and never against the operator's
      // own checkout (item 80a).
      const checkout = current.input.checkoutForChannel(request.channelId);
      if (checkout !== null) {
        const created = await hireDeps.createWorktree({
          workdir: checkout,
          name: plan.worktreeName,
          source: null,
        });
        await hireDeps.stageCreateHint({
          commandId: plan.commandId,
          path: created.path,
        });
      }
      await publishSeatedCodingSessionCreate({
        channelId: plan.channelId,
        commandId: plan.commandId,
        seat: { actor: plan.actor, role: plan.role },
        seatLabel: plan.seatLabel,
        deps: hireDeps.seatDeps,
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
            }),
          );
          await hireDeps.publisher.publishEvent(
            event,
            "Timed out while seating the hired agent.",
            "Failed to seat the hired agent.",
          );
        },
      });
      // A model this host substituted for the lead's words is said out loud in
      // the umbrella, where both the lead and the person can read it. A
      // translation nobody is told about is the host quietly running something
      // other than what was asked for.
      if (plan.modelNotice !== null) {
        await publishCodingSessionLaneMessage(
          {
            channelId: plan.channelId,
            sessionRef: plan.sessionRef,
            content: codingSessionHireModelNoticeLine({
              role: plan.role,
              notice: plan.modelNotice,
            }),
          },
          { publisher: hireDeps.publisher, signer: hireDeps.signer },
        ).catch(() => {});
      }
      report({
        ...outcomeOf(request, "seated", null),
        seatCommandId: plan.commandId,
      });
    }
  }, [enabled]);

  return { outcomes, policy };
}

async function publishRefusal(
  request: CodingSessionHireRequest,
  answer: Extract<CodingSessionHireAnswer, { kind: "refused" }>,
  input: UseCodingSessionHireInput,
  deps: CodingSessionHireDeps,
): Promise<void> {
  // To the seat that asked, so it can act, and to the umbrella, so the person
  // who set the policy sees it enforced. Neither is allowed to fail the other:
  // a lead that heard nothing would wait out its whole turn budget.
  const target = input.targetForActor(
    request.channelId,
    request.requesterPubkey,
  );
  if (target) {
    await publishCodingSessionCommand(
      {
        channelId: request.channelId,
        commandId: deps.newTurnCommandId(),
        target,
        text: answer.text,
        deliver: "boundary",
      },
      { publisher: deps.publisher, signer: deps.signer },
    ).catch(() => {});
  }
  await publishCodingSessionLaneMessage(
    {
      channelId: request.channelId,
      sessionRef: request.action.sessionRef,
      content: codingSessionHireRefusalNotice({
        role: request.action.role,
        requesterLabel: "A seat",
        text: answer.text,
      }),
    },
    { publisher: deps.publisher, signer: deps.signer },
  ).catch(() => {});
}

function outcomeOf(
  request: CodingSessionHireRequest,
  state: CodingSessionHireOutcome["state"],
  detail: string | null,
): CodingSessionHireOutcome {
  return {
    commandId: request.commandId,
    channelId: request.channelId,
    sessionRef: request.action.sessionRef,
    role: request.action.role,
    state,
    detail,
    seatCommandId: null,
  };
}
