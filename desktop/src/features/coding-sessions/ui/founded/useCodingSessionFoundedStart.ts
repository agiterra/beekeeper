import * as React from "react";

import { createCodingSessionWorktree } from "@/shared/api/tauriCodingSessionWorktrees";
import {
  type CodingSessionAutoGoalInput,
  type CodingSessionAutoGoalOutcome,
  autoSummarizeCodingSessionGoal,
} from "../../lib/codingSessionAutoGoal";
import { codingSessionCrewLeadDestination } from "../../lib/codingSessionCrewLaunch";
import { clearCodingSessionFoundedDraft } from "../../lib/codingSessionFoundedDraft";
import { codingSessionLaunchBlockersBySurface } from "../../lib/codingSessionLaunchForm";
import { isNewCodingSessionTargetReady } from "../../lib/newCodingSessionModel";
import type { CodingSessionFoundedSetupModel } from "./useCodingSessionFoundedSetup";
import type { CodingSessionFoundedTextModel } from "./useCodingSessionFoundedText";

/**
 * The acts Start makes outside the create itself; injected in tests: the
 * worktree cut, the draft forgotten, and — Solo only — the goal summarized.
 *
 * There is no post-Start namer here any more (SV-31, Decision 7), and none
 * comes back with the session-title mode (D9, SV-56): in every mode a
 * blank Name publishes nothing after Start. In the agent mode (the default)
 * the agent's computer may title the session from its first message, signed
 * by that provider as a kind 44252; in "Use my naming model" and Off it
 * stays untitled. This desktop never publishes a model's words as the
 * founder's 44229, and never signs a 44252. The goal step consults the
 * naming model only in "Use my naming model" (`namingModelConsulted`).
 */
export type CodingSessionFoundedStartDeps = {
  createWorktree: typeof createCodingSessionWorktree;
  clearFoundedDraft: typeof clearCodingSessionFoundedDraft;
  /** Solo only: the goal becomes one line once the create is accepted. */
  autoGoal: (
    input: CodingSessionAutoGoalInput,
  ) => Promise<CodingSessionAutoGoalOutcome>;
};

/**
 * Summarize a Solo session's goal after Start, quietly. The full prompt
 * stays the goal on every path but success, and the transcript carries it
 * as the first message regardless, so a refusal needs no toast.
 */
export async function autoSummarizeCodingSessionGoalAfterStart(
  input: CodingSessionAutoGoalInput,
): Promise<CodingSessionAutoGoalOutcome> {
  const outcome = await autoSummarizeCodingSessionGoal(input);
  if (outcome.kind === "failed") {
    console.warn("coding-session goal summary failed", outcome.reason);
  }
  return outcome;
}

const DEFAULT_START_DEPS: CodingSessionFoundedStartDeps = {
  createWorktree: createCodingSessionWorktree,
  clearFoundedDraft: clearCodingSessionFoundedDraft,
  autoGoal: autoSummarizeCodingSessionGoalAfterStart,
};

/** The slice of the setup card's state that pressing Start reads. */
export type CodingSessionFoundedStartSetup = Pick<
  CodingSessionFoundedSetupModel,
  | "canLaunch"
  | "candidates"
  | "draft"
  | "fullAccess"
  | "fullAccessOffered"
  | "hireRoster"
  | "lead"
  | "leadModel"
  | "launch"
  | "markAttempted"
  | "mode"
  | "readiness"
  | "policySet"
  | "projectName"
  | "refreshRuntimeTarget"
  | "selectedTarget"
  | "setIsPreparing"
  | "setLaunchError"
  | "setSetupError"
  | "submit"
  | "useWorktree"
  | "workdir"
  | "worktreeName"
  | "worktreeSource"
> & {
  text: Pick<
    CodingSessionFoundedTextModel,
    "flush" | "markPromptAttempted" | "name" | "prompt" | "remember"
  >;
};

/**
 * The repository a Start signs: the click-time answer, or null.
 *
 * Formerly the dialog's rule (`useNewCodingSessionLaunchSubmit`, deleted
 * 2026-09-10), now honest on this screen: a session founded in a reused
 * workspace carries that workspace's
 * `repoRef` only while the Where field still names that folder. Anywhere
 * else is "none named" — never a guess (LANE-L20).
 */
