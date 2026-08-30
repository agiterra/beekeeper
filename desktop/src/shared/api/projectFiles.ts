import { invokeTauri, TauriInvokeError } from "@/shared/api/tauri";
import type { ProjectFileRead } from "@/shared/api/types";

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
