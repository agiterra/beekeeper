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
import { recordPendingCodingSessionLifecycle } from "../lib/codingSessionPendingLifecycle";
import {
  buildCodingSessionCreateEvent,
  createCodingSessionLifecycleCommandId,
  createCodingSessionSessionRef,
} from "../lib/codingSessionLifecycleCommand";
import { publishCodingSessionAuthorityTransition } from "../lib/codingSessionRoster";
import { publishSeatedCodingSessionCreate } from "../lib/codingSessionSeatedCreate";
import { ensureProviderChannelMembership } from "../lib/providerChannelMembership";

/**
 * The crew launch, wired to this computer's relay, provider and keyring.
 *
 * The sequence itself lives in `codingSessionCrewLaunch.ts` and is tested
 * there; this hook only supplies the steps that touch the outside world — the
 * channel, the relay publishes, the receipts and this computer's seat custody
 * — and keeps the step list for the screen.
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
  /** Provider that will run every seat. */
  providerInstanceRef: string | null;
  providerAuthorityPubkey: string | null;
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
}) {
  const [steps, setSteps] = React.useState<CodingSessionCrewLaunchStep[]>([]);
  const [isLaunching, setIsLaunching] = React.useState(false);
  const [result, setResult] =
    React.useState<CodingSessionCrewLaunchResult | null>(null);

  // Read through a ref so the launch callback stays stable while the form's
  // fields keep changing under it.
  const settings = React.useRef(input);
  settings.current = input;

  const launch = React.useCallback(
    async (
      launchInput: CodingSessionCrewLaunchInput,
    ): Promise<CodingSessionCrewLaunchResult> => {
      const current = settings.current;
      setResult(null);
      setSteps(planCodingSessionCrewLaunch(launchInput));
      setIsLaunching(true);
      try {
        if (!current.providerInstanceRef || !current.providerAuthorityPubkey) {
          throw new Error(
            "No coding-session provider is available to run this team.",
          );
        }
        const providerInstanceRef = current.providerInstanceRef;
        const providerAuthorityPubkey = current.providerAuthorityPubkey;
        const launched = await launchCodingSessionCrew(launchInput, {
          ensureChannel: current.ensureChannelId ?? undefined,
          createLeadWorktree: createCodingSessionWorktree,
          newSessionRef: createCodingSessionSessionRef,
          publishGenesis: async ({ channelId, sessionRef }) => {
            // Strict membership on 442xx: a provider that joins after the
            // creates are published never sees them. Here rather than before
            // the launch because the channel may not exist until the launch's
            // own first step publishes it.
            await ensureProviderChannelMembership({
              channelId,
              providerPubkey: providerAuthorityPubkey,
            });
            return publishCodingSessionGenesis({ channelId, sessionRef });
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
            const commandId = createCodingSessionLifecycleCommandId();
            // What the staging call found on this computer, kept so the step
            // list can say a seat carries no role skills instead of implying
            // it does.
            let packStaged = false;
            // The directory the launch settled on — the lead's worktree when
            // it cut one — not the checkout the form still holds.
            const seatWorkdir = workdir ?? current.workdir;
            if (seatWorkdir) {
              await stageCodingSessionCreateHint({
                commandId,
                path: seatWorkdir,
              });
              // What a person returns to is the checkout, never the worktree
              // that was made from it — otherwise the next session prefills a
              // worktree and then cuts a worktree of a worktree.
              await recordCodingSessionWorkdirUse(
                current.workdir ?? seatWorkdir,
              );
            }
            await publishSeatedCodingSessionCreate({
              channelId,
              commandId,
              seat: { actor: seat.actor, role: seat.role },
              seatLabel: seat.actorLabel,
              deps: {
                ensureMembership: ensureActorChannelMembership,
                stageSeat: async (staging) => {
                  const staged = await stageCodingSessionActorSeat(staging);
                  packStaged = staged.packStaged;
                  return staged;
                },
                clearSeat: clearCodingSessionActorSeat,
              },
              publish: async () => {
                const event = await signRelayEvent(
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
                await relayClient.publishEvent(
                  event,
                  "Timed out while seating the team.",
                  "Failed to seat the team.",
                );
                // The relay holds the signed create; file the row now rather
                // than waiting for the provider's 44223 facts, exactly as the
                // one-session path does — this is what puts the session in the
                // project's list the moment it exists.
                recordPendingCodingSessionLifecycle({
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
            awaitCodingSessionCreateReceipt({
              channelId,
              commandId,
              providerAuthorityPubkey,
            }),
          grantOperator: async ({ channelId, genesisRef, granteePubkey }) => {
            await publishCodingSessionAuthorityTransition({
              channelId,
              genesisRef,
              type: "grant-operator",
              granteePubkey,
            });
          },
          sendFirstTurn: async ({ channelId, target, text }) => {
            await publishCodingSessionCommand({
              channelId,
              commandId: createCodingSessionCommandId(),
              target,
              text,
              deliver: "boundary",
            });
          },
          onSteps: setSteps,
        });
        setResult(launched);
        return launched;
      } finally {
        setIsLaunching(false);
      }
    },
    [],
  );

  return { isLaunching, launch, result, steps };
}
