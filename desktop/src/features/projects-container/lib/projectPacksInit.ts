/**
 * "Create packs repository" — the LANE-L23 addendum's host command
 * (2026-09-03, "setup lives inside the app"): announce a `30617` packs
 * repository under the founder's own key, seed it from the app's shipped
 * packs with one signed commit pushed under the app identity's key via the
 * credential helper, and publish the project's `30624` source — all three
 * steps as one host operation, so a founder never has to shell out.
 *
 * The host command is `project_packs_init`
 * (`desktop/src-tauri/src/managed_agents/packs_repo.rs`). It answers with more
 * than this screen prints — the repository id, clone URL, announcement event,
 * branch, seeded roles, whether the push landed and its error — so this reader
 * takes the four facts the panel shows and lets the rest through untouched
 * rather than refusing a response it merely does not use.
 *
 * Two of those four are genuinely absent sometimes, and the type says so:
 * `sourceEventId` is withheld when the push did not land (the host will not
 * point a project at a repository with nothing in it), and
 * `pushRecordEventId` is the relay's own kind:30618, `null` until the relay
 * publishes it. Neither is ever fabricated, so neither is typed as a string
 * this screen could print blindly.
 *
 * LANE-L30 (2026-09-03, "one packs repository for all of agiterra; every
 * project points at it"): the id was previously derived from the project's
 * slug with no way to choose, which meant every project's "Create packs
 * repository" made its own repository even when the intent was to share one.
 * `repoId` and `name` are now caller-chosen — see {@link defaultPacksRepoId}
 * and {@link packsRepoIdError}, the same default and validation the Packs
 * settings screen offers before it ever calls this.
 */
import { invokeTauri } from "@/shared/api/tauri";

/** The host command this module calls. */
export const PROJECT_PACKS_INIT_COMMAND = "project_packs_init";

/**
 * The wire facts the Packs panel prints, out of everything the host returns.
 *
 * LANE-L31 (2026-09-03, Finding 66): a seed or push failure is no longer a
 * thrown error with git's raw stderr as the message — the host reports it as
 * a normal result, because the announcement (`announcementEventId`) had
 * already landed and there is real data to show. `seedCommitSha` and
 * `sourceEventId` are `null` in that case; `seedError` names why, and
 * `announcementWithdrawnEventId` / `announcementWithdrawalError` say whether
 * the stray announcement was rolled back. The UI composes its own sentence
 * from these fields rather than printing `seedError` directly — see
 * `ProjectPacksSettingsSection.tsx`'s result panel.
 */
export type ProjectPacksInitResult = {
  /** The new packs repository's `30617:<owner-hex>:<id>` coordinate. */
  repoRef: string;
  /**
   * The published `30624` source event's id, or `null` when the host withheld
   * it because the seed or push did not land.
   */
  sourceEventId: string | null;
  /**
   * The commit the shipped packs were seeded as, or `null` when seeding
   * itself never produced one — see {@link seedError}.
   */
  seedCommitSha: string | null;
  /**
   * The seed step's own words when it failed before there was anything to
   * push. `null` when seeding succeeded (a push failure afterwards is a
   * separate fact the host does not surface to this screen's four values).
   */
  seedError: string | null;
  /** Display name the seed commit was (or would have been) authored as. */
  commitIdentityName: string;
  /** Email the seed commit was (or would have been) authored as. */
  commitIdentityEmail: string;
  /**
   * The relay's `30618` push-record event id for that seed commit, or `null`
   * — the push did not land, or the relay had not published the record yet.
   */
  pushRecordEventId: string | null;
  /**
   * Event id of the kind:5 withdrawing the announcement, published when the
   * seed or push failed after the announcement had already landed. `null`
   * when nothing needed withdrawing.
   */
  announcementWithdrawnEventId: string | null;
  /**
   * The withdrawal's own words when publishing the tombstone itself failed.
   * `repoRef` is the coordinate to delete by hand in that case
   * (`bee repos delete`).
   */
  announcementWithdrawalError: string | null;
};

/** `true` for a string, or for `null` where the host may honestly have none. */
function isOptionalId(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isProjectPacksInitResult(
  value: unknown,
): value is ProjectPacksInitResult {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record.repoRef === "string" &&
    isOptionalId(record.sourceEventId) &&
    isOptionalId(record.seedCommitSha) &&
    isOptionalId(record.seedError) &&
    typeof record.commitIdentityName === "string" &&
    typeof record.commitIdentityEmail === "string" &&
    isOptionalId(record.pushRecordEventId) &&
    isOptionalId(record.announcementWithdrawnEventId) &&
    isOptionalId(record.announcementWithdrawalError)
  );
}

/**
 * Read the facts this screen prints out of the host's answer.
 *
 * Every key is required and typed; keys beyond them are the host's business
 * and are dropped here rather than refused, so `packs_repo.rs` can report more
 * about a push without this reader calling the response malformed.
 */
