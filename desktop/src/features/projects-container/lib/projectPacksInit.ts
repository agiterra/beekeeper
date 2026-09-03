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
 */
import { invokeTauri } from "@/shared/api/tauri";

/** The host command this module calls. */
export const PROJECT_PACKS_INIT_COMMAND = "project_packs_init";

/** The wire facts the Packs panel prints, out of everything the host returns. */
export type ProjectPacksInitResult = {
  /** The new packs repository's `30617:<owner-hex>:<id>` coordinate. */
  repoRef: string;
  /**
   * The published `30624` source event's id, or `null` when the host withheld
   * it because the seed push did not reach the relay.
   */
  sourceEventId: string | null;
  /** The commit the shipped packs were seeded as, on the new repository. */
  seedCommitSha: string;
  /**
   * The relay's `30618` push-record event id for that seed commit, or `null`
   * — the push did not land, or the relay had not published the record yet.
   */
  pushRecordEventId: string | null;
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
    typeof record.seedCommitSha === "string" &&
    isOptionalId(record.pushRecordEventId)
  );
}

/**
 * Read the four facts this screen prints out of the host's answer.
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
    pushRecordEventId: value.pushRecordEventId,
  };
}

/**
 * Create this project's own packs repository, seeded from the shipped
 * defaults, and publish its `30624` source — one host call, the wire facts
 * back. Throws on any failure (a push failure, a relay refusal, a build with
 * no host); the caller shows the exact message, never guesses at partial
 * success.
 */
export async function projectPacksInit(input: {
  projectRef: string;
}): Promise<ProjectPacksInitResult> {
  return decodeProjectPacksInitResult(
    await invokeTauri(PROJECT_PACKS_INIT_COMMAND, {
      projectRef: input.projectRef,
    }),
  );
}
