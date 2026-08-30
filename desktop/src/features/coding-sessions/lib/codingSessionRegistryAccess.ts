/**
 * Where the shared model registry lives, and why this app cannot read it yet.
 *
 * `team/model-registry.yaml` in the project checkout is the one registry the
 * router and the Agents badge are both about. This module is the single place
 * that says so, and the single place that says — out loud, with the path — why
 * this desktop has never loaded it.
 *
 * **The gap is real and it is not a rendering detail.** The app has exactly
 * two ways to touch a project's files, `scanProjectRolePacks` and
 * `pickCrewRolePacksDirectory` (`shared/api/tauriTeams.ts:331` and `:350`),
 * and both answer with a role, a name and a directory — never with file
 * content. Which project a Dashboard surface is looking at is already solved
 * (`features/agents/lib/rolePacksProject.ts`); reading a file out of it is
 * not. So both consumers refuse rather than guess:
 *
 * - the router refuses a routed hire `HIRE_NO_ROUTE`, naming this reason;
 * - the Agents badge renders "Registry: unknown (not readable)".
 *
 * The alternative — a registry copy compiled into the app — would be worse
 * than the gap. Nobody could check it against the file the team edits, and
 * every routing decision it produced would cite a version that was never on
 * disk. A registry the operator cannot see is not a registry, it is a
 * hardcoded opinion wearing one's name.
 */
import type { CodingSessionRegistrySource } from "./codingSessionHireRouting";

/** The registry's path within a project checkout. Spelled once. */
export const MODEL_REGISTRY_PROJECT_PATH = "team/model-registry.yaml";

/**
 * The `unreadable` source this host really holds, naming the file it would
 * have read.
 *
 * `checkoutPath` is this computer's most recent checkout directory, when it
 * has recorded one. Naming it matters: an operator told only "not readable"
 * has nowhere to go, and an operator told the full path can open the file,
 * see that it is there, and understand that the missing piece is a reader
 * rather than the registry.
 */
export function describeUnreadableModelRegistry(
  checkoutPath: string | null,
): Extract<CodingSessionRegistrySource, { kind: "unreadable" }> {
  const where =
    checkoutPath === null
      ? MODEL_REGISTRY_PROJECT_PATH
      : `${checkoutPath.replace(/\/+$/, "")}/${MODEL_REGISTRY_PROJECT_PATH}`;
  return {
    kind: "unreadable",
    why:
      `The router reads ${where}, and this app has no command that reads a ` +
      "project file — the only project-file access it has returns directory " +
      "listings. Route from the checkout with the CLI, or add a reader.",
  };
}
