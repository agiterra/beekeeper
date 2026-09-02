import * as React from "react";

import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import {
  recordCodingSessionWorkdirUse,
  stageCodingSessionCreateHint,
} from "@/shared/api/tauriCodingSessionWorkdirs";
import { createCodingSessionWorktree } from "@/shared/api/tauriCodingSessionWorktrees";
import { ensureActorChannelMembership } from "../lib/actorSeatChannelMembership";
import {
  clearCodingSessionActorSeat,
  stageCodingSessionActorSeat,
} from "../lib/codingSessionActorSeatCustody";
import {
  createCodingSessionCommandId,
  publishCodingSessionCommand,
} from "../lib/codingSessionCommand";
import { awaitCodingSessionCreateReceipt } from "../lib/codingSessionCrewReceipt";
import {
  launchCodingSessionCrew,
  planCodingSessionCrewLaunch,
  type CodingSessionCrewLaunchInput,
  type CodingSessionCrewLaunchResult,
  type CodingSessionCrewLaunchStep,
} from "../lib/codingSessionCrewLaunch";
import { publishCodingSessionGenesis } from "../lib/codingSessionGenesis";
import { publishCodingSessionGoal } from "../lib/codingSessionGoal";
import {
  ensureCodingSessionCreateOperatorGrants,
  ensureCodingSessionSeatGrant,
} from "../lib/codingSessionOperatorGrant";
import { recordPendingCodingSessionLifecycle } from "../lib/codingSessionPendingLifecycle";
import {
  buildCodingSessionCreateEvent,
  createCodingSessionLifecycleCommandId,
  createCodingSessionSessionRef,
} from "../lib/codingSessionLifecycleCommand";
import {
  publishSeatedCodingSessionCreate,
  type SeatedCodingSessionCreateDeps,
} from "../lib/codingSessionSeatedCreate";
import { ensureProviderChannelMembership } from "../lib/providerChannelMembership";
import type { NewCodingSessionTarget } from "../lib/newCodingSessionModel";

/** Bind a launch to the exact runtime target returned by click-time preflight. */
export function codingSessionCrewLaunchRuntimeBinding(
  runtimeTarget: NewCodingSessionTarget,
): {
  providerAuthorityPubkey: string;
  providerInstanceRef: string;
} {
  const providerInstanceRef = runtimeTarget.provider.providerInstanceRef.trim();
  const providerAuthorityPubkey = runtimeTarget.signerPubkey.trim();
  if (
    !providerInstanceRef ||
    !/^[0-9a-f]{64}$/i.test(providerAuthorityPubkey)
  ) {
    throw new Error(
      "No coding-session provider is available to run this team.",
    );
  }
  return { providerAuthorityPubkey, providerInstanceRef };
}

/** Everything outside this hook that launching a team has to reach. */
export type CodingSessionCrewLaunchHostDeps = {
  /** The launch sequence itself. Real by default; never faked in tests. */
  runLaunch: typeof launchCodingSessionCrew;
  createWorktree: typeof createCodingSessionWorktree;
  newSessionRef: typeof createCodingSessionSessionRef;
  newSeatCommandId: typeof createCodingSessionLifecycleCommandId;
  newTurnCommandId: typeof createCodingSessionCommandId;
  ensureProviderMembership: typeof ensureProviderChannelMembership;
  publishGenesis: typeof publishCodingSessionGenesis;
  /** Publish the umbrella's kind:44227 goal. */
  publishGoal: typeof publishCodingSessionGoal;
  stageCreateHint: typeof stageCodingSessionCreateHint;
  recordWorkdirUse: typeof recordCodingSessionWorkdirUse;
  /** The three custody/membership steps `publishSeatedCodingSessionCreate` runs. */
  seatDeps: SeatedCodingSessionCreateDeps;
  signer: typeof signRelayEvent;
  publisher: {
    publishEvent: typeof relayClient.publishEvent;
  };
  recordPendingLifecycle: typeof recordPendingCodingSessionLifecycle;
  awaitSeatReceipt: typeof awaitCodingSessionCreateReceipt;
  ensureCreateOperatorGrants: typeof ensureCodingSessionCreateOperatorGrants;
  ensureSeatGrant: typeof ensureCodingSessionSeatGrant;
  publishCommand: typeof publishCodingSessionCommand;
};

