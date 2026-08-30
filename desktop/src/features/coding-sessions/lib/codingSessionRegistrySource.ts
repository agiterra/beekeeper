/**
 * Where the hire host gets `team/model-registry.yaml` from.
 *
 * The router cannot route without it: the decision is *registry ∩ live
 * catalog*, and a host with no registry has no eligible set at all. Until
 * 2026-08-30 this app had no way to read a file out of a project checkout —
 * the only project-file access it has is `scanProjectRolePacks` /
 * `pickCrewRolePacksDirectory` (`shared/api/tauriTeams.ts:331`, `:350`), which
 * answer with a role, a name and a directory and never with content — so every
 * routed hire on this build was refused `HIRE_NO_ROUTE  registry not readable
 * on this host` (ledger draft 97).
 *
 * This module is the seam that closes it, and it is deliberately one function.
 * The reader itself is a read-only Tauri command scoped to the project
 * checkout the Agents tab already resolves; installing it is a separate lane's
 * job, and until it is installed this returns the honest `unreadable` answer
 * with the exact path it would have read. **The fallback is never a registry
 * compiled into the app.** Nobody could check such a copy against the file the
 * team edits, and every decision it produced would cite a version that was
 * never on disk — a hardcoded opinion wearing the registry's name.
 */
import {
  describeUnreadableModelRegistry,
  MODEL_REGISTRY_PROJECT_PATH,
} from "./codingSessionRegistryAccess";
import type { CodingSessionRegistrySource } from "./codingSessionHireRouting";

export { MODEL_REGISTRY_PROJECT_PATH };

/**
 * Reads `team/model-registry.yaml` out of one project's checkout.
 *
 * Takes the project coordinate rather than a path so the caller cannot point
 * it at an arbitrary directory: resolving a coordinate to the checkout this
 * computer recorded for it is the reader's job, and a reader that took a path
 * would be a general file-read command wearing a routing name.
 */
export type ModelRegistryReader = (
  projectRef: string,
) => Promise<CodingSessionRegistrySource>;

let installedReader: ModelRegistryReader | null = null;

/**
 * Install the real reader.
 *
 * Called once, at the seam between this feature and the Tauri surface, so this
 * module never imports a backend command and stays loadable under plain
 * `node --test`. Passing `null` uninstalls it, which is what a test that wants
 * the honest unreadable answer does.
 */
export function setModelRegistryReader(
  reader: ModelRegistryReader | null,
): void {
  installedReader = reader;
}

/**
 * This host's registry for one project, or the reason it has none.
 *
 * Total: a reader that throws is reported as `unreadable` with what it said,
 * never as an empty registry. An empty registry would route nothing while
 * looking like a registry that offers nothing, and those are different facts.
 */
export async function readModelRegistry(
  projectRef: string | null,
): Promise<CodingSessionRegistrySource> {
  if (projectRef === null || projectRef.trim().length === 0) {
    return {
      kind: "unreadable",
      why:
        `no project is resolved on this surface, so there is no checkout to ` +
        `read ${MODEL_REGISTRY_PROJECT_PATH} from. Open the project, or route ` +
        "from the checkout with the CLI.",
    };
  }
  if (installedReader === null) {
    return describeUnreadableModelRegistry(null);
  }
  try {
    return await installedReader(projectRef);
  } catch (error) {
    const said = error instanceof Error ? error.message.trim() : String(error);
    return {
      kind: "unreadable",
      why:
        `reading ${MODEL_REGISTRY_PROJECT_PATH} for ${projectRef} failed: ` +
        `${said.length > 0 ? said : "the reader gave no reason"}.`,
    };
  }
}
