import { Bot, Pencil } from "lucide-react";
import type * as React from "react";
import type { ReactNode } from "react";

import type { CodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";

import { CodingSessionFullAccessBadge } from "./CodingSessionFullAccessBadge";
import { CodingSessionHeaderStatusBadge } from "./CodingSessionHeaderParts";
import {
  CodingSessionTitleOrigin,
  type CodingSessionTitleOriginFacts,
} from "./CodingSessionTitleOrigin";

/**
 * The header's left side: `project / title ● Working` (SV-20).
 *
 * Ported from T3 Code's `ChatHeader` breadcrumb (`components/chat/ChatHeader.tsx`
 * and `components/WorkspaceBreadcrumb.tsx`): a `nav` > `ol` whose first item is
 * the project, muted, then a `/` separator, then the title as the current page.
 * Three deliberate differences, each for a Beekeeper rule:
 *
 * - **The status word stays.** T3 shows no status in its header; here the word
 *   sits beside the title, and a demoted status keeps its history clause in
 *   the tooltip, because the status is the fact a person acts on.
 * - **Rename is the founder's signed rename.** T3 renames inline on
 *   double-click; here double-click and the hover pencil open the same
 *   founder-authorized rename the menu always did (`onRename`), because a
 *   session name is a signed event, not local text.
 * - **The project opens the project.** T3's project crumb starts a new thread;
 *   here it navigates to the owning project, as the old crumb did. Without a
 *   way to open it, the project is plain text, never a dead link.
 *
 * The seat chip stays beside the title, so an agent's session never reads as
 * a person's, and so do the full-access badge and the lens control.
 *
 * A provider's generated title carries the muted "Auto-named" marker right
 * after it (SV-70), the same marker and tooltip the sidebar shelf, the
 * Sessions menu, Pulse and Agent Progress rows show (SV-31), so the header
 * never presents a model's words as a person's name. It is `shrink-0` and one
 * `text-2xs` word, so the title keeps its share of the row (SV-57).
 */
export function CodingSessionHeaderBreadcrumb({
  onOpenProject,
  onRename,
  projectName,
  seat,
  fullAccess,
  sessionClosed,
  status,
  statusDetail,
  statusWord,
  title,
  titleOrigin = null,
  viewControl,
}: {
  onOpenProject?: () => void;
  onRename?: () => void;
  projectName: string | null;
  seat: { label: string } | null;
  fullAccess?: React.ComponentProps<
    typeof CodingSessionFullAccessBadge
  >["fullAccess"];
  sessionClosed: boolean;
  status: CodingSessionWorkspaceStatus;
  statusDetail: string | null;
  statusWord: string;
  title: string;
  /** Whose words `title` is; a generated title is marked (SV-70). */
  titleOrigin?: CodingSessionTitleOriginFacts | null;
  viewControl?: ReactNode;
}) {
  const project = projectName?.trim() || null;
  return (
    <nav
      aria-label="Session breadcrumb"
      className="group/title min-w-0 flex-1"
      data-testid="coding-session-breadcrumb"
    >
      <ol className="m-0 flex min-w-0 list-none items-center gap-2 p-0 text-sm">
        {project ? (
          <>
            <li className="flex min-w-0 max-w-40 shrink-0 items-center text-muted-foreground">
              {onOpenProject ? (
                <button
                  className="min-w-0 truncate rounded-sm transition-colors hover:text-foreground focus-visible:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  data-testid="coding-session-project-crumb"
                  onClick={onOpenProject}
                  title={`Open ${project}`}
                  type="button"
                >
                  {project}
                </button>
              ) : (
                <span
                  className="min-w-0 truncate"
                  data-testid="coding-session-project-label"
                >
                  {project}
                </span>
              )}
            </li>
            <li
              aria-hidden
              className="flex shrink-0 items-center text-muted-foreground/60"
            >
              /
            </li>
          </>
        ) : null}
        <li
          aria-current="page"
          className="flex min-w-0 flex-1 items-center gap-1.5"
        >
          <h1
            className="min-w-0 truncate text-sm font-medium text-foreground"
            data-testid="coding-session-title"
            // T3's gesture. The pencil beside it is the keyboard way in.
            onDoubleClick={
              onRename
                ? (event) => {
                    if (
                      event.metaKey ||
                      event.ctrlKey ||
                      event.shiftKey ||
                      event.altKey
                    ) {
                      return;
                    }
                    onRename();
                  }
                : undefined
            }
            title={onRename ? `${title} — double-click to rename` : title}
          >
            {title}
          </h1>
          <CodingSessionTitleOrigin
            name={titleOrigin}
            testId="coding-session-header-title-origin"
          />
          {onRename ? (
            // An edit affordance of the title, not an action in the run: it
            // shows on hover or keyboard focus of the title, and stays in the
            // tab order the whole time.
            <Button
              aria-label="Rename session"
              className="shrink-0 opacity-0 transition-opacity group-hover/title:opacity-100 focus-visible:opacity-100"
              data-testid="coding-session-rename"
              onClick={onRename}
              size="icon-xs"
              title="Rename session"
              type="button"
              variant="ghost"
            >
              <Pencil />
            </Button>
          ) : null}
          <CodingSessionHeaderStatusBadge
            detail={statusDetail}
            sessionClosed={sessionClosed}
            status={status}
            word={statusWord}
          />
          {seat ? (
            <Badge
              className="shrink-0 gap-1.5"
              data-testid="coding-session-header-seat"
              title={`Seated: ${seat.label}`}
              variant="outline"
            >
              <Bot aria-hidden className="size-3" />
              {seat.label}
            </Badge>
          ) : null}
          <CodingSessionFullAccessBadge fullAccess={fullAccess} />
          {viewControl ? (
            <div className="ml-1 shrink-0">{viewControl}</div>
          ) : null}
        </li>
      </ol>
    </nav>
  );
}