/** The real thing: this computer's relay, disk, keystore and provider. */
export const DEFAULT_CODING_SESSION_CREW_LAUNCH_DEPS: CodingSessionCrewLaunchHostDeps =
  {
    runLaunch: launchCodingSessionCrew,
    createWorktree: createCodingSessionWorktree,
    newSessionRef: createCodingSessionSessionRef,
    newSeatCommandId: createCodingSessionLifecycleCommandId,
    newTurnCommandId: createCodingSessionCommandId,
    ensureProviderMembership: ensureProviderChannelMembership,
    publishGenesis: publishCodingSessionGenesis,
    publishGoal: publishCodingSessionGoal,
    stageCreateHint: stageCodingSessionCreateHint,
    recordWorkdirUse: recordCodingSessionWorkdirUse,
    seatDeps: {
      ensureMembership: ensureActorChannelMembership,
      stageSeat: stageCodingSessionActorSeat,
      clearSeat: clearCodingSessionActorSeat,
    },
    signer: signRelayEvent,
    publisher: relayClient,
    recordPendingLifecycle: recordPendingCodingSessionLifecycle,
    awaitSeatReceipt: awaitCodingSessionCreateReceipt,
    ensureCreateOperatorGrants: ensureCodingSessionCreateOperatorGrants,
    ensureSeatGrant: ensureCodingSessionSeatGrant,
    publishCommand: publishCodingSessionCommand,
  };

/**
 * What a launch did about the umbrella's goal.
 *
 * A team launch used to send the goal as the lead's first turn and stop there
 * (item 103, finding 5): the words existed in one agent's transcript and
 * nowhere else, so the goal pill was empty, every later seat was hired against
 * a goal it could not read, and the one sentence the whole session is for was
 * not on the wire. The goal is now published as its own kind:44227 — and when
 * that publish fails the launch still finishes, because a team with an
 * unpublished goal is worth more than no team, but it says so rather than
 * letting an empty pill imply nobody set one.
 */
export type CodingSessionCrewLaunchGoalOutcome = {
  /** True only when a 44227 for this umbrella actually went out. */
  published: boolean;
  /** The publish's own words when it failed, else null. */
  reason: string | null;
};

/** A launch result, plus what became of the goal this launch was given. */
export type CodingSessionCrewLaunchHostResult =
  CodingSessionCrewLaunchResult & {
    goal: CodingSessionCrewLaunchGoalOutcome;
  };

/**
 * The crew launch, wired to this computer's relay, provider and keyring.
 *
 * The sequence itself lives in `codingSessionCrewLaunch.ts` and is tested
 * there; this hook only supplies the steps that touch the outside world — the
 * channel, the relay publishes, the receipts and this computer's seat custody
 * — and keeps the step list for the screen. Every one of them arrives through
 * {@link CodingSessionCrewLaunchHostDeps}, so what a test watches is the real
 * sequence rather than a re-enactment of it.
 *
 * **Not resumable.** Unlike the single-session create, a crew launch is not
 * written to durable storage before it publishes: the durable record holds one
 * in-flight command per scope, and a crew is N of them. If the app dies
 * mid-launch the seats already created stay created — they are real
 * executions in a real session — and the rest is finished from the session
 * itself. The screen says so rather than implying a resume that does not
 * exist.
 */
