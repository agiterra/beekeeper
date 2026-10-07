import type * as React from "react";

import { markdownHrefFilePathCandidate } from "@/features/coding-sessions/lib/filePathCandidate";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";

import { ExternalLinkAnchor } from "./ExternalLinkAnchor";
import { FileChip } from "./FileChip";
import { presentFileRef, useFileRefScope } from "./fileRefContext";

/**
 * A markdown link that may name a file in the session's folder (SV-32).
 *
 * Drop-in for `ExternalLinkAnchor` (same props), so the hook in
 * `markdown.tsx` is a rename, not a new branch. Outside a coding-session
 * answer — no `FileRefContext` — and for any href that is not a relative file
 * path, it **is** `ExternalLinkAnchor`. Inside one, a relative href that
 * resolves on this computer is a chip, and one that does not is its label as
 * plain text with the reason, rather than a link to nowhere.
 *
 * The marker-href branch (a link whose destination was elided before signing,
 * slice S0) is not built here: S0 was not landed in this batch.
 */
export function FileLinkAnchor(
  props: React.ComponentProps<typeof ExternalLinkAnchor>,
) {
  const scope = useFileRefScope();
  const candidate = scope ? markdownHrefFilePathCandidate(props.href) : null;
  const presentation = presentFileRef(scope, candidate);
  if (presentation.kind === "chip" && scope && candidate) {
    return (
      <FileChip
        candidate={candidate}
        fileRef={presentation.ref}
        scope={scope}
      />
    );
  }
  if (presentation.kind === "plain") {
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            className="font-medium"
            data-file-ref-plain=""
            data-file-ref-reason={presentation.reason}
            data-testid="coding-session-file-ref-plain"
            // biome-ignore lint/a11y/noNoninteractiveTabindex: the reason must be reachable by keyboard
            tabIndex={0}
          >
            {props.children}
          </span>
        </TooltipTrigger>
        <TooltipContent className="max-w-xs">
          {presentation.reason}
        </TooltipContent>
      </Tooltip>
    );
  }
  return <ExternalLinkAnchor {...props} />;
}
