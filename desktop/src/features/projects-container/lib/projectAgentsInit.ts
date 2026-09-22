/**
 * A project's two repositories, created with the project (spec § 4.11).
 *
 * The host command is `project_agents_init`
 * (`desktop/src-tauri/src/managed_agents/agents_repo.rs`): preflight both
 * ids community-wide, announce `<slug>` and `<slug>-beekeeper-agents`, seed
 * the code repository with one commit on `main` (a repository with no commit
 * cannot host a worktree), seed the agents repository from this build's
 * shipped role templates by reference, push `main`, publish the kind:30624
 * pinned `ref: refs/heads/main, path: .`, clone `<slug>` under
 * `checkoutParent` (or the host's default repos root) and record it as the
 * project's folder, and put every project agent on the project roster as a
 * collaborator. It is idempotent — **Finish repository setup** in Project
 * settings runs the same command, and whatever already exists under the
 * viewer's key is reused — and it never says "created" over a repository
 * the push did not fill: `complete` and `gap` are the host's own verdict,
 * and this reader prints them rather than composing a happier one.
 */
import { invokeTauri } from "@/shared/api/tauri";

/** The host command this module calls. */
export const PROJECT_AGENTS_INIT_COMMAND = "project_agents_init";

/** Suffix the agents repository id carries. */
export const AGENTS_REPO_SUFFIX = "-beekeeper-agents";

/** The host command that names the default repos root. */
export const DEFAULT_REPOS_ROOT_COMMAND = "default_repos_root";

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
  /**
   * The repository coordinate this run moved the project OFF, when it was
   * asked to migrate one; `null` for an ordinary create or finish.
   */
  migratedFrom: string | null;
  /** The roles converted out of that repository, ascending. */
  migratedRoles: string[];
  /** What the conversion could not carry, one sentence per role. */
  migrationNotes: string[];
  /**
   * The relay refused the re-point because the project's source had moved
   * since it was read. Everything else stands; nothing was re-pointed.
   */
  sourceConflict: boolean;
  /**
   * The code repository is the one the project's own head names rather than
   * one derived from its slug — what a project created before the pivot has.
   */
  codeRepoAdopted: boolean;
  publicationError: string | null;
  commitIdentityName: string;
  commitIdentityEmail: string;
  agentsAnnouncementWithdrawnEventId: string | null;
  agentsAnnouncementWithdrawalError: string | null;
  /** Both repositories announced, the seed on the relay, the source set. */
  complete: boolean;
  /** One sentence naming what is missing when `complete` is `false`. */
  gap: string | null;
  /**
   * The project's default agents this computer installed from the seeded
   * team (spec § 4.11), in role order. Empty when the seed did not land or
   * the install failed — see `agentsError`.
   */
  agentsInstalled: InstalledDefaultAgent[];
  /** Why no agents were installed, when the seed landed and none were. */
  agentsError: string | null;
  /** The one commit the code repository was seeded with, or `null`. */
  codeSeedCommitSha: string | null;
  /** The code repository already had a commit, so no seed was pushed. */
  codeSeedSkipped: boolean;
  /** Why the code seed did not land, in the host's words. */
  codeSeedError: string | null;
  /**
   * Where the project's code repository is checked out on this computer —
   * the folder recorded as the project's, `null` when none was recorded.
   */
  checkoutPath: string | null;
  /** The clone was made by this run; `false` with a path means it was reused. */
  checkoutCloned: boolean;
  /** Why no checkout was recorded, in the host's words. */
  checkoutError: string | null;
  /** Agent pubkeys this run put on the project roster as collaborators. */
  rosterAdded: string[];
  /** Why the roster op did not land, in the host's words. */
  rosterError: string | null;
};

/** One default agent the create installed. */
export type InstalledDefaultAgent = {
  role: string;
  name: string;
  pubkey: string;
  /** Found already installed and refreshed rather than minted. */
  refreshed: boolean;
};

