import * as React from "react";

import { createCodingSessionWorktree } from "@/shared/api/tauriCodingSessionWorktrees";
import { codingSessionCrewLeadDestination } from "../lib/codingSessionCrewLaunch";
import type { CodingSessionLaunchLead } from "../lib/codingSessionLaunchForm";
import {
  isNewCodingSessionTargetReady,
  type NewCodingSessionTarget,
} from "../lib/newCodingSessionModel";
import type { useCodingSessionCrewLaunch } from "./useCodingSessionCrewLaunch";
import type { useNewCodingSessionCreate } from "./useNewCodingSessionCreate";
import type { NewCodingSessionProjectContext } from "./NewCodingSessionDialog";
import type { NewCodingSessionLeadCandidate } from "./NewCodingSessionLeadField";

/**
 * What pressing the one form's button actually does.
 *
 * Split out of `NewCodingSessionLaunchForm.tsx` when that file crossed the
 * 1,000-line ceiling. It is one callback, and it is the only place the two
 * shapes of launch are chosen between:
 *
 * - **Governed** (an agent leads): genesis, goal, policy, one create for the
 *   lead, its grants, its first turn — the sequence in
 *   `launchCodingSessionCrew`, which creates exactly one seat.
 * - **Ungoverned** (you lead): the single durable create that has always
 *   existed, with the goal as the first message.
 *
 * Both re-read the runtime at click time. A cached target is a claim about a
 * process that may have exited while the form was open, and the create is
 * signed against whatever it says.
 */
