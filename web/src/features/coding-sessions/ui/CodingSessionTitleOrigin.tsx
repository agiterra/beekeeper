import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";
import {
  type CodingSessionUmbrella,
  codingSessionTitleOriginDetail,
} from "../domain/index.ts";

/**
 * The "Auto-named" marker beside a session's name (SV-31).
 *
 * A generated title is a model's words, so it is never allowed to read as a
 * person's name: the marker says so, and its tooltip says which execution's
 * provider wrote it and with which model. A person's name (`person`) and the
 * fallback show no marker — the fallback is the founding execution's own
 * title, not a guess dressed as one.
 */
export function CodingSessionTitleOrigin({
  umbrella,
}: {
  umbrella: CodingSessionUmbrella;
}) {
  const detail = codingSessionTitleOriginDetail(umbrella);
  if (detail === null) return null;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className="shrink-0 cursor-default text-xs font-normal text-black/45 dark:text-white/45"
          data-testid="coding-session-title-origin"
        >
          Auto-named
        </span>
      </TooltipTrigger>
      <TooltipContent
        side="top"
        className="max-w-xs"
        data-testid="coding-session-title-origin-detail"
      >
        {detail}
      </TooltipContent>
    </Tooltip>
  );
}
