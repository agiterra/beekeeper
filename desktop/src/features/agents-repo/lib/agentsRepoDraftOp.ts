/**
 * The kind 44250 agents-repository draft op — TypeScript twin of
 * `crates/buzz-core/src/agents_repo_draft.rs` and `docs/nips/NIP-AD.md`.
 *
 * A draft is one proposed change to one file of the project's agents
 * repository: the whole new text (`file.put`), an archive move
 * (`file.move`), a removal (`file.delete`), or the committer's record that
 * named drafts landed on `main` (`commit.record`). The content key set is
 * exact per op — absent is not null — and the `ad-*` tags repeat the
 * content's op, repository and paths so the relay can gate without parsing.
 *
 * No imports: this module is loaded by the conformance binder under plain
 * `node --test`. The kind integer is pinned against
 * `KIND_AGENTS_REPO_DRAFT_OP` in `agentsRepoDraftOp.test.mjs`.
 */

/** Kind 44250. Pinned against `@/shared/constants/kinds` in tests. */
export const AGENTS_REPO_DRAFT_OP_KIND = 44250;
export const AGENTS_REPO_DRAFT_SCHEMA = "buzz-agents-repo-draft/v1";
export const AGENTS_REPO_DRAFT_TAG_VERSION = "ad1-1";
/** The relay's advertised `max_content_len`. */
export const MAX_AGENTS_REPO_DRAFT_CONTENT_BYTES = 65_536;
/** The cap on a `file.put` text. */
export const MAX_AGENTS_REPO_DRAFT_TEXT_BYTES = 60_000;
export const MAX_AGENTS_REPO_DRAFT_MESSAGE_BYTES = 512;
export const MAX_COMMIT_RECORD_ENTRIES = 256;
/** The reserved directory; equal to `buzz_persona::team::ARCHIVE_DIR`. */
export const ARCHIVE_SEGMENT = "archive";
/** Root files a draft may put but never move or delete. */
export const ROOT_FILES = ["README.md", "team.yml", "actions.yml"] as const;
/**
 * The one tree in the layout with folders: a project's document artifacts.
 * `plans/`, `roles/` and `skills/` stay flat, because a plan's path is cited
 * by every adopted `planRef` and a role's stem is a `team.yml` key.
 */
export const DOCS_ROOT = "docs";
/** Components allowed under `docs/`, the last of them the file. */
export const MAX_DOCUMENT_COMPONENTS = 8;
/** A document's formats, lowercase so one path names one file. */
export const DOCUMENT_EXTENSIONS = [".md", ".html"] as const;
/** An image a document embeds, committed beside it. */
export const DOCUMENT_ASSET_EXTENSIONS = [
  ".png",
  ".jpg",
  ".jpeg",
  ".gif",
  ".webp",
  ".svg",
] as const;
/** The only dotfile the documents tree admits — how an empty folder exists. */
export const GITKEEP = ".gitkeep";

export type DraftOpKind =
  | "file.put"
  | "file.move"
  | "file.delete"
  | "asset.put"
  | "commit.record";

export type DraftBase = {
  /** Blob sha of the file on `main` the author started from; null = new. */
  base: string | null;
  /** Advisory: the `main` commit the author read. */
  baseCommit: string | null;
  /** The draft head the author edited from; null when there was none. */
  prev: string | null;
};

export type DraftOpContent =
  | ({ op: "file.put"; path: string; text: string } & DraftBase)
  | ({ op: "file.move"; path: string; to: string } & DraftBase)
  | ({ op: "file.delete"; path: string } & DraftBase)
  | ({
      op: "asset.put";
      path: string;
      /** The blob's id in the relay's media store. */
      sha256: string;
      /** The MIME the path's extension names. */
      mime: string;
      /** The blob's size in bytes, as the uploader observed it. */
      size: number;
    } & DraftBase)
  | { op: "commit.record"; commit: string; paths: string[]; drafts: string[] };

/** One op: the repository it belongs to, an optional reason, and the edit. */
export type DraftOp = {
  /** `30617:<hex>:<id>`, as the project's kind:30624 pins it. */
  repo: string;
  message: string | null;
  content: DraftOpContent;
};

