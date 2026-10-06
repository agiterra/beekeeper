/**
 * Whether the viewer may draft, and whether they may commit — the
 * client's statement of two relay rules, both advisory:
 *
 * - drafting is the project-scoped write rule (`admit_project_scoped_write`):
 *   a private project's creator, owner or collaborator; any member of a
 *   public one; a viewer reads only;
 * - committing is the git push gate (`crates/beekeeper-relay/src/api/git/policy.rs`):
 *   a roster owner or collaborator may push `main`; a viewer may not; a
 *   `buzz-protect` rule on the repository may still refuse.
 *
 * The relay re-decides both; what this buys is an honest control — a viewer
 * sees "read-only" rather than a button that fails.
 */
import type { ProjectCapabilities } from "@/features/projects-container/lib/projectPermissions";
import type {
  ProjectContainer,
  ProjectMember,
} from "@/features/projects-container/lib/projectContainerModel";
import { LOCAL_GENERAL_ID } from "@/features/projects-container/lib/projectContainerModel";

export type AgentsRepoAccess =
  | { kind: "loading" }
  | { kind: "writable" }
  | { kind: "no-coordinate" }
  | { kind: "read-only"; reason: string };

function access(
  self: string | null,
  project: ProjectContainer | null,
  roster: readonly ProjectMember[],
  capabilities: Pick<ProjectCapabilities, "isOwner" | "isLoading">,
  what: string,
): AgentsRepoAccess {
  if (
    !project ||
    project.id === LOCAL_GENERAL_ID ||
    project.owner.length === 0
  ) {
    return { kind: "no-coordinate" };
  }
  if (capabilities.isLoading || self === null) return { kind: "loading" };
  if (project.visibility === "public") return { kind: "writable" };
  if (capabilities.isOwner) return { kind: "writable" };
  const me = self.toLowerCase();
  const role = roster.find(
    (member) => member.pubkey.toLowerCase() === me,
  )?.role;
  if (role === "collaborator") return { kind: "writable" };
  if (role === "viewer") {
    return {
      kind: "read-only",
      reason: `You are a viewer of this project; only owners and collaborators can ${what}.`,
    };
  }
  return {
    kind: "read-only",
    reason: "You are not a member of this private project.",
  };
}

/** May the viewer publish drafts? */
export function agentsRepoDraftAccess(
  self: string | null,
  project: ProjectContainer | null,
  roster: readonly ProjectMember[],
  capabilities: Pick<ProjectCapabilities, "isOwner" | "isLoading">,
): AgentsRepoAccess {
  return access(self, project, roster, capabilities, "draft changes");
}

/** May the viewer commit drafts to `main`? The relay's push gate decides. */
export function agentsRepoCommitAccess(
  self: string | null,
  project: ProjectContainer | null,
  roster: readonly ProjectMember[],
  capabilities: Pick<ProjectCapabilities, "isOwner" | "isLoading">,
): AgentsRepoAccess {
  return access(
    self,
    project,
    roster,
    capabilities,
    "commit to the agents repository",
  );
}
