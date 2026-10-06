/**
 * Remark plugin that turns a coding-session privacy redaction marker into a
 * custom HAST `redaction` element, rendered as a pill by `markdown.tsx`.
 *
 * The marker's shape and the reasons it is read rather than rewritten live in
 * `./redactionMarker`. Two behaviours worth stating here:
 *
 * - **Fenced and inline code are skipped here**, via the shared prefix-plugin's
 *   `shouldSkipNode`, and the pill for those is applied by the `code`
 *   component in `markdown.tsx` instead. This plugin cannot do it: a
 *   `code`/`inlineCode` node carries a string `value`, not children, so there
 *   is nothing to split.
 *
 *   That split of responsibility replaced an earlier decision to leave code
 *   alone entirely, on the reasoning that inside a fence the literal marker is
 *   the honest rendering. It is not — the bytes the reader would be looking at
 *   were replaced before the item was signed, so a fence shows ninety
 *   characters of hash where content used to be, and (worse) the machine that
 *   redacted it cannot reveal its own path back. Agents write host paths in
 *   backticks and paste console output in fences, so that was where most
 *   redactions landed.
 * - **The byte count and digest travel as `hProperties`**, not as parsed
 *   objects, because the element crosses react-markdown's HAST boundary. They
 *   are re-read by the renderer, which is why the pattern that produced them
 *   is the single source of truth for both directions.
 */

import { createRemarkPrefixPlugin } from "./createRemarkPrefixPlugin";
import { isPathShapedContext } from "./hiddenContext";
import { REDACTION_MARKER_PATTERN } from "./redactionMarker";

export default function remarkRedactionMarkers() {
  // A fresh RegExp per plugin instance: the shared factory resets `lastIndex`
  // before each text node, but the `g`-flagged module constant is also used by
  // `parseRedactionMarkers`, and two consumers sharing mutable regex state is
  // a bug waiting for the first concurrent render.
  const pattern = new RegExp(
    REDACTION_MARKER_PATTERN.source,
    REDACTION_MARKER_PATTERN.flags,
  );

  const split = createRemarkPrefixPlugin(pattern, (matchText) => {
    const match = new RegExp(REDACTION_MARKER_PATTERN.source).exec(matchText);
    return {
      type: "redaction",
      value: matchText,
      data: {
        hName: "redaction",
        hProperties: {
          "data-redaction-bytes": match?.[1] ?? "",
          "data-redaction-digest": match?.[2] ?? "",
        },
        hChildren: [],
      },
    };
  });

  // biome-ignore lint/suspicious/noExplicitAny: remark tree types are not available
  return (tree: any) => {
    split(tree);
    markPathShapedRedactions(tree);
  };
}

/**
 * After the split, a redaction node's text neighbours are its siblings; mark
 * the ones whose surroundings say they stood for a path, so the chip can read
 * `hidden path` instead of `hidden` (see `hiddenContext.ts`).
 */
// biome-ignore lint/suspicious/noExplicitAny: remark tree types are not available
function markPathShapedRedactions(node: any) {
  if (!Array.isArray(node?.children)) return;
  node.children.forEach(
    // biome-ignore lint/suspicious/noExplicitAny: remark tree types are not available
    (child: any, index: number, siblings: any[]) => {
      if (child.type !== "redaction") {
        markPathShapedRedactions(child);
        return;
      }
      const before = siblings[index - 1];
      const after = siblings[index + 1];
      const pathShaped = isPathShapedContext(
        before?.type === "text" ? before.value : "",
        after?.type === "text" ? after.value : "",
      );
      if (pathShaped) {
        child.data.hProperties["data-redaction-path"] = "true";
      }
    },
  );
}