const CONTENT_KEYS: Record<DraftOpKind, readonly string[]> = {
  "file.put": [
    "schema",
    "op",
    "path",
    "text",
    "base",
    "baseCommit",
    "prev",
    "message",
  ],
  "file.move": [
    "schema",
    "op",
    "path",
    "to",
    "base",
    "baseCommit",
    "prev",
    "message",
  ],
  "file.delete": [
    "schema",
    "op",
    "path",
    "base",
    "baseCommit",
    "prev",
    "message",
  ],
  "asset.put": [
    "schema",
    "op",
    "path",
    "sha256",
    "mime",
    "size",
    "base",
    "baseCommit",
    "prev",
    "message",
  ],
  "commit.record": ["schema", "op", "commit", "paths", "drafts", "message"],
};

export function isDraftOpKind(value: unknown): value is DraftOpKind {
  return (
    value === "file.put" ||
    value === "file.move" ||
    value === "file.delete" ||
    value === "asset.put" ||
    value === "commit.record"
  );
}

const LOWER_HEX = /^[0-9a-f]+$/;

export function isGitSha(value: unknown): value is string {
  return (
    typeof value === "string" && value.length === 40 && LOWER_HEX.test(value)
  );
}

export function isEventId(value: unknown): value is string {
  return (
    typeof value === "string" && value.length === 64 && LOWER_HEX.test(value)
  );
}

/** A 64-character lowercase hex media blob id. */
export function isBlobSha256(value: unknown): value is string {
  return isEventId(value);
}

function isSlug(value: string): boolean {
  return (
    value.length > 0 &&
    value.length <= 64 &&
    /^[a-z0-9-]+$/.test(value) &&
    value !== ARCHIVE_SEGMENT
  );
}

function isFileSegment(value: string): boolean {
  return (
    value.length > 0 &&
    value !== "." &&
    value !== ".." &&
    /^[A-Za-z0-9._-]+$/.test(value)
  );
}

export type DraftPathClass =
  | "root-file"
  | "role"
  | "archived-role"
  | "role-skill"
  | "shared-skill"
  | "plan"
  | "archived-plan"
  | "document"
  | "document-asset"
  | "document-folder";

/**
 * The MIME the relay's media store admits for each asset extension, which is
 * also what an `asset.put` must name. `.svg` is deliberately absent: the media
 * store refuses `image/svg+xml` as active web content, so an SVG in the tree
 * is committed with git rather than uploaded.
 */
export const DOCUMENT_ASSET_MIMES: readonly (readonly [string, string])[] = [
  [".png", "image/png"],
  [".jpg", "image/jpeg"],
  [".jpeg", "image/jpeg"],
  [".gif", "image/gif"],
  [".webp", "image/webp"],
];

/** A sanity bound on an `asset.put`'s `size`; the media store's cap is real. */
export const MAX_DOCUMENT_ASSET_BYTES = 100 * 1024 * 1024;

/**
 * The MIME an `asset.put` at `path` must name, or null when the path cannot
 * carry an uploaded asset at all. The extension decides, not the uploader.
 */
export function documentAssetMime(path: string): string | null {
  const classified = draftPathClass(path);
  if (!classified.ok || classified.class !== "document-asset") return null;
  for (const [extension, mime] of DOCUMENT_ASSET_MIMES) {
    if (path.endsWith(extension)) return mime;
  }
  return null;
}

/** Whether a class sits in the documents tree, where folders nest. */
export function isDocumentClass(value: DraftPathClass): boolean {
  return (
    value === "document" ||
    value === "document-asset" ||
    value === "document-folder"
  );
}

/**
 * A **plan's** filename stem: the name of a document, not a manifest key. It
 * carries uppercase, `_` and interior dots, because the documents that move
 * into an agents repository are called `CURRENT_STATE.md`, `SESSION_STATE.md`
 * and `README.md`. Requiring `isSlug` here was a carry-over from the role rule
 * and it cost something real — Beekeeper's own map and ledger moved in on
 * 2026-09-22 at paths the Files tab refuses to open.
 *
 * Still bounded, never `archive` in any case (that names the sibling
 * directory), and never leading with `.` or `-`. Pinned by
 * `conformance/agents-repo-draft-path/`.
 */
