/**
 * Remark plugin that turns a coding-session privacy redaction marker into a
 * custom HAST `redaction` element, rendered as a pill by `markdown.tsx`.
 *
 * The marker's shape and the reasons it is read rather than rewritten live in
 * `./redactionMarker`. Two behaviours worth stating here:
 *
 * - **Fenced and inline code are skipped**, via the shared prefix-plugin's
 *   `shouldSkipNode`. Inside a fence the literal marker *is* the honest
 *   rendering — the reader is looking at the bytes, and a pill would claim the
 *   bytes say something they do not.
 * - **The byte count and digest travel as `hProperties`**, not as parsed
 *   objects, because the element crosses react-markdown's HAST boundary. They
 *   are re-read by the renderer, which is why the pattern that produced them
 *   is the single source of truth for both directions.
 */

import { createRemarkPrefixPlugin } from "./createRemarkPrefixPlugin";
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

  return createRemarkPrefixPlugin(pattern, (matchText) => {
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
}
