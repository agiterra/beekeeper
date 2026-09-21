/**
 * The agents-repository draft fold — TypeScript twin of
 * `crates/buzz-core/src/agents_repo_draft_fold.rs`, bound to
 * `conformance/agents-repo-draft-fold/` (CONTRACT.md states the rules; the
 * vectors pin them byte for byte, key order included).
 *
 * Pure and total: any bag of events in, one digest out, the same digest
 * from every client. No imports outside this directory.
 */
import {
  AGENTS_REPO_DRAFT_OP_KIND,
  type DraftOp,
  decodeDraftOp,
  draftOpPaths,
} from "./agentsRepoDraftOp.ts";

export const AGENTS_REPO_DRAFT_DIGEST_SCHEMA =
  "buzz-agents-repo-draft-digest/v1";

/** The subset of a relay event the fold reads. */
export type DraftFoldEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
};

/** One file op as the digest reports it, head or superseded. */
export type DraftRow = {
  id: string;
  author: string;
  createdAt: number;
  op: "file.put" | "file.move" | "file.delete";
  path: string;
  to: string | null;
  text: string | null;
  base: string | null;
  baseCommit: string | null;
  prev: string | null;
  message: string | null;
};

export type DraftPath = {
  path: string;
  head: DraftRow;
  /** Every other open op naming the path, oldest first. */
  superseded: DraftRow[];
  /** The head did not build on the newest superseded op. */
  diverged: boolean;
  updatedAt: number;
};

export type CommitRecordRow = {
  id: string;
  commit: string;
  by: string;
  createdAt: number;
  paths: string[];
  drafts: string[];
  message: string | null;
};

export type AgentsRepoDraftDigest = {
  schema: typeof AGENTS_REPO_DRAFT_DIGEST_SCHEMA;
  project: string;
  repo: string;
  ignored: number;
  otherRepo: number;
  paths: DraftPath[];
  commits: CommitRecordRow[];
};

type OpKey = { createdAt: number; id: string };

function keyLess(a: OpKey, b: OpKey): boolean {
  return (
    a.createdAt < b.createdAt || (a.createdAt === b.createdAt && a.id < b.id)
  );
}

function bytewiseLess(a: string, b: string): boolean {
  const ea = new TextEncoder().encode(a);
  const eb = new TextEncoder().encode(b);
  const n = Math.min(ea.length, eb.length);
  for (let i = 0; i < n; i++) {
    const x = ea[i] ?? 0;
    const y = eb[i] ?? 0;
    if (x !== y) return x < y;
  }
  return ea.length < eb.length;
}

/** `30621:<hex>:<dtag>` with the hex lowercased; null when not that shape. */
function normalizeProjectCoordinate(value: string): string | null {
  const first = value.indexOf(":");
  const second = value.indexOf(":", first + 1);
  if (first < 0 || second < 0) return null;
  const kind = value.slice(0, first);
  const hex = value.slice(first + 1, second);
  const dtag = value.slice(second + 1);
  if (kind !== "30621" || hex.length !== 64 || !/^[0-9a-fA-F]+$/.test(hex)) {
    return null;
  }
  if (dtag.length === 0) return null;
  return `30621:${hex.toLowerCase()}:${dtag}`;
}

/** `30617:<lowercase hex>:<id>` exactly; null otherwise. */
function canonicalRepositoryCoordinate(value: string): string | null {
  const first = value.indexOf(":");
  const second = value.indexOf(":", first + 1);
  if (first < 0 || second < 0) return null;
  const kind = value.slice(0, first);
  const hex = value.slice(first + 1, second);
  const id = value.slice(second + 1);
  if (kind !== "30617" || hex.length !== 64 || !/^[0-9a-fA-F]+$/.test(hex)) {
    return null;
  }
  if (id.length === 0) return null;
  const canonical = `30617:${hex.toLowerCase()}:${id}`;
  return canonical === value ? canonical : null;
}

function singleTag(event: DraftFoldEvent, key: string): string | null {
  const values = event.tags.filter((t) => t[0] === key).map((t) => t[1]);
  if (values.length !== 1) return null;
  return values[0] ?? null;
}