function isDocStem(value: string): boolean {
  return (
    value.length > 0 &&
    value.length <= 96 &&
    !value.startsWith(".") &&
    !value.startsWith("-") &&
    /^[A-Za-z0-9._-]+$/.test(value) &&
    value.toLowerCase() !== ARCHIVE_SEGMENT
  );
}

/** A role or skill file: named for its manifest key, so a slug. */
function isRoleFile(segment: string): boolean {
  return segment.endsWith(".md") && isSlug(segment.slice(0, -3));
}

/** A plan file: named for the document it holds. */
function isPlanFile(segment: string): boolean {
  return segment.endsWith(".md") && isDocStem(segment.slice(0, -3));
}

/**
 * A segment of the documents tree: a folder name, or a document's or asset's
 * stem. The same shape as `isDocStem` — these are document names too — except
 * that `archive` is an ordinary name here, because the documents tree has no
 * archive rule. A document is moved or deleted.
 */
function isDocSegment(value: string): boolean {
  return (
    value.length > 0 &&
    value.length <= 96 &&
    !value.startsWith(".") &&
    !value.startsWith("-") &&
    /^[A-Za-z0-9._-]+$/.test(value)
  );
}

/**
 * The stem of `file` once a known extension is taken off, or null when the
 * name ends in none of them. The *trailing* extension is the one read, so
 * `notes.md.txt` is not a document.
 */
function stemWithExtension(
  file: string,
  extensions: readonly string[],
): string | null {
  for (const extension of extensions) {
    if (file.endsWith(extension)) return file.slice(0, -extension.length);
  }
  return null;
}

/**
 * Classify the components under `docs/`. Folders nest, bounded by
 * `MAX_DOCUMENT_COMPONENTS`; every folder name is a document name; and the
 * file is a document, an asset, or the keep that holds an empty folder open.
 */
function classifyDocumentPath(
  path: string,
  components: readonly string[],
): { ok: true; class: DraftPathClass } | { ok: false; error: string } {
  if (components.length > MAX_DOCUMENT_COMPONENTS) {
    return {
      ok: false,
      error: `${path} is ${components.length} components under ${DOCS_ROOT}/; the cap is ${MAX_DOCUMENT_COMPONENTS}`,
    };
  }
  const file = components[components.length - 1] ?? "";
  const folders = components.slice(0, -1);
  const badFolder = folders.find((folder) => !isDocSegment(folder));
  if (badFolder !== undefined) {
    return {
      ok: false,
      error: `${path} has a folder ${badFolder} that is not a document name (at most 96 bytes of letters, digits, '.', '_' or '-', never leading with '.' or '-')`,
    };
  }
  if (file === GITKEEP) return { ok: true, class: "document-folder" };
  const documentStem = stemWithExtension(file, DOCUMENT_EXTENSIONS);
  if (documentStem !== null && isDocSegment(documentStem)) {
    return { ok: true, class: "document" };
  }
  const assetStem = stemWithExtension(file, DOCUMENT_ASSET_EXTENSIONS);
  if (assetStem !== null && isDocSegment(assetStem)) {
    return { ok: true, class: "document-asset" };
  }
  return {
    ok: false,
    error: `${path} is not a document (${DOCUMENT_EXTENSIONS.join(", ")}), an image (${DOCUMENT_ASSET_EXTENSIONS.join(", ")}) or ${GITKEEP}`,
  };
}

