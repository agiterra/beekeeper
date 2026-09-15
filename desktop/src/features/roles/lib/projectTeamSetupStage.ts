import type {
  ProjectTeamSetupActivation,
  ProjectTeamSetupLaunch,
  ProjectTeamSetupPublication,
} from "./projectTeamSetup";
import {
  projectRosterBlockedSentence,
  projectRosterCompleteSentence,
  type ProjectRosterReadiness,
} from "./projectRosterReadiness";

/** The five steps a user walks through, in order. */
export const PROJECT_TEAM_SETUP_STEPS = [
  "author",
  "check_and_save",
  "publish",
  "install",
  "start_lead",
] as const;

export type ProjectTeamSetupStep = (typeof PROJECT_TEAM_SETUP_STEPS)[number];

export const PROJECT_TEAM_SETUP_STEP_LABELS: Record<
  ProjectTeamSetupStep,
  string
> = {
  author: "Author",
  check_and_save: "Check & save",
  publish: "Publish",
  install: "Install",
  start_lead: "Start lead",
};

/**
 * How far the current step got. Only `done` means finished: `uncertain`
 * (the host could not confirm an outcome) and `blocked` (refused, conflicted
 * or unresolvable) never read as done.
 */
export type ProjectTeamSetupStageState =
  | "todo"
  | "checking"
  | "working"
  | "uncertain"
  | "blocked"
  | "done";

export type ProjectTeamSetupStageId =
  | "author"
  | "authoring_uncertain"
  | "authoring_failed"
  | "check_and_save"
  | "saved_version_unverified"
  | "draft_invalid"
  | "publish"
  | "publish_blocked"
  | "publish_in_progress"
  | "publish_uncertain"
  | "publish_refused"
  | "install"
  | "source_unresolved"
  | "install_uncertain"
  | "install_refused"
  | "start_lead"
  | "lead_uncertain"
  | "lead_refused"
  | "roster_checking"
  | "roster_uncertain"
  | "roster_blocked"
  | "lead_started";

export type ProjectTeamSetupStage = {
  id: ProjectTeamSetupStageId;
  step: ProjectTeamSetupStep;
  state: ProjectTeamSetupStageState;
  /** Short heading for the current stage. */
  title: string;
  /** The one next action, as a sentence. */
  next: string;
};

/** Observed facts; every field is what the host or this view has seen so far. */
export type ProjectTeamSetupStageInput = {
  /**
   * `undefined` while the saved authoring records are still being read;
   * `"unreadable"` when that read failed.
   */
  authoringLaunch?:
    | Pick<ProjectTeamSetupLaunch, "status">
    | null
    | "unreadable";
  validation?: { valid: boolean } | null;
  /**
   * `recorded`: the host journal names one, not yet reverified here.
   * `unverified`: re-checking the recorded version failed.
   */
  snapshot: "none" | "checking" | "saved" | "recorded" | "unverified";
  /** `undefined` while publication choices are still being read. */
  publication?: Pick<ProjectTeamSetupPublication, "status"> | null;
  publicationBlocked?:
    | "missing_destination"
    | "missing_base"
    | "unavailable"
    | null;
  /** `undefined` while the installation record is still being read. */
  activation?: {
    source: unknown | null;
    installation: Pick<ProjectTeamSetupActivation["installation"], "status">;
    lead: Pick<ProjectTeamSetupActivation["lead"], "status" | "sessionRef">;
  } | null;
  /**
   * Whether the installed agents are this project's agents on this computer
   * (`projectRosterReadiness`). `undefined` reads as still checking: an
   * installation is never ready or complete on an unread roster.
   */
  roster?: Pick<ProjectRosterReadiness, "status" | "blocked" | "leadName">;
  /** For the completion sentence; "this project" when absent. */
  projectName?: string;
};

function stage(
  id: ProjectTeamSetupStageId,
  step: ProjectTeamSetupStep,
  state: ProjectTeamSetupStageState,
  title: string,
  next: string,
): ProjectTeamSetupStage {
  return { id, step, state, title, next };
}

/**
 * The roster gate after installation: `null` when every installed agent is
 * this project's agent here, otherwise the stage that says why the lead
 * can't hire them. It outranks "ready" and "started", never a lead failure.
 */
function rosterStage(
  roster: ProjectTeamSetupStageInput["roster"],
  leadStarted: boolean,
): ProjectTeamSetupStage | null {
  const step = leadStarted ? "start_lead" : "install";
  switch (roster?.status) {
    case "ready":
      return null;
    case undefined:
    case "checking":
      return stage(
        "roster_checking",
        step,
        "checking",
        "Checking project agents",
        "Checking that the installed agents belong to this project on this computer…",
      );
    case "unreadable":
      return stage(
        "roster_uncertain",
        step,
        "uncertain",
        "Project agents not confirmed",
        "This computer couldn't read its agents, so it isn't confirmed the lead can hire them. Check again.",
      );
    case "empty":
      return stage(
        "roster_uncertain",
        step,
        "uncertain",
        "No project agents recorded",
        "The installation recorded no agents, so the lead has nobody to hire. Retry installation (safe, keeps identities).",
      );
    case "blocked":
      return stage(
        "roster_blocked",
        step,
        "blocked",
        leadStarted
          ? "The lead can't hire every project agent"
          : "Project agents aren't associated",
        projectRosterBlockedSentence(roster, leadStarted),
      );
  }
}

