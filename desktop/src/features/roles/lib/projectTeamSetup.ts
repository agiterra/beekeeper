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

/** A host-resolved publication target; the UI never constructs repository data. */
export type ProjectTeamSetupPublicationDestination = {
  repoRef: string;
  packPath: string;
  baseCommit: string | null;
  createAnnouncement: { name: string; description: string } | null;
};

/** Explicitly names the source head the host must compare before adoption. */
export type ProjectTeamSetupSourceExpectation =
  | { kind: "if_unset" }
  | { kind: "expected"; eventId: string };

/**
 * The host reads and verifies source state before producing these choices.
 * A null destination blocks publication; the UI never invents one.
 */
export type ProjectTeamSetupPublicationOptions = {
  currentSourceEventId: string | null;
  suggestedDestination: ProjectTeamSetupPublicationDestination | null;
  sourceExpectation: ProjectTeamSetupSourceExpectation;
  publication: ProjectTeamSetupPublication | null;
};

export type ProjectTeamSetupPublicationOutput = {
  kind: "snapshot";
  snapshotId: string;
};

/** Durable, host-observed publication state. It says nothing about local install. */
export type ProjectTeamSetupPublication = {
  publicationId: string;
  setupId: string;
  status:
    | "checking"
    | "candidate_prepared"
    | "push_unknown"
    | "pushed"
    | "source_unknown"
    | "adopted"
    | "superseded"
    | "conflict"
    | "refused";
  snapshotId: string;
  destination: ProjectTeamSetupPublicationDestination;
  sourceExpectation: ProjectTeamSetupSourceExpectation;
  candidateRef: string;
  candidateCommit: string | null;
  sourceEventId: string | null;
  message: string | null;
};

/** The source actually resolved for a publication, before local installation. */
export type ProjectTeamSetupActivationSource = {
  repoRef: string;
  commit: string;
  packPath: string;
};

/** One identity the host installed from the adopted project source. */
export type ProjectTeamSetupInstalledRole = {
  role: string;
  agentPubkey: string;
  packRef: PackRef;
};

/** Durable local installation and lead-launch observations for one publication. */
export type ProjectTeamSetupActivation = {
  source: ProjectTeamSetupActivationSource | null;
  installation: {
    status: "not_installed" | "installed" | "unknown" | "refused";
    installedRoles: ProjectTeamSetupInstalledRole[];
    message: string | null;
  };
  lead: {
    status:
      | "needs_channel"
      | "ready"
      | "starting"
      | "started"
      | "unknown"
      | "refused";
    channelId: string | null;
    sessionRef: string | null;
    message: string | null;
  };
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
