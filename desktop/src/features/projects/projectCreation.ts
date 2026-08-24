import {
  KIND_PROJECT_ANNOUNCEMENT,
  KIND_REPO_ANNOUNCEMENT,
} from "@/shared/constants/kinds";
import { isValidProjectChannelId } from "./projectModels";

export type ProjectEventTemplate = {
  kind: number;
  content: string;
  tags: string[][];
};

export type InitialProjectEventTemplates = {
  dtag: string;
  project: ProjectEventTemplate;
  repository: ProjectEventTemplate;
  repositoryAddress: string;
};

export function isUnsupportedProjectKindError(error: unknown): boolean {
  return (
    error instanceof Error &&
    /(?:unknown|unsupported) event kind/i.test(error.message)
  );
}

export function projectDtagFromName(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export function buildInitialProjectEventTemplates({
  accessChannelId,
  cloneUrl,
  description,
  name,
  ownerPubkey,
  projectRef,
  webUrl,
}: {
  /**
   * Optional legacy `buzz-channel` binding, emitted on both the repository
   * and the project head when supplied. Access normally comes from the
   * project roster via the repository's `project` back-reference, so the
   * create/import dialogs no longer collect it.
   */
  accessChannelId?: string;
  cloneUrl?: string;
  description?: string;
  name: string;
  ownerPubkey: string;
  /** Project container coordinate (`30621:<owner>:<slug>`) this repo is
   * created inside — becomes the announcement's `project` tag. */
  projectRef?: string;
  webUrl?: string;
}): InitialProjectEventTemplates {
  const normalizedName = name.trim();
  if (!normalizedName) {
    throw new Error("Project name is required.");
  }
  if (new TextEncoder().encode(normalizedName).byteLength > 256) {
    throw new Error("Project name must not exceed 256 bytes.");
  }
  const dtag = projectDtagFromName(normalizedName);
  if (!dtag) {
    throw new Error("Project name must include letters or numbers.");
  }
  const normalizedOwner = ownerPubkey.trim().toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(normalizedOwner)) {
    throw new Error("Project owner public key is invalid.");
  }

  const normalizedDescription = description?.trim() ?? "";
  if (new TextEncoder().encode(normalizedDescription).byteLength > 2_048) {
    throw new Error("Project description must not exceed 2,048 bytes.");
  }
  const repositoryTags: string[][] = [
    ["d", dtag],
    ["name", normalizedName],
  ];
  const projectTags: string[][] = [
    ["d", dtag],
    ["name", normalizedName],
  ];
  const normalizedAccessChannelId = accessChannelId?.trim();
  if (normalizedAccessChannelId) {
    // Shape-validated only when supplied: a malformed value would produce a
    // `Broken` binding, which the relay fails closed on for everyone —
    // strictly worse than the no-binding case it would have replaced.
    if (!isValidProjectChannelId(normalizedAccessChannelId)) {
      throw new Error("Repository access channel is invalid.");
    }
    repositoryTags.push(["buzz-channel", normalizedAccessChannelId]);
    projectTags.push(["buzz-channel", normalizedAccessChannelId]);
  }
  if (normalizedDescription) {
    repositoryTags.push(["description", normalizedDescription]);
    projectTags.push(["description", normalizedDescription]);
  }
  const normalizedCloneUrl = cloneUrl?.trim();
  if (normalizedCloneUrl) {
    repositoryTags.push(["clone", normalizedCloneUrl]);
  }
  const normalizedWebUrl = webUrl?.trim();
  if (normalizedWebUrl) {
    repositoryTags.push(["web", normalizedWebUrl]);
  }
  const normalizedProjectRef = projectRef?.trim();
  if (normalizedProjectRef) {
    repositoryTags.push(["project", normalizedProjectRef]);
  }

  const repositoryAddress = `${KIND_REPO_ANNOUNCEMENT}:${normalizedOwner}:${dtag}`;
  projectTags.push(["a", repositoryAddress]);

  return {
    dtag,
    project: {
      kind: KIND_PROJECT_ANNOUNCEMENT,
      content: "",
      tags: projectTags,
    },
    repository: {
      kind: KIND_REPO_ANNOUNCEMENT,
      content: normalizedDescription,
      tags: repositoryTags,
    },
    repositoryAddress,
  };
}
