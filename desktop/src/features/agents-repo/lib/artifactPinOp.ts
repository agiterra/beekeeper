/**
 * NIP-AR kind:44251 — the wire twin of
 * `crates/beekeeper-core/src/project_artifact_pin.rs`.
 *
 * Which documents, plans and folders of a project's agents repository show in
 * every member's sidebar, and in what order. A pin is shared: every member
 * sees it. Hiding the pinned rows is a per-device viewing preference that
 * never reaches the wire.
 *
 * Import constraint (load-bearing): this module and `artifactPinFold.ts` are
 * loaded by the conformance binder under plain `node --test` with no loader,
 * so every import is a **relative path with an explicit extension** — never
 * the `@/` alias, never an npm package — and the syntax stays erasable.
 */
import {
  draftPathClass,
  DOCS_ROOT,
  MAX_DOCUMENT_COMPONENTS,
} from "./agentsRepoDraftOp.ts";
import { rankError } from "../../project-todos/lib/fractionalRank.ts";

export const PROJECT_ARTIFACT_PIN_KIND = 44251;
export const PROJECT_ARTIFACT_PIN_SCHEMA = "buzz-project-artifact-pin/v1";
export const PROJECT_ARTIFACT_PIN_TAG_VERSION = "ar1-1";
export const MAX_PROJECT_ARTIFACT_PIN_CONTENT_BYTES = 2 * 1024;

export type PinOpKind = "pin.set" | "pin.rank";

/** What a pin points at. */
export type PinTargetKind = "file" | "folder";

export type PinOpContent =
  | {
      op: "pin.set";
      target: string;
      targetKind: PinTargetKind;
      pinned: boolean;
      rank: string;
    }
  | { op: "pin.rank"; target: string; rank: string };

/** One op: the repository it is about, and what it sets. */
export type PinOp = {
  /** `30617:<hex>:<id>`, as the project's kind:30624 pins it. */
  repo: string;
  content: PinOpContent;
};

const CONTENT_KEYS: Record<PinOpKind, readonly string[]> = {
  "pin.set": ["schema", "op", "target", "targetKind", "pinned", "rank"],
  "pin.rank": ["schema", "op", "target", "rank"],
};

export function isPinOpKind(value: unknown): value is PinOpKind {
  return value === "pin.set" || value === "pin.rank";
}

function isPinTargetKind(value: unknown): value is PinTargetKind {
  return value === "file" || value === "folder";
}

/**
 * Why a folder prefix is refused, or null.
 *
 * A folder is `docs/<segment>/…` with one to `MAX_DOCUMENT_COMPONENTS - 1`
 * segments, each a document name. It is deliberately not a path the grammar
 * admits: git has no directory object, so a folder is only the prefix its
 * files share. One fewer segment than a path, so a file under the deepest
 * pinnable folder still fits.
 */
export function pinFolderError(target: string): string | null {
  if (target.length === 0 || target.length > 512) {
    return "a pinned folder is 1–512 bytes";
  }
  if (target.startsWith("/") || target.endsWith("/")) {
    return `${target} must be relative with no trailing slash`;
  }
  const segments = target.split("/");
  const root = segments[0];
  const folders = segments.slice(1);
  if (root !== DOCS_ROOT) {
    return `${target} is outside the documents tree, the only part of the layout with folders`;
  }
  if (folders.length === 0) {
    return `${target} is the documents tree itself, which is not pinnable`;
  }
  if (folders.length >= MAX_DOCUMENT_COMPONENTS) {
    return `${target} is ${folders.length} segments under ${DOCS_ROOT}/; the deepest pinnable folder is ${MAX_DOCUMENT_COMPONENTS - 1} so a file under it still fits`;
  }
  // One rule for both: a folder name is a document name, so a path built
  // under a pinnable folder is a path the grammar admits.
  for (const segment of folders) {
    if (!draftPathClass(`${DOCS_ROOT}/${segment}/x.md`).ok) {
      return `${target} has a segment ${segment} that is not a document name`;
    }
  }
  return null;
}

/**
 * Why `target` is refused for `kind`, or null.
 *
 * A file target is any path the layout admits except a folder keep: the keep
 * is how an empty directory exists in git, and pinning it instead of the
 * folder it holds open would put a row called `.gitkeep` in the sidebar.
 */
export function pinTargetError(
  target: string,
  kind: PinTargetKind,
): string | null {
  if (kind === "folder") return pinFolderError(target);
  const classified = draftPathClass(target);
  if (!classified.ok) return classified.error;
  if (classified.class === "document-folder") {
    return `${target} is a folder's keep; pin the folder it holds open, not the file`;
  }
  return null;
}

/** Whether `target` is a legal target of either kind. */
export function isPinTarget(target: string): boolean {
  return (
    pinTargetError(target, "file") === null ||
    pinTargetError(target, "folder") === null
  );
}

/** Canonical content JSON: the exact key set, in canonical order. */
export function encodePinOpContent(op: PinOp): string {
  const c = op.content;
  const object: Record<string, unknown> = {
    schema: PROJECT_ARTIFACT_PIN_SCHEMA,
    op: c.op,
    target: c.target,
  };
  if (c.op === "pin.set") {
    object.targetKind = c.targetKind;
    object.pinned = c.pinned;
  }
  object.rank = c.rank;
  return JSON.stringify(object);
}

/**
 * The tags this op carries, in canonical order: `a`, `ar-v`, `ar-op`,
 * `ar-repo`, `ar-target`.
 */
export function pinOpTags(coordinate: string, op: PinOp): string[][] {
  return [
    ["a", coordinate],
    ["ar-v", PROJECT_ARTIFACT_PIN_TAG_VERSION],
    ["ar-op", op.content.op],
    ["ar-repo", op.repo],
    ["ar-target", op.content.target],
  ];
}

/** Decode and validate content JSON, or null when it fails any rule. */
export function decodePinOp(content: string, repo: string): PinOp | null {
  if (content.length > MAX_PROJECT_ARTIFACT_PIN_CONTENT_BYTES) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return null;
  }
  const object = parsed as Record<string, unknown>;
  if (object.schema !== PROJECT_ARTIFACT_PIN_SCHEMA) return null;
  const kind = object.op;
  if (!isPinOpKind(kind)) return null;
  const expected = CONTENT_KEYS[kind];
  const keys = Object.keys(object);
  if (keys.some((key) => !expected.includes(key))) return null;
  if (expected.some((key) => !(key in object))) return null;
  const target = object.target;
  if (typeof target !== "string") return null;
  const rank = object.rank;
  if (typeof rank !== "string" || rankError(rank) !== null) return null;
  if (kind === "pin.set") {
    const targetKind = object.targetKind;
    if (!isPinTargetKind(targetKind)) return null;
    if (pinTargetError(target, targetKind) !== null) return null;
    const pinned = object.pinned;
    if (typeof pinned !== "boolean") return null;
    return { repo, content: { op: kind, target, targetKind, pinned, rank } };
  }
  // A `pin.rank` does not repeat what the target is, so it cannot be checked
  // against a kind. Either shape is legal here and the fold keeps it only when
  // a `pin.set` established the target.
  if (!isPinTarget(target)) return null;
  return { repo, content: { op: kind, target, rank } };
}