export function decodeProjectPacksInitResult(
  value: unknown,
): ProjectPacksInitResult {
  if (!isProjectPacksInitResult(value)) {
    throw new Error(
      "native project-packs-init adapter returned a malformed response",
    );
  }
  return {
    repoRef: value.repoRef,
    sourceEventId: value.sourceEventId,
    seedCommitSha: value.seedCommitSha,
    seedError: value.seedError,
    commitIdentityName: value.commitIdentityName,
    commitIdentityEmail: value.commitIdentityEmail,
    pushRecordEventId: value.pushRecordEventId,
    announcementWithdrawnEventId: value.announcementWithdrawnEventId,
    announcementWithdrawalError: value.announcementWithdrawalError,
  };
}

/**
 * Create this project's own packs repository, seeded from the shipped
 * defaults, and publish its `30624` source — one host call, the wire facts
 * back. Throws on any failure (a push failure, a relay refusal, a build with
 * no host); the caller shows the exact message, never guesses at partial
 * success.
 *
 * `repoId` and `name` are both optional — omitted, the host falls back to
 * `<project-slug>-packs` and, for the name, to whatever the repository id
 * resolved to (`packs_repo.rs`'s own defaults, mirrored so the two never
 * drift). The Packs settings screen always sends both explicitly (LANE-L30);
 * the fields stay optional here for any other caller (tests included).
 */
export async function projectPacksInit(input: {
  projectRef: string;
  repoId?: string;
  name?: string;
}): Promise<ProjectPacksInitResult> {
  return decodeProjectPacksInitResult(
    await invokeTauri(PROJECT_PACKS_INIT_COMMAND, {
      projectRef: input.projectRef,
      repoId: input.repoId,
      name: input.name,
    }),
  );
}

/**
 * The one sentence `ProjectPacksSettingsSection.tsx`'s result panel prints
 * for what `project_packs_init` actually did — LANE-L31 (Finding 66):
 * before this, a seed failure threw git's own raw stderr ("Author identity
 * unknown … fatal: unable to auto-detect email address") straight at the
 * viewer, with no mention of the identity the host tried to use or of the
 * stray announcement it had already published. This function says what
 * failed, which identity it would have used, and what was withdrawn — the
 * raw `seedError` stays on the result for a "details" disclosure, never
 * inlined here.
 */
export function describeSeedOutcome(result: ProjectPacksInitResult): string {
  const identity = `${result.commitIdentityName} <${result.commitIdentityEmail}>`;
  if (result.seedCommitSha !== null) {
    return `Seeded as ${identity}, commit ${result.seedCommitSha.slice(0, 8)}.`;
  }
  const withdrawal =
    result.announcementWithdrawnEventId !== null
      ? `the announcement was withdrawn (${result.announcementWithdrawnEventId.slice(0, 8)})`
      : result.announcementWithdrawalError !== null
        ? `the announcement could not be withdrawn — delete ${result.repoRef} by hand`
        : "no announcement needed withdrawing";
  return `Announced ${result.repoRef}, but seeding failed as ${identity}; ${withdrawal}.`;
}

/** Longest repository id the relay's git routes and the CLI both accept. */
const REPO_ID_MAX_LENGTH = 64;

/** Suffix this screen's default id appends to a project's slug. */
const PACKS_REPO_SUFFIX = "-packs";

/**
 * The repository id a project's packs get by default: `<slug>-packs`,
 * lowercased and sanitized the same way `packs_repo.rs`'s
 * `default_packs_repo_id` derives it host-side, so the value this field
 * starts with is exactly the value the host would have picked on its own —
 * editable from there, never a guess this screen alone believes in.
 */
export function defaultPacksRepoId(projectSlug: string): string {
  const sanitized = projectSlug
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9._-]/g, "-")
    .replace(/^-+|-+$/g, "");
  const room = REPO_ID_MAX_LENGTH - PACKS_REPO_SUFFIX.length;
  const head = sanitized.slice(0, room).replace(/-+$/g, "");
  return `${head}${PACKS_REPO_SUFFIX}`;
}

/**
 * `null` when `value` is a valid `30617` `d` tag; otherwise the one sentence
 * explaining why, for the field's own error text.
 *
 * Mirrors the CLI's `validate_repo_id`
 * (`crates/buzz-cli/src/commands/repos.rs` via `crates/buzz-cli/src/validate.rs`)
 * restricted to the lowercase subset this screen's own default always
 * produces: lowercase ASCII letters, digits, `.`, `_`, `-`; 1–64 characters;
 * no leading `.` or `-`; no `..`. A stricter subset of the wire rule can
 * never be refused by the host that mirrors the wider one — this field would
 * rather ask for `agiterra-packs` than accept `Agiterra..Packs` and let the
 * host be the one to say no.
 */
export function packsRepoIdError(value: string): string | null {
  if (value.length === 0) {
    return "Repository id cannot be empty.";
  }
  if (value.length > REPO_ID_MAX_LENGTH) {
    return `Repository id must be ${REPO_ID_MAX_LENGTH} characters or fewer.`;
  }
  if (value.startsWith(".")) {
    return "Repository id must not start with '.'.";
  }
  if (value.startsWith("-")) {
    return "Repository id must not start with '-'.";
  }
  if (value.includes("..")) {
    return "Repository id must not contain '..'.";
  }
  if (!/^[a-z0-9._-]+$/.test(value)) {
    return "Repository id may only contain lowercase letters, digits, '.', '_', and '-'.";
  }
  return null;
}
