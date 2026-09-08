import { Button } from "@/shared/ui/button";

import type { PacksSourceSummary } from "../lib/rolesViewModel";
import {
  INSTALL_ROLES_BUTTON_LABEL,
  type PacksSourceDetail,
  packsSourceSentence,
  ROLES_RECHECK_BUSY_LABEL,
  ROLES_RECHECK_LABEL,
  ROLES_SUBTITLE,
  ROLES_TITLE,
} from "./rolesCopy";

/**
 * The Roles page header: what this page is, where the instructions on this
 * computer came from, and the one control that re-reads them.
 *
 * One line, not a paragraph. The longer explanation of what a role is
 * (`ROLES_EXPLANATION`) is still on the page — it moved into Technical
 * details, where a reader who needs it can open it, rather than standing
 * between the header and the roles on every visit.
 *
 * "Check again" calls the reads this page already makes — it adds no new
 * query and no polling. While a read this view can observe is in flight the
 * button is disabled and says so, rather than looking idle over a fetch.
 */
export function RolesHeader({
  busy,
  onInstall,
  onRecheck,
  packCount,
  packsSource,
  projectName,
  sourceDetail,
}: {
  busy: boolean;
  onInstall: () => void;
  onRecheck: () => void;
  packCount: number;
  packsSource: PacksSourceSummary;
  projectName: string;
  sourceDetail: PacksSourceDetail;
}) {
  const sentence = packsSourceSentence(
    projectName,
    packCount,
    packsSource,
    sourceDetail,
  );
  return (
    <header className="flex flex-col gap-1">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-xl font-semibold text-foreground">{ROLES_TITLE}</h2>
        <div className="flex flex-wrap items-center gap-1">
          <Button
            data-testid="roles-recheck"
            disabled={busy}
            onClick={onRecheck}
            size="sm"
            type="button"
            variant="ghost"
          >
            {busy ? ROLES_RECHECK_BUSY_LABEL : ROLES_RECHECK_LABEL}
          </Button>
          <Button
            data-testid="project-packs-install"
            onClick={onInstall}
            size="sm"
            type="button"
            variant="outline"
          >
            {INSTALL_ROLES_BUTTON_LABEL}
          </Button>
        </div>
      </div>
      <p className="text-sm text-muted-foreground" data-testid="roles-subtitle">
        {ROLES_SUBTITLE}
      </p>
      <p
        className="min-w-0 text-xs text-muted-foreground"
        data-testid="packs-source-sentence"
        title={sentence.shaTitle ?? sentence.text}
      >
        {sentence.text}
      </p>
    </header>
  );
}
