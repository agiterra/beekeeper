import { Files } from "lucide-react";

import {
  readCodingSessionFilesExtension,
  useCodingSessionFilesExtension,
} from "../CodingSessionFilesAgentsRepoRead";
import { CodingSessionFilesPanel } from "../CodingSessionFilesPanel";
import type {
  CodingSessionSurfaceAvailability,
  CodingSessionSurfaceCtx,
} from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";
import { CodingSessionSurfacePlaceholder } from "./CodingSessionSurfaceDevicePlaceholder";

/**
 * Files: when it can open, or the sentence why not (§3, SV-23): "there is an
 * agents repo, or a local tree". A tree this computer has opens it; so does
 * a project whose agents repository is there or not yet known (the panel
 * then says what the read found). Only a project whose read answered "no
 * agents repository", on a computer without the tree, is dimmed.
 */
export function codingSessionSurfaceFilesAvailability(
  ctx: CodingSessionSurfaceCtx,
): CodingSessionSurfaceAvailability {
  if (ctx.tree.available) return { available: true };
  const agentsRepo =
    readCodingSessionFilesExtension(ctx.extensions.files)?.agentsRepo ??
    "unknown";
  return ctx.projectRef !== null && agentsRepo !== "absent"
    ? { available: true }
    : {
        available: false,
        reason: "This session has no project files to show.",
      };
}

/** The Files surface: the working tree (read-only) and the agents repo. */
export function CodingSessionSurfaceFilesPanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const availability = codingSessionSurfaceFilesAvailability(ctx);
  if (!availability.available) {
    return (
      <CodingSessionSurfacePlaceholder
        icon={Files}
        id="files"
        label="Files"
        reason={availability.reason}
      />
    );
  }
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-available="true"
      data-testid="coding-session-surface-panel-files"
    >
      <CodingSessionFilesPanel ctx={ctx} />
    </div>
  );
}

export const codingSessionSurfaceFiles: CodingSessionSurfaceDefinition = {
  id: "files",
  label: "Files",
  icon: Files,
  shortcut: "F",
  order: 40,
  placement: "right",
  lenses: ["conversation", "mission"],
  availability: codingSessionSurfaceFilesAvailability,
  Panel: CodingSessionSurfaceFilesPanel,
  // Whether the project has an agents repository, read once per view so the
  // launcher row can dim with its reason (§3) rather than open to nothing.
  readExtension: useCodingSessionFilesExtension,
};