/** Classify a path against the agents repository layout, or say why not. */
export function draftPathClass(
  path: string,
): { ok: true; class: DraftPathClass } | { ok: false; error: string } {
  if (path.length === 0 || path.length > 512) {
    return { ok: false, error: "the path must be 1–512 bytes" };
  }
  if (path.startsWith("/") || path.endsWith("/")) {
    return {
      ok: false,
      error: `${path} must be relative with no trailing slash`,
    };
  }
  if ((ROOT_FILES as readonly string[]).includes(path)) {
    return { ok: true, class: "root-file" };
  }
  const segments = path.split("/");
  if (!segments.every(isFileSegment)) {
    return {
      ok: false,
      error: `${path} has an empty, dot or non-portable segment`,
    };
  }
  const [a, b, c, d, ...rest] = segments;
  if (
    segments.length === 3 &&
    a === "roles" &&
    b === ARCHIVE_SEGMENT &&
    c &&
    isRoleFile(c)
  ) {
    return { ok: true, class: "archived-role" };
  }
  if (
    segments.length === 3 &&
    a === "plans" &&
    b === ARCHIVE_SEGMENT &&
    c &&
    isPlanFile(c)
  ) {
    return { ok: true, class: "archived-plan" };
  }
  if (segments.length === 2 && a === "roles" && b && isRoleFile(b)) {
    return { ok: true, class: "role" };
  }
  if (segments.length === 2 && a === "plans" && b && isPlanFile(b)) {
    return { ok: true, class: "plan" };
  }
  if (
    segments.length >= 5 &&
    a === "roles" &&
    b &&
    isSlug(b) &&
    c === "skills" &&
    d &&
    isSlug(d) &&
    rest.length > 0
  ) {
    return { ok: true, class: "role-skill" };
  }
  if (segments.length >= 3 && a === "skills" && b && isSlug(b)) {
    return { ok: true, class: "shared-skill" };
  }
  if (segments.length >= 2 && a === DOCS_ROOT) {
    return classifyDocumentPath(path, segments.slice(1));
  }
  return {
    ok: false,
    error: `${path} is outside the agents repository layout (README.md, team.yml, actions.yml, roles/<role>.md, roles/archive/<role>.md, roles/<role>/skills/<skill>/…, skills/<skill>/…, plans/<plan>.md, plans/archive/<plan>.md, docs/<folder>/…/<document>.md|.html, docs/<folder>/…/<image>, docs/<folder>/…/.gitkeep)`,
  };
}

/** The only legal `to` of a `file.move`, or null. */
/**
 * Whether a `file.move` from `path` to `to` names a legal destination, and
 * why not when it does not.
 *
 * The rule differs by class, deliberately: a role or plan has exactly one
 * destination, its archive counterpart (a plan is never renamed — every
 * adopted `planRef` names it by path); a document, asset or folder keep may
 * move to any path of its own class, which is what rename and
 * move-between-folders are, and Markdown and HTML are one class so changing a
 * document's format is a move; a root file is put-only and a skill file has no
 * move at all.
 *
 * Renaming a folder is one move per file under it, issued by the caller. This
 * admits each one; it does not make a directory move atomic.
 * Pinned by `conformance/agents-repo-draft-path/` (`moves`).
 */
export function moveDestinationError(path: string, to: string): string | null {
  const from = draftPathClass(path);
  if (!from.ok) return from.error;
  if (isDocumentClass(from.class)) {
    if (to === path) return `${path} must move to a different path`;
    const target = draftPathClass(to);
    if (!target.ok) return target.error;
    if (target.class !== from.class) {
      return `${path} may only name another path of its own class, not ${to}`;
    }
    return null;
  }
  const counterpart = archiveCounterpart(path);
  if (counterpart === null) {
    return `${path} is not a role or plan that can move to or from archive/`;
  }
  if (counterpart !== to) {
    return `${path} may only go to ${counterpart}, not ${to}`;
  }
  return null;
}

export function archiveCounterpart(path: string): string | null {
  const classified = draftPathClass(path);
  if (!classified.ok) return null;
  const segments = path.split("/");
  switch (classified.class) {
    case "role":
    case "plan":
      return `${segments[0]}/${ARCHIVE_SEGMENT}/${segments[1]}`;
    case "archived-role":
    case "archived-plan":
      return `${segments[0]}/${segments[2]}`;
    default:
      return null;
  }
}

const encoder = new TextEncoder();