export function useNewCodingSessionLaunchSubmit(input: {
  canLaunch: boolean;
  candidates: readonly NewCodingSessionLeadCandidate[];
  channelId: string | null;
  clearDraft: () => void;
  /**
   * The model this launch will publish: the lead identity's own, or an
   * explicit pick. Never the model picker's default for an agent that declared
   * none, and never an empty string — `resolveCodingSessionLeadModel` settles
   * both, and readiness blocks the launch when it comes back null for an agent
   * lead (REVIEW-B3 F1/F2).
   */
  leadModel: string | null;
  goCodingSession: (
    channelId: string,
    generationId: string,
    options: { replace: boolean },
  ) => Promise<unknown> | unknown;
  goal: string;
  governed: boolean;
  launch: ReturnType<typeof useCodingSessionCrewLaunch>["launch"];
  lead: CodingSessionLaunchLead;
  onDone: () => void;
  policySet: boolean;
  projectContext: NewCodingSessionProjectContext | null;
  refreshRuntimeTarget: () => Promise<NewCodingSessionTarget | null>;
  selectedTarget: NewCodingSessionTarget | null;
  setIsPreparingChannel: (value: boolean) => void;
  setLaunchError: (value: string | null) => void;
  setSetupError: (value: string | null) => void;
  submit: ReturnType<typeof useNewCodingSessionCreate>["submit"];
  title: string;
  useWorktree: boolean;
  workdir: string;
  worktreeName: string;
  worktreeSource: string | null;
}): () => void {
  const {
    canLaunch,
    candidates,
    channelId,
    clearDraft,
    goCodingSession,
    goal,
    governed,
    launch,
    lead,
    onDone,
    leadModel,
    policySet,
    projectContext,
    refreshRuntimeTarget,
    selectedTarget,
    setIsPreparingChannel,
    setLaunchError,
    setSetupError,
    submit,
    title,
    useWorktree,
    workdir,
    worktreeName,
    worktreeSource,
  } = input;
  // One guard, before either branch signs anything. The ungoverned branch had
  // it and the governed branch did not, so a runtime whose models command had
  // not answered published `defaultModel: ""`, the create builder threw
  // `action.model must not be empty`, and by then 44226, 44227 and possibly
  // 44245 were already on the wire (REVIEW-B3 F2).
  const publishedModel =
    leadModel !== null && leadModel.trim().length > 0 ? leadModel.trim() : null;
  // Finding 51 (live run 4): two sessions were created three seconds apart
  // from one click. `canLaunch` only goes false once `isLaunching` (from
  // `useCodingSessionCrewLaunch`) flips, and that flip happens *inside* the
  // async work below, after the channel is prepared and the runtime target
  // is re-read — both awaits. A second click landing in that window read a
  // still-`true` `canLaunch` and started a second, fully independent launch.
  // This ref closes exactly that window: set synchronously before the first
  // `await`, so a re-entrant call in the same tick or the next is a no-op
  // regardless of what the hook's own state has had time to render.
  const submittingRef = React.useRef(false);
  return React.useCallback(() => {
    if (!canLaunch || !selectedTarget || submittingRef.current) return;
    submittingRef.current = true;
    setLaunchError(null);
    setSetupError(null);
    void (async () => {
      try {
        let launchChannelId = channelId;
        if (launchChannelId === null && projectContext?.ensureChannelId) {
          setIsPreparingChannel(true);
          try {
            launchChannelId = await projectContext.ensureChannelId();
          } catch (error) {
            setSetupError(
              error instanceof Error
                ? error.message
                : "Could not prepare a channel for this project's sessions.",
            );
            return;
          } finally {
            setIsPreparingChannel(false);
          }
        }
        if (!governed || lead.kind !== "agent") {
          // One ungoverned execution, founded by you: the create path that has
          // always existed, and the worktree is cut before the command is signed
          // because its path *is* the working directory the create names.
          const checkout = workdir.trim();
          let effectiveWorkdir = checkout;
          if (useWorktree && effectiveWorkdir.length > 0) {
            setIsPreparingChannel(true);
            try {
              effectiveWorkdir = (
                await createCodingSessionWorktree({
                  workdir: effectiveWorkdir,
                  name: worktreeName.trim(),
                  source: worktreeSource,
                })
              ).path;
            } catch (error) {
              setSetupError(
                `Could not create the worktree: ${
                  error instanceof Error ? error.message : String(error)
                }`,
              );
              return;
            } finally {
              setIsPreparingChannel(false);
            }
          }
          await submit({
            target:
              launchChannelId && launchChannelId !== selectedTarget.channelId
                ? { ...selectedTarget, channelId: launchChannelId }
                : selectedTarget,
            model: publishedModel,
            title: title.trim().length > 0 ? title.trim() : null,
            initialTurn: goal.trim().length > 0 ? goal : null,
            workdir: effectiveWorkdir.length > 0 ? effectiveWorkdir : null,
            rememberWorkdir: checkout.length > 0 ? checkout : null,
            projectRef: projectContext?.projectRef ?? null,
            // LANE-L20 (finding 38): named whenever the launch resolved one —
            // the checkout's own repo, or the project's only repository.
            repoRef: projectContext?.repoRef ?? null,
            seat: null,
            seatLabel: null,
          });
          clearDraft();
          return;
        }
        try {
          // Re-read the runtime at click time: a cached target is a claim about
          // a process that may have exited while the form was open.
          const fresh = await refreshRuntimeTarget();
          if (!fresh || !isNewCodingSessionTargetReady(fresh)) {
            setLaunchError(
              fresh?.availability?.hint ??
                "No installed and authenticated runtime is ready to run the lead.",
            );
            return;
          }
          const launched = await launch(
            {
              channelId: launchChannelId,
              goal,
              // The one seat this launch creates. There is no roster, so there
              // is no seat whose model has to be guessed.
              seats: [
                {
                  personaId:
                    candidates.find((entry) => entry.pubkey === lead.actor)
                      ?.pubkey ?? lead.actor,
                  role: lead.role,
                  actor: lead.actor,
                  actorLabel: lead.label,
                  model: publishedModel,
                  vendor: null,
                  ...(lead.hasRolePack === undefined
                    ? {}
                    : { hasRolePack: lead.hasRolePack }),
                },
              ],
              primaryPersonaId: lead.actor,
              projectRef: projectContext?.projectRef ?? null,
              // LANE-L20 (finding 38): named whenever the launch resolved one.
              repoRef: projectContext?.repoRef ?? null,
              provider: {
                allowedModels: fresh.provider.allowedModels,
                instanceRef: fresh.provider.providerInstanceRef,
                label: fresh.availability?.label ?? null,
              },
              policySet,
              workdir: workdir.trim().length > 0 ? workdir.trim() : null,
              leadWorktree:
                useWorktree && worktreeName.trim().length > 0
                  ? { name: worktreeName.trim(), source: worktreeSource }
                  : null,
            },
            fresh,
          );
          if (launched.ok && launched.channelId) {
            onDone();
            const destination = codingSessionCrewLeadDestination({
              result: launched,
              providerAuthorityPubkey: fresh.signerPubkey,
            });
            if (destination) {
              void goCodingSession(
                destination.channelId,
                destination.generationId,
                { replace: true },
              );
            }
          } else if (launched.ok) {
            setLaunchError(
              "The session launched, but into no channel this screen can name.",
            );
          } else setLaunchError(launched.failureReason);
        } catch (error) {
          setLaunchError(
            error instanceof Error
              ? error.message
              : "The session could not be launched.",
          );
        }
      } finally {
        submittingRef.current = false;
      }
    })();
  }, [
    canLaunch,
    candidates,
    channelId,
    clearDraft,
    goCodingSession,
    goal,
    governed,
    launch,
    lead,
    onDone,
    policySet,
    projectContext,
    publishedModel,
    refreshRuntimeTarget,
    selectedTarget,
    setIsPreparingChannel,
    setLaunchError,
    setSetupError,
    submit,
    title,
    useWorktree,
    workdir,
    worktreeName,
    worktreeSource,
  ]);
}
