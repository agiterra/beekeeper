import * as React from "react";

import type {
  CodingSessionFileRef,
  CodingSessionFileRefWhere,
} from "@/shared/api/tauriCodingSessionFileRefs";

/**
 * What the message being rendered knows about the file paths in it (SV-32).
 *
 * A context, not a prop, for the same reason as `RedactionDictionaryContext`:
 * the `code` and `a` renderers sit inside markdown element trees cached
 * module-wide (`nodeCache.ts`, `runtimeContext.ts`), and only a context is
 * read at render time. **The default is `null`**, so channels, forums, DMs
 * and every other Markdown surface render inline code and links exactly as
 * before; only a coding-session answer wrapped in `FileRefProvider` ever
 * sees a chip.
 *
 * This value can hold an absolute path (`ref.fullPath`, for "Copy full
 * path"). It must never be read by anything that builds an event: a test
 * pins that no command or event builder imports this module.
 */
export type FileRefScopeState = {
  /**
   * `pending` until this computer has answered. A pending scope renders
   * plain code with no tooltip: nothing is claimed before it is known.
   */
  where: CodingSessionFileRefWhere | "pending" | "unknown";
  /** Why paths stay plain text here; `null` on this computer. */
  reason: string | null;
  /** Keyed by the candidate exactly as the agent wrote it. */
  refs: Readonly<Record<string, CodingSessionFileRef>>;
  /** Open in the default app. The host resolves `candidate` again. */
  open: (candidate: string) => Promise<void>;
  /** Reveal in the file manager. The host resolves `candidate` again. */
  reveal: (candidate: string) => Promise<void>;
};

export const FileRefContext = React.createContext<FileRefScopeState | null>(
  null,
);

/** The scope in force, or `null` outside a coding-session answer. */
export function useFileRefScope(): FileRefScopeState | null {
  return React.useContext(FileRefContext);
}

/** How one candidate renders under `scope`. */
export type FileRefPresentation =
  | { kind: "none" }
  | { kind: "chip"; ref: CodingSessionFileRef }
  | { kind: "plain"; reason: string };

/** The sentence for a path this computer looked for and did not find. */
export const FILE_REF_NOT_FOUND_REASON = "Not found in this session's folder";

/** The sentence for a path that resolves outside the session's folder. */
export const FILE_REF_OUTSIDE_FOLDER_REASON = "Outside this session's folder";

/**
 * Decide chip, plain-with-reason, or untouched for one candidate. Pure.
 *
 * Untouched (`none`) outside a scope, while pending, and for a candidate the
 * host was never asked about. On this computer a path that exists is a chip,
 * one the host looked for and did not find is plain code saying so.
 * Anywhere else it is plain code with the host's own reason.
 */
export function presentFileRef(
  scope: FileRefScopeState | null,
  candidate: string | null,
): FileRefPresentation {
  if (!scope || !candidate || scope.where === "pending")
    return { kind: "none" };
  if (scope.where !== "thisComputer") {
    return scope.reason
      ? { kind: "plain", reason: scope.reason }
      : { kind: "none" };
  }
  const ref = scope.refs[candidate];
  // Never asked (or not path-shaped to the host): no claim either way.
  if (!ref) return { kind: "none" };
  // Outside the folder it resolved against: disclosed, never launchable.
  if (ref.exists && ref.relativePath === null)
    return { kind: "plain", reason: FILE_REF_OUTSIDE_FOLDER_REASON };
  if (ref.exists) return { kind: "chip", ref };
  return { kind: "plain", reason: FILE_REF_NOT_FOUND_REASON };
}