type Decoded = { key: OpKey; pubkey: string; op: DraftOp };

type Decode =
  | { kind: "ignored" }
  | { kind: "other-repo" }
  | { kind: "op"; op: DraftOp };

function decode(project: string, repo: string, event: DraftFoldEvent): Decode {
  if (event.kind !== AGENTS_REPO_DRAFT_OP_KIND) return { kind: "ignored" };
  const coordinate = singleTag(event, "a");
  if (
    coordinate === null ||
    normalizeProjectCoordinate(coordinate) !== project
  ) {
    return { kind: "ignored" };
  }
  const tagRepo = singleTag(event, "ad-repo");
  if (tagRepo === null) return { kind: "ignored" };
  const canonical = canonicalRepositoryCoordinate(tagRepo);
  if (canonical === null) return { kind: "ignored" };
  const op = decodeDraftOp(event.content, canonical);
  if (op === null) return { kind: "ignored" };
  if (canonical !== repo) return { kind: "other-repo" };
  return { kind: "op", op };
}

function row(d: Decoded): DraftRow | null {
  const c = d.op.content;
  if (c.op === "commit.record") return null;
  return {
    id: d.key.id,
    author: d.pubkey,
    createdAt: d.key.createdAt,
    op: c.op,
    path: c.path,
    to: c.op === "file.move" ? c.to : null,
    text: c.op === "file.put" ? c.text : null,
    base: c.base,
    baseCommit: c.baseCommit,
    prev: c.prev,
    message: d.op.message,
  };
}

/** Fold `events` for `project` and its agents repository `repo`. */
export function foldAgentsRepoDrafts(
  project: string,
  repo: string,
  events: readonly DraftFoldEvent[],
): AgentsRepoDraftDigest {
  let ignored = 0;
  let otherRepo = 0;
  const seen = new Set<string>();
  const ops: Decoded[] = [];
  for (const event of events) {
    if (seen.has(event.id)) continue;
    seen.add(event.id);
    const decoded = decode(project, repo, event);
    if (decoded.kind === "op") {
      ops.push({
        key: { createdAt: event.created_at, id: event.id },
        pubkey: event.pubkey,
        op: decoded.op,
      });
    } else if (decoded.kind === "other-repo") otherRepo++;
    else ignored++;
  }
  ops.sort((a, b) =>
    keyLess(a.key, b.key) ? -1 : keyLess(b.key, a.key) ? 1 : 0,
  );

  const closed = new Set<string>();
  const commits: CommitRecordRow[] = [];
  for (const d of ops) {
    const c = d.op.content;
    if (c.op !== "commit.record") continue;
    for (const id of c.drafts) closed.add(id);
    commits.push({
      id: d.key.id,
      commit: c.commit,
      by: d.pubkey,
      createdAt: d.key.createdAt,
      paths: [...c.paths],
      drafts: [...c.drafts],
      message: d.op.message,
    });
  }
  commits.reverse();

  const byPath = new Map<string, DraftRow[]>();
  for (const d of ops) {
    if (closed.has(d.key.id)) continue;
    const r = row(d);
    if (r === null) continue;
    for (const path of draftOpPaths(d.op.content)) {
      const list = byPath.get(path) ?? [];
      list.push({ ...r });
      byPath.set(path, list);
    }
  }
  const paths: DraftPath[] = [];
  for (const [path, open] of byPath) {
    const head = open.pop();
    if (!head) continue;
    const last = open.length > 0 ? open[open.length - 1] : undefined;
    const diverged = last !== undefined && last.id !== head.prev;
    paths.push({
      path,
      head,
      superseded: open,
      diverged,
      updatedAt: head.createdAt,
    });
  }
  paths.sort((a, b) =>
    bytewiseLess(a.path, b.path) ? -1 : bytewiseLess(b.path, a.path) ? 1 : 0,
  );

  return {
    schema: AGENTS_REPO_DRAFT_DIGEST_SCHEMA,
    project,
    repo,
    ignored,
    otherRepo,
    paths,
    commits,
  };
}
