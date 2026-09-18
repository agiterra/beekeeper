/**
 * A project's two repositories, created with the project (spec § 4.11).
 *
 * The host command is `project_agents_init`
 * (`desktop/src-tauri/src/managed_agents/agents_repo.rs`): preflight both
 * ids community-wide, announce `<slug>` (empty) and
 * `<slug>-beekeeper-agents`, seed the latter from this build's shipped role
 * templates by reference, push `main`, and publish the kind:30624 pinned
 * `ref: refs/heads/main, path: .`. It is idempotent — **Finish setup** in
 * Project settings runs the same command, and whatever already exists under
 * the viewer's key is reused — and it never says "created" over a
 * repository the push did not fill: `complete` and `gap` are the host's own
 * verdict, and this reader prints them rather than composing a happier one.
 */
import { invokeTauri } from "@/shared/api/tauri";

/** The host command this module calls. */
export const PROJECT_AGENTS_INIT_COMMAND = "project_agents_init";

/** Suffix the agents repository id carries. */
export const AGENTS_REPO_SUFFIX = "-beekeeper-agents";

/** Longest repository id the relay's git routes accept. */
const REPO_ID_MAX_LENGTH = 64;

/** Everything `project_agents_init` reports. */
export type ProjectAgentsInitResult = {
  projectRef: string;
  codeRepoRef: string;
  codeRepoId: string;
  codeAnnouncementEventId: string | null;
  codeRepoExisted: boolean;
  agentsRepoRef: string;
  agentsRepoId: string;
  agentsCloneUrl: string;
  agentsAnnouncementEventId: string | null;
  agentsRepoExisted: boolean;
  branch: string;
  roles: string[];
  seedCommitSha: string | null;
  seedError: string | null;
  seedSkipped: boolean;
  pushed: boolean;
  pushError: string | null;
  pushRecordEventId: string | null;
  sourceEventId: string | null;
  sourceExisted: boolean;
  publicationError: string | null;
  commitIdentityName: string;
  commitIdentityEmail: string;
  agentsAnnouncementWithdrawnEventId: string | null;
  agentsAnnouncementWithdrawalError: string | null;
  /** Both repositories announced, the seed on the relay, the source set. */
  complete: boolean;
  /** One sentence naming what is missing when `complete` is `false`. */
  gap: string | null;
};

function isOptionalString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isProjectAgentsInitResult(
  value: unknown,
): value is ProjectAgentsInitResult {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record.projectRef === "string" &&
    typeof record.codeRepoRef === "string" &&
    typeof record.codeRepoId === "string" &&
    isOptionalString(record.codeAnnouncementEventId) &&
    typeof record.codeRepoExisted === "boolean" &&
    typeof record.agentsRepoRef === "string" &&
    typeof record.agentsRepoId === "string" &&
    typeof record.agentsCloneUrl === "string" &&
    isOptionalString(record.agentsAnnouncementEventId) &&
    typeof record.agentsRepoExisted === "boolean" &&
    typeof record.branch === "string" &&
    Array.isArray(record.roles) &&
    record.roles.every((role) => typeof role === "string") &&
    isOptionalString(record.seedCommitSha) &&
    isOptionalString(record.seedError) &&
    typeof record.seedSkipped === "boolean" &&
    typeof record.pushed === "boolean" &&
    isOptionalString(record.pushError) &&
    isOptionalString(record.pushRecordEventId) &&
    isOptionalString(record.sourceEventId) &&
    typeof record.sourceExisted === "boolean" &&
    isOptionalString(record.publicationError) &&
    typeof record.commitIdentityName === "string" &&
    typeof record.commitIdentityEmail === "string" &&
    isOptionalString(record.agentsAnnouncementWithdrawnEventId) &&
    isOptionalString(record.agentsAnnouncementWithdrawalError) &&
    typeof record.complete === "boolean" &&
    isOptionalString(record.gap)
  );
}

/** Read the host's answer, refusing a shape this reader cannot vouch for. */
export function decodeProjectAgentsInitResult(
  value: unknown,
): ProjectAgentsInitResult {
  if (!isProjectAgentsInitResult(value)) {
    throw new Error(
      "native project-agents-init adapter returned a malformed response",
    );
  }
  return value;
}

/**
 * Create, or finish creating, the project's repositories. Throws only when
 * the host refused before signing anything (an id another key holds, a
 * project already pointed elsewhere, a relay that could not be read) or the
 * command itself failed; a step that did not land is a normal result with
 * `complete: false` and `gap` set.
 */
export async function projectAgentsInit(input: {
  projectRef: string;
}): Promise<ProjectAgentsInitResult> {
  return decodeProjectAgentsInitResult(
    await invokeTauri(PROJECT_AGENTS_INIT_COMMAND, {
      projectRef: input.projectRef,
    }),
  );
}

/**
 * Sync this computer's clone of the project's agents repository and record
 * it for the provider, which reads `actions.yml` and `team.yml` from its
 * fetched tip (spec § 4.11). Resolves `false` when the source is not an
 * agents repository (its path is not the repository root, or it pins a sha
 * rather than a branch) — nothing recorded, and the caller says so.
 */
export async function recordProjectAgentsRepo(input: {
  projectRef: string;
  repo: string;
  ref: string | null;
  sha: string | null;
  path: string;
}): Promise<boolean> {
  const recorded = await invokeTauri<unknown>("record_project_agents_repo", {
    projectRef: input.projectRef,
    repo: input.repo,
    gitRef: input.ref,
    sha: input.sha,
    path: input.path,
  });
  return recorded === true;
}

/**
 * `<slug>-beekeeper-agents`, derived exactly as the host derives it
 * (`agents_repo.rs`'s `default_agents_repo_id`): lowercased, sanitized to
 * `[a-z0-9._-]`, the suffix kept whole inside the 64-byte bound.
 */
export function defaultAgentsRepoId(projectSlug: string): string {
  const sanitized = projectSlug
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9._-]/g, "-")
    .replace(/^-+|-+$/g, "");
  const room = REPO_ID_MAX_LENGTH - AGENTS_REPO_SUFFIX.length;
  const head = sanitized.slice(0, room).replace(/-+$/g, "");
  return `${head}${AGENTS_REPO_SUFFIX}`;
}

/**
 * The sentence a toast or panel prints for what the host actually did.
 * `gap` is the host's own words; nothing here softens them.
 */
export function describeAgentsSetup(result: ProjectAgentsInitResult): string {
  if (result.complete) {
    const seed = result.seedSkipped
      ? "already seeded"
      : result.seedCommitSha
        ? `seeded as ${result.commitIdentityName}, commit ${result.seedCommitSha.slice(0, 8)}`
        : "seeded";
    return `Repositories ready: ${result.codeRepoId} (code) and ${result.agentsRepoId} (${seed}); the project's roles come from ${result.agentsRepoId} on ${result.branch}.`;
  }
  return `Not finished: ${result.gap ?? "unknown"}. Finish setup from Project settings → Packs.`;
}