function activationStage(
  activation: NonNullable<ProjectTeamSetupStageInput["activation"]>,
  input: ProjectTeamSetupStageInput,
): ProjectTeamSetupStage {
  const { installation, lead } = activation;
  if (installation.status === "installed" && lead.status === "started")
    return (
      rosterStage(input.roster, true) ??
      stage(
        "lead_started",
        "start_lead",
        "done",
        "Project lead started",
        projectRosterCompleteSentence(
          input.roster?.leadName ?? null,
          input.projectName,
        ),
      )
    );
  if (!activation.source)
    return stage(
      "source_unresolved",
      "install",
      "blocked",
      "Published roles not found here",
      "This computer couldn't resolve the published roles. Reopen setup before installing.",
    );
  if (installation.status === "refused")
    return stage(
      "install_refused",
      "install",
      "blocked",
      "Installation was refused",
      "Installing the roles was refused. See Technical details for the reason.",
    );
  if (installation.status === "unknown")
    return stage(
      "install_uncertain",
      "install",
      "uncertain",
      "Installation not confirmed",
      "It isn't confirmed the roles were installed. Retry installation; it installs the same published version.",
    );
  if (installation.status === "not_installed")
    return stage(
      "install",
      "install",
      "todo",
      "Install project roles",
      "Install the published roles on this computer.",
    );
  switch (lead.status) {
    case "needs_channel":
      return (
        rosterStage(input.roster, false) ??
        stage(
          "start_lead",
          "start_lead",
          "todo",
          "Start the project lead",
          "Every installed agent is this project's agent. Start the project lead; its project session channel is created first.",
        )
      );
    case "ready":
      return (
        rosterStage(input.roster, false) ??
        stage(
          "start_lead",
          "start_lead",
          "todo",
          "Start the project lead",
          "Every installed agent is this project's agent. Start the project lead.",
        )
      );
    case "starting":
      return stage(
        "lead_uncertain",
        "start_lead",
        "uncertain",
        "Project lead is starting",
        "The lead handoff is in progress. Reopen setup to check the result.",
      );
    case "unknown":
      return stage(
        "lead_uncertain",
        "start_lead",
        "uncertain",
        "Lead start not confirmed",
        lead.sessionRef
          ? "It isn't confirmed the lead started. Retry the lead handoff; it reuses the same saved request."
          : "It isn't confirmed the session channel was created. Retry session-channel setup; it reuses the same saved request.",
      );
    case "refused":
      return stage(
        "lead_refused",
        "start_lead",
        "blocked",
        "Lead start was refused",
        "The project lead couldn't start. See Technical details for the reason.",
      );
    case "started":
      // Started without a recorded installation contradicts the journal
      // order; never call that done.
      return stage(
        "lead_uncertain",
        "start_lead",
        "uncertain",
        "Lead start not confirmed",
        "The lead is recorded as started but its roles aren't recorded as installed. Reopen setup to check again.",
      );
  }
}

function publicationStage(
  input: ProjectTeamSetupStageInput,
  publication: Pick<ProjectTeamSetupPublication, "status">,
): ProjectTeamSetupStage {
  switch (publication.status) {
    case "adopted":
      if (input.activation === undefined)
        return stage(
          "install",
          "install",
          "checking",
          "Install project roles",
          "Checking what's installed on this computer…",
        );
      if (input.activation === null)
        return stage(
          "source_unresolved",
          "install",
          "blocked",
          "Installation status unavailable",
          "This computer couldn't read the installation record. Reopen setup to check again.",
        );
      return activationStage(input.activation, input);
    case "checking":
    case "candidate_prepared":
    case "pushed":
      return stage(
        "publish_in_progress",
        "publish",
        "working",
        "Publishing isn't finished",
        "Retry publication to continue the same request.",
      );
    case "push_unknown":
      return stage(
        "publish_uncertain",
        "publish",
        "uncertain",
        "Publication not confirmed",
        "It isn't confirmed the roles reached the project repository. Retry publication; it resends the same request.",
      );
    case "source_unknown":
      return stage(
        "publish_uncertain",
        "publish",
        "uncertain",
        "Publication not confirmed",
        "It isn't confirmed the project adopted the published roles. Retry publication; it checks the same request.",
      );
    case "conflict":
      return stage(
        "publish_refused",
        "publish",
        "blocked",
        "Publication conflicted",
        "The project's shared roles changed while publishing. Reopen setup and check the draft again.",
      );
    case "superseded":
      return stage(
        "publish_refused",
        "publish",
        "blocked",
        "A newer version replaced this one",
        "A later version of the project's shared roles replaced this publication. Reopen setup before continuing.",
      );
    case "refused":
      return stage(
        "publish_refused",
        "publish",
        "blocked",
        "Publication was refused",
        "Publishing was refused. See Technical details for the reason.",
      );
  }
}

