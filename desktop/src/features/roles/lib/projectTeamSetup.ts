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
    /** The journal's reserved lead identity; `null` when no lead is installed. */
    leadPubkey: string | null;
    message: string | null;
  };
};

/** The exact brief the host writes for, and sends to, the authoring agent. */
export type ProjectTeamSetupBrief = { text: string };

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
    return "Open a saved project to set up its roles.";
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

/**
 * One plain sentence per native `SetupError.code`, each naming what the user
 * can do next. The native message is never dropped: it stays as `detail`.
 */
const FAILURE_SUMMARY: Record<string, string> = {
  existing_authoring:
    "An authoring session is already saved for this draft. Check authoring status to continue it.",
  existing_draft:
    "This project already has a saved draft. Continue that draft instead of preparing a new one.",
  filesystem:
    "A local file couldn't be read or written. Check that the folder still exists and this app can access it, then try again.",
  invalid_authoring:
    "The saved authoring request doesn't match this draft. Check authoring status before retrying.",
  invalid_draft:
    "The draft has problems. Check the draft and fix what it lists.",
  invalid_input: "Some setup details aren't valid. Review them and try again.",
  invalid_publication:
    "The saved publication doesn't match this draft. Reopen setup before retrying.",
  invalid_setup_actor:
    "The setup agent identity on this computer doesn't match this draft. Reopen setup before retrying.",
  invalid_setup_launch:
    "The saved authoring request can't be used as recorded. Check authoring status before retrying.",
  invalid_snapshot:
    "The saved checked version no longer matches its files. Check the draft and save it again.",
  publication_unavailable:
    "This step didn't complete. Details are below; retrying resends the same saved request when one exists.",
  scope_changed:
    "The community or project changed since setup opened. Reopen setup from the right project.",
  setup_launch_unavailable:
    "This step didn't complete. Details are below; retrying resends the same saved request when one exists.",
  source_changed:
    "The project's shared roles changed since this draft was checked. Reopen setup, then check the draft again.",
};

/** A setup failure as the UI shows it: a plain summary plus the raw detail. */
export type ProjectTeamSetupFailure = {
  summary: string;
  /** The native or thrown message, when it differs from the summary. */
  detail: string | null;
  code: string | null;
};

function errorPayload(error: unknown): unknown {
  return typeof error === "object" && error !== null && "payload" in error
    ? error.payload
    : null;
}

function setupErrorCode(value: unknown): string | null {
  return typeof value === "object" &&
    value !== null &&
    "code" in value &&
    typeof value.code === "string"
    ? value.code
    : null;
}

/** Map a native `SetupError {code, message}` (or any thrown value) for display. */
export function projectTeamSetupFailure(
  error: unknown,
): ProjectTeamSetupFailure {
  const message = projectTeamSetupError(error);
  // `invokeTauri` wraps a native refusal in `TauriInvokeError`, keeping the
  // original `{code, message}` as `payload`; a direct object carries `code`.
  const code = setupErrorCode(error) ?? setupErrorCode(errorPayload(error));
  const summary = code ? FAILURE_SUMMARY[code] : undefined;
  if (!summary) return { summary: message, detail: null, code };
  return { summary, detail: message === summary ? null : message, code };
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
