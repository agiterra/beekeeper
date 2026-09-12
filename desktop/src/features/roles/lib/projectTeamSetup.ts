import type { CodingSessionCommandTarget } from "@/features/coding-sessions/lib/codingSessionCommand";
import type { PackRef } from "@/features/coding-sessions/lib/codingSessionPackRef";

/** Host-local setup work; preparing a draft does not publish a pack source. */
export type ProjectTeamSetupDraft = {
  setupId: string;
  projectRef: string;
  projectDirectory: string;
  draftDirectory: string;
  rolesDirectory: string;
  status: "draft";
  intent: string;
  ownerPubkey: string;
  relayUrl: string;
  roles: string[];
  createdAt: string;
  /** Host-recorded checked version, reverified before the UI describes it. */
  latestSnapshotId?: string | null;
};

/** Local pack-loader checks, independent of publication or runtime adoption. */
export type ProjectTeamSetupValidation = {
  setupId: string;
  status: "draft";
  valid: boolean;
  roles: string[];
  diagnostics: { level: "error" | "warning"; message: string }[];
};

/** A checked copy of exact draft bytes; not a published project revision. */
export type ProjectTeamSetupSnapshot = {
  setupId: string;
  snapshotId: string;
  rolesDirectory: string;
  manifestPath: string;
  roles: string[];
};

/** Durable IDs; reserving them neither publishes nor launches. */
export type ProjectTeamSetupAuthoringReservation = {
  authoringId: string;
  sessionRef: string;
  channelId: string;
  createCommandId: string;
  status: "reserved";
  genesisEventId: string;
};

/** Host journal plus a verified provider receipt, when one has been observed. */
export type ProjectTeamSetupLaunch = {
  setupId: string;
  sessionRef: string;
  channelId: string;
  createCommandId: string;
  providerPubkey: string;
  providerInstanceRef: string;
  runtime: string;
  model: string;
  actorPubkey: string;
  packRef: PackRef;
  status:
    | "prepared"
    | "awaiting_receipt"
    | "ambiguous"
    | "created"
    | "initial_turn_failed"
    | "failed";
  message?: string | null;
  target?: CodingSessionCommandTarget | null;
  receiptEventId?: string | null;
};

/** Explain the prerequisite the user can resolve before preparing a draft. */
export function projectTeamSetupBlocker(input: {
  projectRef: string;
  relayUrl: string;
  intent: string;
  projectDirectory: string;
}): string | null {
  if (!/^30621:[a-f0-9]{64}:[^:]+$/i.test(input.projectRef.trim())) {
    return "Open a saved project to set up its team.";
  }
  if (!input.relayUrl.trim())
    return "Connect to this project's community first.";
  if (!input.intent.trim())
    return "Describe what this project should accomplish.";
  if (new TextEncoder().encode(input.intent).length > 16 * 1024) {
    return "Shorten the project intent to 16 KiB or less.";
  }
  if (!input.projectDirectory.trim())
    return "Choose this project's local repository folder.";
  return null;
}

/** Preserve native structured refusal messages instead of showing Object. */
export function projectTeamSetupError(error: unknown): string {
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return error.message;
  }
  return typeof error === "string"
    ? error
    : "Project setup could not be completed. Try again.";
}

/** Instructions for the authoring execution; paths are local to its host. */
export function projectTeamSetupBrief(draft: ProjectTeamSetupDraft): string {
  return [
    "Build a useful baseline team for this project.",
    `Project: ${draft.projectRef}`,
    `Intent: ${draft.intent}`,
    `Inspect the project repository at ${draft.projectDirectory}. Read its contributor instructions, product documents, code and actual build/test commands.`,
    `Write project-specific role packs and skills only under ${draft.rolesDirectory}. This isolated draft begins with neutral defaults; it has not been published.`,
    "Adapt the role roster to the project: keep a lead, retain the identity of any starting role you keep, and add or remove other roles when the work warrants it. Cover leadership, implementation and verification responsibilities with the smallest useful team. Existing test agents are not project requirements.",
    "Give the lead responsibility for maintaining this shared project baseline as evidence changes. Keep procedures grounded in this repository, and distinguish verified commands from unknowns.",
    "Do not include credentials, personal configuration or machine-specific paths in the role packs. Do not invent tool access, spending permission, installed providers or approval requirements.",
    "Validate the complete pack structure and report changed roles, skills, evidence and unresolved limitations. Draft validation, publication and execution adoption are separate facts.",
  ].join("\n\n");
}