function isInstalledDefaultAgent(
  value: unknown,
): value is InstalledDefaultAgent {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record.role === "string" &&
    typeof record.name === "string" &&
    typeof record.pubkey === "string" &&
    typeof record.refreshed === "boolean"
  );
}

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
    isOptionalString(record.migratedFrom) &&
    Array.isArray(record.migratedRoles) &&
    record.migratedRoles.every((role) => typeof role === "string") &&
    Array.isArray(record.migrationNotes) &&
    record.migrationNotes.every((note) => typeof note === "string") &&
    typeof record.sourceConflict === "boolean" &&
    typeof record.codeRepoAdopted === "boolean" &&
    isOptionalString(record.publicationError) &&
    typeof record.commitIdentityName === "string" &&
    typeof record.commitIdentityEmail === "string" &&
    isOptionalString(record.agentsAnnouncementWithdrawnEventId) &&
    isOptionalString(record.agentsAnnouncementWithdrawalError) &&
    typeof record.complete === "boolean" &&
    isOptionalString(record.gap) &&
    Array.isArray(record.agentsInstalled) &&
    record.agentsInstalled.every(isInstalledDefaultAgent) &&
    isOptionalString(record.agentsError) &&
    isOptionalString(record.codeSeedCommitSha) &&
    typeof record.codeSeedSkipped === "boolean" &&
    isOptionalString(record.codeSeedError) &&
    isOptionalString(record.checkoutPath) &&
    typeof record.checkoutCloned === "boolean" &&
    isOptionalString(record.checkoutError) &&
    Array.isArray(record.rosterAdded) &&
    record.rosterAdded.every((pubkey) => typeof pubkey === "string") &&
    isOptionalString(record.rosterError)
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
  /**
   * The folder the code repository is cloned UNDER — the clone lands at
   * `<checkoutParent>/<slug>`. Omitted or `null`, the host uses its default
   * repos root (`defaultReposRoot`).
   */
  checkoutParent?: string | null;
  /**
   * Move the project off the role source it points at today. The id is the
   * kind:30624 the caller READ: the host refuses if the relay holds another
   * one, and publishes the new source conditionally on it, so a source that
   * moved is a refusal rather than a silent overwrite.
   */
  migrate?: { expectedSourceId: string; convert: boolean } | null;
}): Promise<ProjectAgentsInitResult> {
  return decodeProjectAgentsInitResult(
    await invokeTauri(PROJECT_AGENTS_INIT_COMMAND, {
      projectRef: input.projectRef,
      checkoutParent: input.checkoutParent ?? null,
      migrate: input.migrate ?? null,
    }),
  );
}

/**
 * The host's default repos root — the folder a project's code repository is
 * cloned under when no other is chosen (`~/.beekeeper/REPOS` on this build's
 * `project_repo_paths.rs`). Throws off-host; callers fall back to naming the
 * host default rather than inventing a path.
 */
