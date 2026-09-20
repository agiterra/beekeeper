import { invokeTauri, TauriInvokeError } from "@/shared/api/tauri";
import type {
  ModelRegistryHostRead,
  ProjectFileRead,
} from "@/shared/api/types";

/**
 * Reading one allowlisted file out of a project checkout.
 *
 * The host command is deliberately narrow — read-only, one allowlisted
 * relative path, containment-checked after symlink resolution, size-capped.
 * See `desktop/src-tauri/src/commands/project_files.rs` for the whole policy.
 * This module is only the invoke boundary and the refusal it hands back.
 */

/** Why a read was refused, as the host worded it. */
export type ProjectFileRefusal = {
  /** Stable machine-readable code, e.g. `file-missing`, `outside-checkout`. */
  code: string;
  /** The sentence to show a person. Always names the path or the limit. */
  message: string;
};

function isProjectFileRefusal(value: unknown): value is ProjectFileRefusal {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as { code?: unknown }).code === "string" &&
    typeof (value as { message?: unknown }).message === "string"
  );
}

/**
 * Read `relativePath` out of `projectRef`'s checkout on this computer.
 *
 * Rejects rather than resolves on every refusal, so a caller cannot mistake a
 * missing registry for an empty one. The rejection is a `ProjectFileRefusal`
 * when the host produced one, and an `Error` otherwise (no Tauri host, IPC
 * failure) — both carry a sentence.
 */
export async function readProjectFile(
  projectRef: string,
  relativePath: string,
): Promise<ProjectFileRead> {
  try {
    return await invokeTauri<ProjectFileRead>("read_project_file", {
      projectRef,
      relativePath,
    });
  } catch (error) {
    if (
      error instanceof TauriInvokeError &&
      isProjectFileRefusal(error.payload)
    ) {
      throw error.payload;
    }
    throw error;
  }
}

/**
 * Read a project's model registry from this computer.
 *
 * The host resolves *where* — the project's agents repository first (spec
 * § 4.11), then `team/model-registry.yaml` in its code checkout — and says
 * which copy answered. Rejects rather than resolves when there is none, with
 * a sentence naming every place it looked; a caller that treated an absent
 * registry as an empty one would route on nothing and call it a decision.
 */
export async function readModelRegistrySource(
  projectRef: string,
): Promise<ModelRegistryHostRead> {
  try {
    return await invokeTauri<ModelRegistryHostRead>("read_model_registry", {
      projectRef,
    });
  } catch (error) {
    if (
      error instanceof TauriInvokeError &&
      isProjectFileRefusal(error.payload)
    ) {
      throw error.payload;
    }
    throw error;
  }
}