/** UTF-8 byte length. */
export function utf8Length(value: string): number {
  return encoder.encode(value).length;
}

/** Why a `file.put` text is refused, or null. */
export function draftTextError(text: string): string | null {
  const bytes = utf8Length(text);
  if (bytes > MAX_AGENTS_REPO_DRAFT_TEXT_BYTES) {
    return `The text is ${bytes.toLocaleString()} bytes; a draft carries at most ${MAX_AGENTS_REPO_DRAFT_TEXT_BYTES.toLocaleString()}. A file this size is edited with git, not drafted.`;
  }
  if (hasControlCharacters(text, true)) {
    return "The text must not contain control characters.";
  }
  return null;
}

/** C0 controls (and DEL), optionally letting newline, carriage return and tab through. */
function hasControlCharacters(
  value: string,
  allowWhitespace: boolean,
): boolean {
  for (let i = 0; i < value.length; i++) {
    const code = value.charCodeAt(i);
    if (code === 0x7f) return true;
    if (code >= 0x20) continue;
    if (allowWhitespace && (code === 0x0a || code === 0x0d || code === 0x09))
      continue;
    return true;
  }
  return false;
}

export function draftMessageError(message: string): string | null {
  if (utf8Length(message) > MAX_AGENTS_REPO_DRAFT_MESSAGE_BYTES) {
    return `A note is at most ${MAX_AGENTS_REPO_DRAFT_MESSAGE_BYTES} bytes.`;
  }
  if (hasControlCharacters(message, false)) {
    return "A note is one line.";
  }
  return null;
}

/** Every path an op names: the file (and a move's destination), or a record's paths. */
export function draftOpPaths(content: DraftOpContent): string[] {
  switch (content.op) {
    case "file.put":
    case "file.delete":
    case "asset.put":
      return [content.path];
    case "file.move":
      return [content.path, content.to];
    case "commit.record":
      return [...content.paths];
  }
}

/** Canonical content JSON: the exact key set, nullables as null. */
export function encodeDraftOpContent(op: DraftOp): string {
  const c = op.content;
  const object: Record<string, unknown> = {
    schema: AGENTS_REPO_DRAFT_SCHEMA,
    op: c.op,
  };
  switch (c.op) {
    case "file.put":
      object.path = c.path;
      object.text = c.text;
      object.base = c.base;
      object.baseCommit = c.baseCommit;
      object.prev = c.prev;
      break;
    case "file.move":
      object.path = c.path;
      object.to = c.to;
      object.base = c.base;
      object.baseCommit = c.baseCommit;
      object.prev = c.prev;
      break;
    case "file.delete":
      object.path = c.path;
      object.base = c.base;
      object.baseCommit = c.baseCommit;
      object.prev = c.prev;
      break;
    case "asset.put":
      object.path = c.path;
      object.sha256 = c.sha256;
      object.mime = c.mime;
      object.size = c.size;
      object.base = c.base;
      object.baseCommit = c.baseCommit;
      object.prev = c.prev;
      break;
    case "commit.record":
      object.commit = c.commit;
      object.paths = c.paths;
      object.drafts = c.drafts;
      break;
  }
  object.message = op.message;
  return JSON.stringify(object);
}

/** The tags: `a`, `ad-v`, `ad-op`, `ad-repo`, then one `ad-path` per path. */
export function draftOpTags(coordinate: string, op: DraftOp): string[][] {
  return [
    ["a", coordinate],
    ["ad-v", AGENTS_REPO_DRAFT_TAG_VERSION],
    ["ad-op", op.content.op],
    ["ad-repo", op.repo],
    ...draftOpPaths(op.content).map((path) => ["ad-path", path]),
  ];
}

function nullableString(
  object: Record<string, unknown>,
  key: string,
): { ok: true; value: string | null } | { ok: false } {
  const value = object[key];
  if (value === null) return { ok: true, value: null };
  if (typeof value === "string") return { ok: true, value };
  return { ok: false };
}