export function codingSessionFoundedStartRepoRef(input: {
  workdir: string;
  workspaceSourcePath: string | null;
  repoRef: string | null;
}): string | null {
  if (input.workspaceSourcePath === null) return input.repoRef;
  return input.workdir.trim() === input.workspaceSourcePath
    ? input.repoRef
    : null;
}

/**
 * What pressing the founded page's Start does.
 *
 * First, whichever mode: the two text fields flush — name, then prompt, only
 * the dirty ones — and a refusal stops here with the relay's words, so a
 * Start never creates against a name or a prompt the relay refused. A blank
 * prompt is the on-press blocker, said under the field; nothing is signed.
 *
 * Then the umbrella exists — genesis on the wire, name and prompt just
 * flushed — so neither branch founds anything:
 *
 * - **Solo**: the durable create that has always existed, joined to the
 *   umbrella by its refs, with the prompt as the first message and no title
 *   (the 44229 was just flushed). The worktree is cut here first, because
 *   its path is what the create names.
 * - **Team**: `launchCodingSessionCrew` against the existing umbrella —
 *   policy, one create for the lead, its grants, its first turn. The lead's
 *   worktree is cut by the launch's own step, before the create is signed.
 *   The first turn names the agents this computer would seat for the
 *   session's hires — its project's agents, or agents in no project — never
 *   the launch seats alone, which are only the lead.
 *
 * `rememberWorkspace` and `repoRef` come from the click-time draft on both
 * branches. Both forget the drafts on success only: a refused create keeps
 * the answers for the retry.
 */