export async function defaultReposRoot(): Promise<string> {
  const root = await invokeTauri<unknown>(DEFAULT_REPOS_ROOT_COMMAND);
  if (typeof root !== "string" || root.trim().length === 0) {
    throw new Error("native default-repos-root adapter returned no path");
  }
  return root;
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
 * Collapse per-role notes that say the same thing.
 *
 * Each note is `"<role>: <sentence>"`. Roles sharing a sentence fold into
 * one line naming them all, so a seven-role conversion reads as one fact
 * about seven roles rather than the same fact seven times. A sentence only
 * one role carries is printed as it stands, and a note in no such shape is
 * passed through untouched rather than reformatted into something it is not.
 */
export function foldMigrationNotes(notes: string[]): string {
  if (notes.length === 0) return "";
  const byReason = new Map<string, string[]>();
  const loose: string[] = [];
  for (const note of notes) {
    const split = note.indexOf(": ");
    if (split <= 0) {
      loose.push(note);
      continue;
    }
    const role = note.slice(0, split);
    const reason = note.slice(split + 2);
    byReason.set(reason, [...(byReason.get(reason) ?? []), role]);
  }
  const folded = [...byReason.entries()].map(
    ([reason, roles]) => `${roles.join(", ")}: ${reason}`,
  );
  return ` ${[...folded, ...loose].join(" ")}`;
}

/**
 * Whether a project's role source is its OWN agents repository — the flat
 * layout at the repository root, under the id this project's slug derives.
 *
 * The three states a settings panel has to tell apart: no source at all
 * (create), this (finish setup), and anything else (a pack-layout or
 * borrowed source, which only a deliberate migration moves).
 */
export function isProjectAgentsRepoSource(
  source: { repo: string; path: string; ref: string | null } | null,
  projectSlug: string,
): boolean {
  if (!source) return false;
  const id = source.repo.split(":")[2] ?? "";
  const path = source.path.trim();
  return (
    id === defaultAgentsRepoId(projectSlug) &&
    (path === "." || path === "") &&
    source.ref !== null
  );
}

/**
 * The sentence a toast or panel prints for what the host actually did.
 * `gap` is the host's own words; nothing here softens them.
 */
export function describeAgentsSetup(result: ProjectAgentsInitResult): string {
  const tail = ` ${describeCheckoutOutcome(result)} ${describeRosterOutcome(result)}`;
  if (result.complete) {
    const seed = result.seedSkipped
      ? "already seeded"
      : result.seedCommitSha
        ? `seeded as ${result.commitIdentityName}, commit ${result.seedCommitSha.slice(0, 8)}`
        : "seeded";
    const agents =
      result.agentsInstalled.length > 0
        ? ` ${result.agentsInstalled.length} default agents installed: ${result.agentsInstalled.map((agent) => agent.name).join(", ")}.`
        : result.agentsError
          ? ` No default agents were installed: ${result.agentsError}.`
          : "";
    return `Repositories ready: ${result.codeRepoId} (code) and ${result.agentsRepoId} (${seed}); the project's roles come from ${result.agentsRepoId} on ${result.branch}.${agents}${tail}`;
  }
  return `Not finished: ${result.gap ?? "unknown"}. Finish setup from Project settings → Packs.${tail}`;
}

/**
 * What a migration moved, for the panel that asked for one. Empty string
 * when this run migrated nothing, so a caller can print it unconditionally.
 */
export function describeMigrationOutcome(
  result: Pick<
    ProjectAgentsInitResult,
    "migratedFrom" | "migratedRoles" | "migrationNotes" | "sourceConflict"
  >,
): string {
  if (!result.migratedFrom) return "";
  const roles =
    result.migratedRoles.length > 0
      ? `${result.migratedRoles.length} roles converted (${result.migratedRoles.join(", ")})`
      : "no roles converted";
  const moved = result.sourceConflict
    ? "the project was NOT re-pointed: its source moved while this ran"
    : "the project now reads its roles from this repository";
  // One sentence per role, all identical, is seven copies of one fact.
  const notes = foldMigrationNotes(result.migrationNotes);
  return `From ${result.migratedFrom}: ${roles}; ${moved}.${notes}`;
}

/**
 * Where the project's code lives on this computer after this run — cloned
 * by it, reused, or not recorded and why. Never a path the host did not
 * report.
 */
export function describeCheckoutOutcome(
  result: Pick<
    ProjectAgentsInitResult,
    "checkoutPath" | "checkoutCloned" | "checkoutError"
  >,
): string {
  if (result.checkoutPath) {
    return result.checkoutCloned
      ? `Code cloned to ${result.checkoutPath} and recorded as this project's folder.`
      : `Code already checked out at ${result.checkoutPath}; recorded as this project's folder.`;
  }
  return `No folder was recorded for this project: ${result.checkoutError ?? "the host did not say why"}.`;
}

/** The code seed, in one clause: the commit, "already had commits", or the error. */
export function describeCodeSeedOutcome(
  result: Pick<
    ProjectAgentsInitResult,
    "codeSeedCommitSha" | "codeSeedSkipped" | "codeSeedError"
  >,
): string {
  if (result.codeSeedCommitSha) {
    return `seeded, commit ${result.codeSeedCommitSha.slice(0, 8)}`;
  }
  if (result.codeSeedSkipped) return "already had commits";
  return result.codeSeedError ?? "not seeded";
}

/**
 * The roster line: how many agents this run made project collaborators, or
 * the host's reason none were. "0 agents added" is said only when nothing
 * failed — an agent already on the roster is skipped, not added.
 */
export function describeRosterOutcome(
  result: Pick<ProjectAgentsInitResult, "rosterAdded" | "rosterError">,
): string {
  const count = result.rosterAdded.length;
  const added =
    count === 1
      ? "1 agent added to the project roster."
      : `${count} agents added to the project roster.`;
  if (result.rosterError) {
    return `${added} Roster error: ${result.rosterError}.`;
  }
  return added;
}
