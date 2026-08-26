import { createRemarkPrefixPlugin } from "./createRemarkPrefixPlugin";

const PRIVATE_CONTEXT_MARKER =
  /\[elided private context: \d+ bytes, sha256:[0-9a-f]{64}\]/g;
const PRIVATE_CONTEXT_FIELDS =
  /^\[elided private context: (\d+) bytes, sha256:([0-9a-f]{64})\]$/;

/** Render durable privacy markers as compact semantic chips, not inline hex. */
export default function remarkPrivateContextMarkers() {
  return createRemarkPrefixPlugin(PRIVATE_CONTEXT_MARKER, (matchText) => {
    const match = PRIVATE_CONTEXT_FIELDS.exec(matchText);
    if (!match) return { type: "text", value: matchText };
    const [, byteCount, digest] = match;
    const label = `Private context · ${byteCount} bytes`;
    return {
      type: "private-context",
      value: label,
      data: {
        hName: "private-context",
        hProperties: {
          "data-private-context": "",
          title: `Private context withheld · ${byteCount} bytes · sha256:${digest}`,
        },
        hChildren: [{ type: "text", value: label }],
      },
    };
  });
}
