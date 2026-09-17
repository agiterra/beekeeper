/**
 * Whether the viewer may write to a project's to-do lists — the client's
 * statement of the relay's `project_scoped_write_admitted` rule
 * (`crates/buzz-relay/src/handlers/ingest.rs`):
 *
 * - private project: the creator, a roster `owner` or a `collaborator`
 *   writes; a `viewer` reads only;
 * - public project: any community member writes;
 * - no coordinate (the local General placeholder): nothing can be written.
 *
 * Advisory, like every capability here: the relay re-decides, and a client
 * that got it wrong gets a refusal, not a privilege. What this buys is an
 * honest control — a viewer sees "read-only" rather than a checkbox that
 * fails.
 */
import type { ProjectCapabilities } from "@/features/projects-container/lib/projectPermissions";
import type {
  ProjectContainer,
  ProjectMember,
} from "@/features/projects-container/lib/projectContainerModel";
import { LOCAL_GENERAL_ID } from "@/features/projects-container/lib/projectContainerModel";

export type TodoWriteAccess =
  | { kind: "loading" }
  | { kind: "writable" }
  | { kind: "no-coordinate" }
  | { kind: "read-only"; reason: string };

export function todoWriteAccess(
  self: string | null,
  project: ProjectContainer | null,
  roster: readonly ProjectMember[],
  capabilities: Pick<ProjectCapabilities, "isOwner" | "isLoading">,
): TodoWriteAccess {
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
      reason:
        "You are a viewer of this project; only owners and collaborators can edit its lists.",
    };
  }
  return {
    kind: "read-only",
    reason: "You are not a member of this private project.",
  };
}