/**
 * Derive the one current stage and its one next action from what the host
 * has recorded. It reads from the furthest step backwards, so a later fact
 * (an adopted publication) outranks an earlier unknown (an unread launch).
 */
export function projectTeamSetupStage(
  input: ProjectTeamSetupStageInput,
): ProjectTeamSetupStage {
  if (input.publication) return publicationStage(input, input.publication);
  if (input.snapshot === "saved" || input.snapshot === "recorded") {
    if (input.publicationBlocked === "missing_destination")
      return stage(
        "publish_blocked",
        "publish",
        "blocked",
        "Publishing is blocked",
        "The project's shared roles location couldn't be identified, so publishing is blocked rather than replacing existing roles.",
      );
    if (input.publicationBlocked === "missing_base")
      return stage(
        "publish_blocked",
        "publish",
        "blocked",
        "Publishing is blocked",
        "The project's shared roles have no fixed base version. Refresh the project source, then reopen setup.",
      );
    if (input.publicationBlocked === "unavailable")
      return stage(
        "publish_blocked",
        "publish",
        "blocked",
        "Publishing is unavailable",
        "The project's publication choices couldn't be read. Reopen setup to try again.",
      );
    if (input.snapshot === "saved" && input.publication === undefined)
      return stage(
        "publish",
        "publish",
        "checking",
        "Publish project roles",
        "Checking the project's shared roles before publishing…",
      );
    return stage(
      "publish",
      "publish",
      "todo",
      "Publish project roles",
      "Publish the saved version to the project's shared roles.",
    );
  }
  if (input.snapshot === "checking")
    return stage(
      "check_and_save",
      "check_and_save",
      "checking",
      "Check and save the draft",
      "Checking the saved version…",
    );
  // A fresh check outranks the failed re-check: the user is already fixing it.
  if (input.snapshot === "unverified" && !input.validation)
    return stage(
      "saved_version_unverified",
      "check_and_save",
      "blocked",
      "The saved version couldn't be re-checked",
      "The checked version saved earlier couldn't be re-checked on this computer. Check the draft and save it again.",
    );
  if (input.validation?.valid)
    return stage(
      "check_and_save",
      "check_and_save",
      "todo",
      "Save the checked version",
      "Save the checked version so later edits can't change what gets published.",
    );
  if (input.validation)
    return stage(
      "draft_invalid",
      "check_and_save",
      "blocked",
      "The draft needs fixes",
      "Fix the problems listed under Check draft, then check again.",
    );
  const launch = input.authoringLaunch;
  if (launch === undefined)
    return stage(
      "author",
      "author",
      "checking",
      "Author project roles",
      "Checking this draft's saved progress…",
    );
  if (launch === "unreadable")
    return stage(
      "authoring_uncertain",
      "author",
      "uncertain",
      "Authoring status unknown",
      "This draft's saved authoring progress couldn't be read. Check authoring status.",
    );
  if (launch === null)
    return stage(
      "author",
      "author",
      "todo",
      "Author project roles",
      "Start an authoring session so an agent adapts the draft roles to this project.",
    );
  switch (launch.status) {
    case "created":
      return stage(
        "check_and_save",
        "check_and_save",
        "todo",
        "Check the draft",
        "When the authoring session has finished editing, check the draft.",
      );
    case "awaiting_receipt":
      return stage(
        "authoring_uncertain",
        "author",
        "working",
        "Authoring session starting",
        "Waiting for the authoring session to confirm it started. Check authoring status.",
      );
    case "prepared":
    case "ambiguous":
      return stage(
        "authoring_uncertain",
        "author",
        "uncertain",
        "Authoring request not confirmed",
        "It isn't confirmed the authoring session started. Check authoring status, or retry the same saved request.",
      );
    case "initial_turn_failed":
      return stage(
        "authoring_failed",
        "author",
        "blocked",
        "Authoring didn't finish",
        "The authoring session started but its first turn failed. Open the session to see why, or check the draft if you edited it yourself.",
      );
    case "failed":
      return stage(
        "authoring_failed",
        "author",
        "blocked",
        "Authoring was refused",
        "The authoring request was refused. If you edited the draft yourself, check it next.",
      );
  }
}

/** Index of a step in the stepper, for "is this step before the current one". */
export function projectTeamSetupStepIndex(step: ProjectTeamSetupStep): number {
  return PROJECT_TEAM_SETUP_STEPS.indexOf(step);
}