function decodeBase(object: Record<string, unknown>): DraftBase | null {
  const base = nullableString(object, "base");
  const baseCommit = nullableString(object, "baseCommit");
  const prev = nullableString(object, "prev");
  if (!base.ok || !baseCommit.ok || !prev.ok) return null;
  if (base.value !== null && !isGitSha(base.value)) return null;
  if (baseCommit.value !== null && !isGitSha(baseCommit.value)) return null;
  if (prev.value !== null && !isEventId(prev.value)) return null;
  return { base: base.value, baseCommit: baseCommit.value, prev: prev.value };
}

function decodeStringList(
  value: unknown,
  check: (item: string) => boolean,
): string[] | null {
  if (!Array.isArray(value)) return null;
  if (value.length === 0 || value.length > MAX_COMMIT_RECORD_ENTRIES)
    return null;
  const out: string[] = [];
  for (const item of value) {
    if (typeof item !== "string" || !check(item) || out.includes(item))
      return null;
    out.push(item);
  }
  return out;
}

/**
 * Decode content JSON for the repository `repo`. Returns null for anything
 * the Rust validator refuses; the fold counts those as `ignored`.
 */
export function decodeDraftOp(content: string, repo: string): DraftOp | null {
  if (utf8Length(content) > MAX_AGENTS_REPO_DRAFT_CONTENT_BYTES) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return null;
  }
  const object = parsed as Record<string, unknown>;
  if (object.schema !== AGENTS_REPO_DRAFT_SCHEMA) return null;
  const kind = object.op;
  if (!isDraftOpKind(kind)) return null;
  const expected = CONTENT_KEYS[kind];
  const keys = Object.keys(object);
  if (keys.some((key) => !expected.includes(key))) return null;
  if (expected.some((key) => !(key in object))) return null;
  const message = nullableString(object, "message");
  if (!message.ok) return null;
  if (message.value !== null && draftMessageError(message.value) !== null) {
    return null;
  }
  switch (kind) {
    case "file.put": {
      const path = object.path;
      const text = object.text;
      if (typeof path !== "string" || !draftPathClass(path).ok) return null;
      if (typeof text !== "string" || draftTextError(text) !== null)
        return null;
      const base = decodeBase(object);
      if (!base) return null;
      return {
        repo,
        message: message.value,
        content: { op: kind, path, text, ...base },
      };
    }
    case "file.move": {
      const path = object.path;
      const to = object.to;
      if (typeof path !== "string" || typeof to !== "string") return null;
      if (moveDestinationError(path, to) !== null) return null;
      const base = decodeBase(object);
      if (!base) return null;
      return {
        repo,
        message: message.value,
        content: { op: kind, path, to, ...base },
      };
    }
    case "file.delete": {
      const path = object.path;
      if (typeof path !== "string") return null;
      const classified = draftPathClass(path);
      if (!classified.ok || classified.class === "root-file") return null;
      const base = decodeBase(object);
      if (!base) return null;
      return {
        repo,
        message: message.value,
        content: { op: kind, path, ...base },
      };
    }
    case "asset.put": {
      const path = object.path;
      if (typeof path !== "string") return null;
      const mime = documentAssetMime(path);
      if (mime === null || object.mime !== mime) return null;
      const sha256 = object.sha256;
      if (typeof sha256 !== "string" || !isBlobSha256(sha256)) return null;
      const size = object.size;
      if (
        typeof size !== "number" ||
        !Number.isInteger(size) ||
        size <= 0 ||
        size > MAX_DOCUMENT_ASSET_BYTES
      ) {
        return null;
      }
      const base = decodeBase(object);
      if (!base) return null;
      return {
        repo,
        message: message.value,
        content: { op: kind, path, sha256, mime, size, ...base },
      };
    }
    case "commit.record": {
      const commit = object.commit;
      if (!isGitSha(commit)) return null;
      const paths = decodeStringList(object.paths, (p) => draftPathClass(p).ok);
      const drafts = decodeStringList(object.drafts, isEventId);
      if (!paths || !drafts) return null;
      return {
        repo,
        message: message.value,
        content: { op: kind, commit, paths, drafts },
      };
    }
  }
}
