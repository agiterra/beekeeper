/**
 * Wire types of the Files tab's host commands
 * (`desktop/src-tauri/src/managed_agents/agents_repo_read.rs`,
 * `agents_repo_commit.rs`). Field names are law: the Rust structs serialize
 * camelCase and the renderer prints them, composing no happier sentence.
 */

export type AgentsRepoEntryKind =
  | "plan"
  | "document"
  | "document-asset"
  | "document-folder"
  | "role"
  | "skill"
  | "manifest"
  | "readme"
  | "archived-role"
  | "archived-plan"
  | "gitkeep"
  | "other";

export type AgentsRepoEntry = {
  path: string;
  blob: string;
  size: number;
  kind: AgentsRepoEntryKind;
};

export type AgentsRepoListing = {
  repo: string;
  branch: string;
  commit: string;
  syncedAt: string | null;
  entries: AgentsRepoEntry[];
};

export type AgentsRepoFileState =
  | "on-main"
  | "not-on-main"
  | "not-text"
  | "too-large";

export type AgentsRepoFile = {
  path: string;
  text: string | null;
  state: AgentsRepoFileState;
  blob: string | null;
  commit: string;
  size: number | null;
  syncedAt: string | null;
};

export type AgentsRepoDraftChange = {
  id: string;
  author: string;
  op: "file.put" | "file.move" | "file.delete" | "asset.put";
  path: string;
  to: string | null;
  text: string | null;
  /**
   * An `asset.put`'s media blob id. The host fetches those bytes and
   * sha-verifies them before the commit; `mime` and `size` ride along for
   * symmetry with the fold's row and the host ignores them.
   */
  sha256: string | null;
  mime: string | null;
  size: number | null;
  base: string | null;
  message: string | null;
};

export type AgentsRepoAuthor = { pubkey: string; name: string | null };

export type AgentsRepoCommitRequest = {
  projectRef: string;
  expectedTip: string | null;
  message: string;
  drafts: AgentsRepoDraftChange[];
  authors: AgentsRepoAuthor[];
};

export type AgentsRepoCommitRefusalCode =
  | "main-moved"
  | "no-main"
  | "stale-base"
  | "invalid-tree"
  | "lease-rejected"
  | "push-refused";

export type AgentsRepoCommitRefusal = {
  path: string | null;
  code: AgentsRepoCommitRefusalCode | string;
  message: string;
};

export type AgentsRepoCommittedPath = { path: string; status: string };

export type AgentsRepoCommitResult = {
  pushed: "yes" | "no" | "unknown";
  tipBefore: string | null;
  commit: string | null;
  tree: string | null;
  paths: AgentsRepoCommittedPath[];
  actions: string | null;
  refusals: AgentsRepoCommitRefusal[];
  unknownReason: string | null;
  committerName: string;
  committerEmail: string;
  draftIds: string[];
};

/** `buzz-core`'s `beekeeper-plan/v1` reader's answer; all null when it reads. */
export type PlanSourceCheck = {
  code: string | null;
  path: string | null;
  message: string | null;
};