export function useCodingSessionFoundedStart(input: {
  channelId: string;
  sessionRef: string;
  genesisRef: string;
  /** The founder — whose key signs the Solo goal summary. */
  founderPubkey: string;
  projectRef: string | null;
  setup: CodingSessionFoundedStartSetup;
  goCodingSession: (
    channelId: string,
    generationId: string,
    options: { replace: boolean },
  ) => Promise<unknown> | unknown;
  deps?: CodingSessionFoundedStartDeps;
}): () => void {
  const {
    channelId,
    sessionRef,
    genesisRef,
    founderPubkey,
    projectRef,
    setup,
    goCodingSession,
    deps = DEFAULT_START_DEPS,
  } = input;
  // The one guard over both branches (REVIEW-B3 F2): a runtime whose models
  // command has not answered publishes `defaultModel: ""`, and an empty
  // model is not a model.
  const publishedModel =
    setup.leadModel !== null && setup.leadModel.trim().length > 0
      ? setup.leadModel.trim()
      : null;
  // Finding 51: `canLaunch` only goes false once the hooks' own busy state
  // flips, and that flip happens inside the async work below. This ref
  // closes the window a second click could land in.
  const submittingRef = React.useRef(false);
  return React.useCallback(() => {
    if (submittingRef.current) return;
    const prompt = setup.text.prompt.trim();
    // The blockers surfaced on press — a blank prompt, no lead in Team, an
    // unnamed worktree — are said under their fields now, and nothing is
    // signed or cut.
    const onAttempt = codingSessionLaunchBlockersBySurface(
      setup.readiness,
    ).onAttempt;
    if (prompt.length === 0 || onAttempt.length > 0) {
      if (prompt.length === 0) setup.text.markPromptAttempted();
      setup.markAttempted();
      return;
    }
    if (!setup.canLaunch) return;
    const selectedTarget = setup.selectedTarget;
    if (!selectedTarget) {
      setup.setLaunchError(
        "No coding-session provider is selected, so there is nothing to run the lead on.",
      );
      return;
    }
    submittingRef.current = true;
    setup.setLaunchError(null);
    setup.setSetupError(null);
    const checkout = setup.workdir.trim();
    const worktreeName = setup.worktreeName.trim();
    const repoRef = codingSessionFoundedStartRepoRef({
      workdir: setup.workdir,
      workspaceSourcePath: setup.draft.workspaceSourcePath,
      repoRef: setup.draft.repoRef,
    });
    // A blank Name publishes nothing here: the agent's computer titles the
    // session from this first message as a provider-signed 44252 (SV-31).
    // A typed or picked Name was flushed above as the founder's 44229.
    // Solo only: the goal above the transcript becomes one line. A Team
    // session's goal is the lead's mission, left exactly as written.
    const goalAfterStart = () => {
      if (setup.mode !== "solo") return;
      void deps.autoGoal({
        channelId,
        sessionRef,
        founderPubkey,
        firstMessage: prompt,
      });
    };
    void (async () => {
      try {
        // Both fields first. A refusal is the relay's words and a stop.
        const flushed = await setup.text.flush();
        if (!flushed.ok) {
          setup.setLaunchError(flushed.reason);
          return;
        }
        if (setup.mode === "solo") {
          let effectiveWorkdir = checkout;
          if (setup.useWorktree && effectiveWorkdir.length > 0) {
            setup.setIsPreparing(true);
            try {
              effectiveWorkdir = (
                await deps.createWorktree({
                  workdir: effectiveWorkdir,
                  name: worktreeName,
                  source: setup.worktreeSource,
                  sessionRef,
                  projectRef,
                })
              ).path;
            } catch (error) {
              setup.setSetupError(
                `Could not create the worktree: ${
                  error instanceof Error ? error.message : String(error)
                }`,
              );
              return;
            } finally {
              setup.setIsPreparing(false);
            }
          }
          const outcome = await setup.submit({
            target:
              selectedTarget.channelId === channelId
                ? selectedTarget
                : { ...selectedTarget, channelId },
            model: publishedModel,
            // The 44229 was just flushed; a title here would publish a second.
            title: null,
            initialTurn: prompt,
            workdir: effectiveWorkdir.length > 0 ? effectiveWorkdir : null,
            rememberWorkdir: checkout.length > 0 ? checkout : null,
            rememberWorkspace: setup.draft.rememberWorkspace,
            projectRef,
            repoRef,
            seat: null,
            seatLabel: null,
            sessionRef,
            genesisRef,
          });
          // A refused publish keeps the drafts: `publishError` shows the
          // relay's words and the person retries with the same answers.
          if (!outcome.ok) return;
          setup.text.remember();
          deps.clearFoundedDraft(sessionRef);
          goalAfterStart();
          return;
        }
        const lead = setup.lead;
        if (lead.kind !== "agent") {
          // Readiness already blocks on this; a belt over that brace, said
          // rather than silently returned.
          setup.setLaunchError(
            "Pick an agent to lead this session, or switch to Solo.",
          );
          return;
        }
        const fresh = await setup.refreshRuntimeTarget();
        if (!fresh || !isNewCodingSessionTargetReady(fresh)) {
          setup.setLaunchError(
            fresh?.availability?.hint ??
              "No installed and authenticated runtime is ready to run the lead.",
          );
          return;
        }
        const launched = await setup.launch(
          {
            channelId,
            goal: prompt,
            seats: [
              {
                personaId:
                  setup.candidates.find((entry) => entry.pubkey === lead.actor)
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
            projectRef,
            repoRef,
            provider: {
              allowedModels: fresh.provider.allowedModels,
              instanceRef: fresh.provider.providerInstanceRef,
              label: fresh.availability?.label ?? null,
            },
            policySet: setup.policySet,
            workdir: checkout.length > 0 ? checkout : null,
            fullAccess: setup.fullAccessOffered && setup.fullAccess,
            leadWorktree:
              setup.useWorktree && worktreeName.length > 0
                ? { name: worktreeName, source: setup.worktreeSource }
                : null,
            existingUmbrella: { sessionRef, genesisRef },
            hireRoster: {
              projectRef,
              projectName: setup.projectName ?? null,
              agents: setup.hireRoster ?? [],
            },
          },
          fresh,
        );
        if (!launched.ok) {
          setup.setLaunchError(launched.failureReason);
          return;
        }
        setup.text.remember();
        deps.clearFoundedDraft(sessionRef);
        const destination = codingSessionCrewLeadDestination({
          result: launched,
          providerAuthorityPubkey: fresh.signerPubkey,
        });
        if (destination) {
          void goCodingSession(
            destination.channelId,
            destination.generationId,
            {
              replace: true,
            },
          );
        } else {
          // The screen's own catalog watcher still hands off once the
          // generation appears; say why nothing moved now.
          setup.setLaunchError(
            "The lead was seated, but its generation could not be named from the receipt. This screen opens it when the catalog reports it.",
          );
        }
      } catch (error) {
        setup.setLaunchError(
          error instanceof Error
            ? error.message
            : "The session could not be started.",
        );
      } finally {
        submittingRef.current = false;
      }
    })();
  }, [
    channelId,
    deps,
    founderPubkey,
    genesisRef,
    goCodingSession,
    projectRef,
    publishedModel,
    sessionRef,
    setup,
  ]);
}