export function useCodingSessionCrewLaunch(input: {
  /**
   * Resolve — creating it if needed — the channel the team launches into.
   *
   * Supplied only where the destination is a fact without an id yet: a project
   * whose sessions channel has never been published. It is the project flow's
   * own `ensureChannelId`, so the team launch and the one-session create mint
   * the same channel and record the same project fact.
   */
  ensureChannelId?: (() => Promise<string>) | null;
  /**
   * The checkout the launch was pointed at, host-local and never on the wire.
   *
   * Only a fallback now: the directory the lead actually runs in is decided by
   * the launch itself (it may cut a worktree first) and arrives on each
   * `publishSeatCreate`.
   */
  workdir: string | null;
  title: string | null;
  /** Injected in tests; this computer's relay, disk and keyring by default. */
  deps?: CodingSessionCrewLaunchHostDeps;
}) {
  const [steps, setSteps] = React.useState<CodingSessionCrewLaunchStep[]>([]);
  const [isLaunching, setIsLaunching] = React.useState(false);
  const [result, setResult] =
    React.useState<CodingSessionCrewLaunchHostResult | null>(null);

  // Read through a ref so the launch callback stays stable while the form's
  // fields keep changing under it.
  const settings = React.useRef(input);
  settings.current = input;

  const launch = React.useCallback(
    async (
      launchInput: CodingSessionCrewLaunchInput,
      runtimeTarget: NewCodingSessionTarget,
    ): Promise<CodingSessionCrewLaunchHostResult> => {
      const current = settings.current;
      const deps = current.deps ?? DEFAULT_CODING_SESSION_CREW_LAUNCH_DEPS;
      setResult(null);
      setSteps(planCodingSessionCrewLaunch(launchInput));
      setIsLaunching(true);
      // The goal is published inside `publishGenesis`, so this is written
      // before the launch and read after it.
      let goal: CodingSessionCrewLaunchGoalOutcome = {
        published: false,
        reason: "the launch stopped before the genesis was published",
      };
      try {
        const { providerAuthorityPubkey, providerInstanceRef } =
          codingSessionCrewLaunchRuntimeBinding(runtimeTarget);
        const launched = await deps.runLaunch(launchInput, {
          ensureChannel: current.ensureChannelId ?? undefined,
          createLeadWorktree: deps.createWorktree,
          newSessionRef: deps.newSessionRef,
          publishGenesis: async ({ channelId, sessionRef }) => {
            // Strict membership on 442xx: a provider that joins after the
            // creates are published never sees them. Here rather than before
            // the launch because the channel may not exist until the launch's
            // own first step publishes it.
            await deps.ensureProviderMembership({
              channelId,
              providerPubkey: providerAuthorityPubkey,
            });
            const genesis = await deps.publishGenesis({
              channelId,
              sessionRef,
            });
            // After the genesis and before any create: the goal is a fact
            // about the umbrella, and every seat this launch creates — and
            // every seat the lead later hires — should be able to read it
            // from the wire rather than from one agent's first turn.
            try {
              await deps.publishGoal({
                channelId,
                content: launchInput.goal,
                sessionRef,
              });
              goal = { published: true, reason: null };
            } catch (error) {
              // Never fatal. The first turn still carries the words, so a
              // failed 44227 costs the pill and the later hires, not the run.
              // Never "not published, no reason": an empty message would
              // render as an unexplained missing goal, which is the silence
              // this whole path exists to end.
              const said = (
                error instanceof Error ? error.message : String(error)
              ).trim();
              goal = {
                published: false,
                reason:
                  said.length > 0 ? said : "the goal publish did not go out",
              };
            }
            return genesis;
          },
          publishSeatCreate: async ({
            seat,
            index,
            channelId,
            sessionRef,
            genesisRef,
            projectRef,
            workdir,
          }) => {
            const commandId = deps.newSeatCommandId();
            // What the staging call found on this computer, kept so the step
            // list can say a seat carries no role skills instead of implying
            // it does.
            let packStaged = false;
            // The directory the launch settled on — the lead's worktree when
            // it cut one — not the checkout the form still holds.
            const seatWorkdir = workdir ?? current.workdir;
            if (seatWorkdir) {
              await deps.stageCreateHint({
                commandId,
                path: seatWorkdir,
              });
              // What a person returns to is the checkout, never the worktree
              // that was made from it — otherwise the next session prefills a
              // worktree and then cuts a worktree of a worktree.
              await deps.recordWorkdirUse(current.workdir ?? seatWorkdir);
            }
            await publishSeatedCodingSessionCreate({
              channelId,
              commandId,
              seat: { actor: seat.actor, role: seat.role },
              seatLabel: seat.actorLabel,
              deps: {
                ensureMembership: deps.seatDeps.ensureMembership,
                stageSeat: async (staging) => {
                  const staged = await deps.seatDeps.stageSeat(staging);
                  // `?? false` only satisfies the injected dep's optional
                  // return type; the real `stageCodingSessionActorSeat`
                  // always answers with a boolean
                  // (`codingSessionActorSeatCustody.ts:37-48`), and
                  // `packStaged` was already initialised to false above.
                  packStaged = staged?.packStaged ?? false;
                  return staged;
                },
                clearSeat: deps.seatDeps.clearSeat,
              },
              publish: async () => {
                const event = await deps.signer(
                  buildCodingSessionCreateEvent({
                    channelId,
                    commandId,
                    // The session's placement authority for the rest of its
                    // life. A team launched from inside a project used to sign
                    // null here and land where nobody was looking (item 87a).
                    projectRef,
                    repoRef: null,
                    sessionRef,
                    genesisRef,
                    actor: seat.actor,
                    role: seat.role,
                    providerInstanceRef,
                    providerAuthorityPubkey,
                    // Verbatim: the seat's model was decided once, in
                    // `resolveCodingSessionCrewSeats`, and that is the model
                    // the family check and the roster both read. A fallback
                    // applied here would publish a model no check ever saw.
                    model: seat.model,
                    title: index === 0 && current.title ? current.title : null,
                    // The goal reaches the primary as its own turn, after the
                    // grant — so it can carry the roster the grant makes true.
                    initialTurn: null,
                  }),
                );
                await deps.publisher.publishEvent(
                  event,
                  "Timed out while seating the team.",
                  "Failed to seat the team.",
                );
                // The relay holds the signed create; file the row now rather
                // than waiting for the provider's 44223 facts, exactly as the
                // one-session path does — this is what puts the session in the
                // project's list the moment it exists.
                deps.recordPendingLifecycle({
                  kind: "create",
                  channelId,
                  commandId,
                  sessionRef,
                  title: index === 0 ? current.title : null,
                  projectRef,
                  providerAuthorityPubkey,
                  hasInitialTurn: false,
                  recordedAt: Date.now(),
                });
              },
            });
            return { commandId, packStaged };
          },
          awaitSeatReceipt: ({ channelId, commandId }) =>
            deps.awaitSeatReceipt({
              channelId,
              commandId,
              providerAuthorityPubkey,
            }),
          grantOperator: async ({ channelId, genesisRef, granteePubkey }) => {
            const result = await deps.ensureCreateOperatorGrants({
              channelId,
              genesisRef,
              providerAuthorityPubkey,
              actorPubkey: granteePubkey,
            });
            if (!result.ok) throw new Error(result.reason);
          },
          grantLeadSeat: async ({
            channelId,
            genesisRef,
            actorPubkey,
            role,
          }) => {
            await deps.ensureSeatGrant({
              channelId,
              genesisRef,
              actorPubkey,
              role,
            });
          },
          sendFirstTurn: async ({ channelId, target, text }) => {
            await deps.publishCommand({
              channelId,
              commandId: deps.newTurnCommandId(),
              target,
              text,
              deliver: "boundary",
            });
          },
          onSteps: setSteps,
        });
        const hostResult: CodingSessionCrewLaunchHostResult = {
          ...launched,
          goal,
        };
        setResult(hostResult);
        return hostResult;
      } finally {
        setIsLaunching(false);
      }
    },
    [],
  );

  return { isLaunching, launch, result, steps };
}
