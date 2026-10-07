/**
 * Which inline code and link hrefs in an agent's answer are shaped like a
 * file path (SV-32).
 *
 * A port of T3 Code's rule (`packages/client-runtime/src/markdownLinks.ts`,
 * `inlineCodeFilePathCandidate`, at t3code 517188b3): a candidate has a
 * separator plus an extension or a `:line` suffix, does not start with a
 * hostname, and does not end in a version (`glm-5.3`, `python/3.12`). Fenced
 * code and prose without backticks are never candidates, as in T3.
 *
 * One deliberate difference: **only relative paths are candidates.** An
 * absolute host path is elided before signing (`coding_session_context.rs`),
 * so what reaches a transcript as `/…` is either a marker or a path that was
 * never this session's; vault-recovered absolute paths are slice S3. `~/`,
 * drive letters and UNC paths are therefore not candidates either, and the
 * host refuses them again on its side.
 *
 * Pure: no React, no IPC. Nothing here resolves a path.
 */

const RELATIVE_PATH_PREFIX_PATTERN = /^\.{1,2}\//;
const POSITION_SUFFIX_PATTERN = /:\d+(?::\d+)?$/;
const POSITION_SUFFIX_CAPTURE_PATTERN = /:(\d+)(?::(\d+))?$/;
const POSITION_HASH_PATTERN = /^#L(\d+)(?:C(\d+))?$/i;
const INLINE_CODE_DISQUALIFIER_PATTERN = /[\s`]/;
const PATH_SEPARATOR_PATTERN = /\//;
const FILE_EXTENSION_PATTERN = /\.[A-Za-z0-9_-]+$/;
// A final dot between digits marks a version or model id, not an extension.
// `ls.1` and `libfoo.so.1` stay files.
const VERSION_SUFFIX_PATTERN = /\d\.\d[^.]*$/;
const NUMERIC_DOTTED_PATTERN = /^\d+(?:\.\d+)+$/;
const EXTERNAL_SCHEME_PATTERN = /^[A-Za-z][A-Za-z0-9+.-]*:/;
const ABSOLUTE_PATTERN = /^(?:\/|~|[A-Za-z]:[\\/]|\\\\)/;

const SINGLE_LABEL_HOSTNAMES = new Set(["localhost"]);
const GENERIC_HOSTNAME_TLDS = new Set([
  "com",
  "net",
  "org",
  "io",
  "dev",
  "app",
  "ai",
  "co",
  "edu",
  "gov",
  "mil",
  "info",
  "biz",
  "xyz",
  "me",
  "tv",
  "cc",
  "gg",
  "chat",
  "cloud",
  "site",
  "online",
  "tech",
  "store",
  "link",
]);
// Country codes also name file extensions; a `:line` suffix makes `.pl` and
// `.pt` files likelier than hostnames.
const COUNTRY_HOSTNAME_TLDS = new Set(
  "uk de fr nl se no fi dk pl ch at be es it pt eu us ca au nz jp kr cn br ru mx ie cz tr sg hk".split(
    " ",
  ),
);

function looksLikeHostname(segment: string, hasPosition: boolean): boolean {
  if (segment.startsWith(".")) return false;
  const lowered = segment.toLowerCase();
  if (SINGLE_LABEL_HOSTNAMES.has(lowered)) return true;
  if (NUMERIC_DOTTED_PATTERN.test(segment)) return true;
  const labels = lowered.split(".");
  const lastLabel = labels.at(-1);
  if (labels.length < 2 || lastLabel === undefined) return false;
  if (GENERIC_HOSTNAME_TLDS.has(lastLabel)) return true;
  return !hasPosition && COUNTRY_HOSTNAME_TLDS.has(lastLabel);
}

/**
 * The candidate in one inline code span, or `null` when it is not shaped like
 * a relative file path. Backslashes read as separators.
 */
export function inlineCodeFilePathCandidate(codeText: string): string | null {
  const trimmed = codeText.trim();
  if (trimmed.length === 0 || INLINE_CODE_DISQUALIFIER_PATTERN.test(trimmed)) {
    return null;
  }
  if (ABSOLUTE_PATTERN.test(trimmed)) return null;
  const candidate = trimmed.replaceAll("\\", "/");
  const hasPosition = POSITION_SUFFIX_PATTERN.test(candidate);
  if (!hasPosition && !PATH_SEPARATOR_PATTERN.test(candidate)) return null;

  if (!RELATIVE_PATH_PREFIX_PATTERN.test(candidate)) {
    const withoutPosition = candidate.replace(POSITION_SUFFIX_PATTERN, "");
    if (withoutPosition.includes(":")) return null;
    const firstSegment = withoutPosition.split("/")[0] ?? withoutPosition;
    if (looksLikeHostname(firstSegment, hasPosition)) return null;
    const basename =
      withoutPosition.replace(/\/+$/, "").split("/").at(-1) ?? "";
    if (VERSION_SUFFIX_PATTERN.test(basename)) return null;
    if (!hasPosition && !FILE_EXTENSION_PATTERN.test(basename)) return null;
  }
  return candidate;
}

function safeDecode(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

/**
 * The candidate behind a markdown link's href, or `null`. Only relative hrefs
 * qualify — a scheme (`https:`, `file:`, `beekeeper:`), a protocol-relative
 * `//`, an absolute path or a bare `#anchor` never does. A `#L12C3` fragment
 * becomes `:12:3`, so link and inline code share one candidate form.
 */
export function markdownHrefFilePathCandidate(
  href: string | undefined,
): string | null {
  if (!href) return null;
  let value = href.trim();
  if (value.startsWith("<") && value.endsWith(">")) value = value.slice(1, -1);
  if (value.length === 0 || value.startsWith("#") || value.startsWith("//")) {
    return null;
  }
  if (EXTERNAL_SCHEME_PATTERN.test(value)) return null;
  let position = "";
  const hashAt = value.indexOf("#");
  if (hashAt >= 0) {
    const match = POSITION_HASH_PATTERN.exec(value.slice(hashAt));
    if (match)
      position = match[2] ? `:${match[1]}:${match[2]}` : `:${match[1]}`;
    value = value.slice(0, hashAt);
  }
  if (value.includes("?")) return null;
  const decoded = safeDecode(value);
  // A link names its file explicitly, so the hostname/extension heuristics
  // that guard inline code are relaxed to "has a separator or an extension".
  if (decoded.length === 0 || /\s{2,}|`/.test(decoded)) return null;
  if (ABSOLUTE_PATTERN.test(decoded)) return null;
  const path = decoded.replaceAll("\\", "/");
  if (path.includes(":")) return null;
  const basename = path.replace(/\/+$/, "").split("/").at(-1) ?? "";
  if (
    !PATH_SEPARATOR_PATTERN.test(path) &&
    !FILE_EXTENSION_PATTERN.test(basename)
  ) {
    return null;
  }
  const first = path.split("/")[0] ?? "";
  if (
    !RELATIVE_PATH_PREFIX_PATTERN.test(path) &&
    looksLikeHostname(first, false)
  ) {
    return null;
  }
  return `${path}${position}`;
}

/** A candidate split into its path and its 1-based position, when given. */
export type FilePathPosition = {
  path: string;
  line?: number;
  column?: number;
};

/** Split a trailing `:line[:col]`; a zero line is no line. */
export function splitFilePathPosition(candidate: string): FilePathPosition {
  const match = POSITION_SUFFIX_CAPTURE_PATTERN.exec(candidate);
  if (!match) return { path: candidate };
  const path = candidate.slice(0, match.index);
  const line = Number(match[1]);
  if (!Number.isInteger(line) || line <= 0) return { path };
  const column = match[2] === undefined ? undefined : Number(match[2]);
  return column !== undefined && column > 0
    ? { path, line, column }
    : { path, line };
}

/** The last path segment, ignoring a trailing slash. */
export function filePathBasename(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  return trimmed.split("/").at(-1) || trimmed;
}

/** The chip's label: `App.tsx`, `App.tsx · L42`, `App.tsx · L42:C3`. */
export function filePathChipLabel(candidate: string): string {
  const { path, line, column } = splitFilePathPosition(candidate);
  const base = filePathBasename(path);
  if (line === undefined) return base;
  return column === undefined
    ? `${base} · L${line}`
    : `${base} · L${line}:C${column}`;
}
