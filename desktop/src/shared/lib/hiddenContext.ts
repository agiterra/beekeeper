/**
 * How a privacy redaction marker reads on screen: a quiet `hidden path` (or
 * `hidden`) chip in place of `[elided private context: N bytes, sha256:…]`.
 *
 * The marker itself carries no class — the provider does not say whether it
 * replaced a path, a credential or provider bookkeeping — so the word "path"
 * is earned only from the text *around* the marker: a path separator touching
 * it, a key that names a path (`"cwd": "…"`), a command that takes one
 * (`cd …`), or a caller that knows the whole line is a path (`pwd` output).
 * Anything else is just `hidden`. The chip never guesses at the content; the
 * digest stays one hover or click away for whoever needs to verify it.
 */

import type {
  RedactionMarker,
  RedactionSegment,
} from "@/shared/lib/redactionMarker";

/** Characters of digest shown in the description before the ellipsis. */
export const HIDDEN_DIGEST_PREFIX_LENGTH = 12;

/** A key that names a filesystem location, ending right before the value. */
const PATH_KEY_BEFORE =
  /(?:^|[^A-Za-z0-9_])(?:cwd|pwd|path|paths|file|file_?path|filepath|filename|dir|directory|dirname|workdir|work_?dir|working_?dir|working_?directory|root|home|repo|repository|worktree|checkout|location|source|target|dest|destination)["']?\s*[:=]\s*["'`]?$/i;

/** A shell command whose next argument is a path. */
const PATH_COMMAND_BEFORE =
  /(?:^|[\s;&|(`$])(?:cd|pushd|ls|cat|less|head|tail|mkdir|rmdir|rm|cp|mv|touch|open|stat|realpath|readlink|source|git\s+-C)\s+(?:-{1,2}[\w-]+\s+)*["']?$/;

/**
 * Is the marker between `before` and `after` standing in for a path?
 *
 * Only adjacent text counts: a separator touching the marker
 * (`[marker]/src/main.rs`, `~/[marker]`), a path-naming key, or a path-taking
 * command immediately before it.
 */
export function isPathShapedContext(before: string, after: string): boolean {
  if (/^[/\\]/.test(after)) return true;
  if (/(?:[/\\]|~)$/.test(before)) return true;
  // Only the current line of `before` can name the value.
  const line = before.slice(before.lastIndexOf("\n") + 1);
  return PATH_KEY_BEFORE.test(line) || PATH_COMMAND_BEFORE.test(line);
}

/** Is this marker the entire content of its line (ignoring whitespace)? */
function isWholeLine(before: string, after: string): boolean {
  const lineBefore = before.slice(before.lastIndexOf("\n") + 1);
  const newline = after.indexOf("\n");
  const lineAfter = newline === -1 ? after : after.slice(0, newline);
  return lineBefore.trim() === "" && lineAfter.trim() === "";
}

/** A parsed segment, with redactions annotated by their surroundings. */
export type HiddenContextSegment =
  | { kind: "text"; text: string }
  | ({ kind: "redaction"; pathShaped: boolean } & RedactionMarker);

/**
 * Annotate `parseRedactionMarkers` output with whether each marker sits in a
 * path-shaped context.
 *
 * `wholeLinesArePaths` is for a caller that knows every line of the text is a
 * path — the output of `pwd`, say — so a marker alone on its line is one.
 */
export function annotateHiddenContext(
  segments: ReadonlyArray<RedactionSegment>,
  { wholeLinesArePaths = false }: { wholeLinesArePaths?: boolean } = {},
): HiddenContextSegment[] {
  return segments.map((segment, index) => {
    if (segment.kind === "text") return segment;
    const before = textAt(segments, index - 1);
    const after = textAt(segments, index + 1);
    const pathShaped =
      isPathShapedContext(before, after) ||
      (wholeLinesArePaths && isWholeLine(before, after));
    return { ...segment, pathShaped };
  });
}

function textAt(
  segments: ReadonlyArray<RedactionSegment>,
  index: number,
): string {
  const segment = segments[index];
  // A neighbouring marker is not text a reader could see; treat it as empty.
  return segment?.kind === "text" ? segment.text : "";
}

/** The chip's visible word. */
export function hiddenContextLabel(pathShaped: boolean): string {
  return pathShaped ? "hidden path" : "hidden";
}

/**
 * The tooltip's first line and the chip's accessible name:
 * `Hidden before publishing — 148 bytes, sha256 eb7930a9c1d2…`.
 *
 * An unreadable byte count (a marker rebuilt from HAST attributes that lost
 * it) is said as unknown rather than printed as `NaN`.
 */
export function hiddenContextDescription(
  marker: Pick<RedactionMarker, "bytes" | "digest">,
): string {
  const size =
    Number.isFinite(marker.bytes) && marker.bytes >= 0
      ? `${marker.bytes} ${marker.bytes === 1 ? "byte" : "bytes"}`
      : "size unknown";
  const digest = marker.digest
    ? `sha256 ${marker.digest.slice(0, HIDDEN_DIGEST_PREFIX_LENGTH)}…`
    : "no digest recorded";
  return `Hidden before publishing — ${size}, ${digest}`;
}
