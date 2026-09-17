import type { previewCodingSessionSeatPack } from "./codingSessionActorSeatCustody";
import {
  type CodingSessionSeatPackSourceReader,
  resolveSeatPackSource,
} from "./codingSessionSeatedCreate";

/**
 * Spec § 4.10: whether a seat's worktree is cut without the project's team
 * definitions (`beekeeper/`).
 *
 * The default is to hide them — a seat that finds the other roles'
 * instructions while searching the repository confuses itself about its
 * own. The one exception is a role whose `team.yml` entry says
 * `workspace.roles_visible: true` (a role that authors roles), which the
 * host reports on the seat's pack preview as `rolesVisible`.
 *
 * Every failure keeps the default. A preview that refuses, throws, or comes
 * from a host too old to say `rolesVisible` hides the directory and names
 * why in `reason`, so the caller can disclose it; nothing here ever shows
 * the directory on a guess.
 */
export type SeatWorkspaceDecision = {
  hideRoles: boolean;
  /** Why the default held when it did not come from the manifest; null otherwise. */
  reason: string | null;
};

export type SeatWorkspaceDeps = {
  previewSeat?: typeof previewCodingSessionSeatPack;
  fetchPackSource?: CodingSessionSeatPackSourceReader;
};

export async function hideRolesForSeat(
  input: {
    agentPubkey: string;
    role: string | null;
    projectRef: string | null;
    requireProjectRef?: string | null;
    newSelection?: boolean;
  },
  deps: SeatWorkspaceDeps,
): Promise<SeatWorkspaceDecision> {
  if (!deps.previewSeat) {
    return {
      hideRoles: true,
      reason:
        "this caller cannot preview the seat's pack, so the directory is hidden",
    };
  }
  if (!input.role) {
    return {
      hideRoles: true,
      reason:
        "the seat has no role, so no manifest entry can make the directory visible",
    };
  }
  let packSource = null;
  try {
    packSource = await resolveSeatPackSource(
      input.projectRef,
      deps.fetchPackSource,
    );
  } catch (error) {
    return {
      hideRoles: true,
      reason: `the project's pack source could not be read: ${describe(error)}`,
    };
  }
  try {
    const preview = await deps.previewSeat({
      agentPubkey: input.agentPubkey,
      role: input.role,
      packSource,
      ...(input.requireProjectRef
        ? { requireProjectRef: input.requireProjectRef }
        : {}),
      ...(input.newSelection === true ? { newSelection: true } : {}),
    });
    if (!preview) {
      return {
        hideRoles: true,
        reason: "the host previewed no pack for this seat",
      };
    }
    if (preview.refusal) {
      return {
        hideRoles: true,
        reason: `the seat's pack would be refused: ${preview.refusal}`,
      };
    }
    if (preview.rolesVisible === true) {
      return { hideRoles: false, reason: null };
    }
    return { hideRoles: true, reason: null };
  } catch (error) {
    return {
      hideRoles: true,
      reason: `the seat's pack could not be previewed: ${describe(error)}`,
    };
  }
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
