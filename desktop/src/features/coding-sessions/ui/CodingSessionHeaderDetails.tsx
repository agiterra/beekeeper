import { ChevronRight, Info, Users } from "lucide-react";
import * as React from "react";
import type { ReactNode } from "react";

import { Button } from "@/shared/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";

import {
  CodingSessionProvenanceDetails,
  type CodingSessionContextRow,
  type CodingSessionRoutedSeatRow,
} from "./CodingSessionHeaderProvenance";
import {
  CodingSessionDetailsContinuity,
  useCodingSessionDetailsContinuity,
} from "./CodingSessionHeaderDetailsContinuity";

/**
 * One `Details` control for the session header: who has access, and where
 * this session came from (SESSION_VIEW_UX_PLAN L4 review, item 4).
 *
 * It replaces two row controls — `People N` and the provenance `ⓘ` — that
 * answered the same question ("what is this session, and who is in it?") from
 * opposite ends of the row. The People count stays on the trigger, because a
 * count you have to open something to see signals nothing.
 *
 * Both former targets keep their test ids: the trigger is still
 * `coding-session-provenance-toggle`, the popover still holds
 * `coding-session-provenance-details` verbatim, and `coding-session-people-
 * toggle` is now the popover's People row, which opens the same People
 * dialog it always did. The roster itself stays in that dialog: it is where
 * invites and revokes happen, and a popover is too small to manage access in.
 *
 * Session continuity lives here too (SV-16): each execution start's "fresh /
 * resumed / restarted without context" disclosure, read from the transcript
 * by the workspace. A start that lost context puts a warning dot on the
 * trigger, so moving the row out of the transcript does not hide it.
 */
export function CodingSessionHeaderDetails({
  channelName,
  compact,
  contextLoads,
  founderDetails,
  generationLabel,
  onOpenPeople,
  peopleCount,
  projectName,
  providerAuthorityPubkey,
  routedSeats,
}: {
  channelName: string | null;
  compact: boolean;
  contextLoads?: readonly CodingSessionContextRow[];
  founderDetails?: ReactNode;
  generationLabel: string;
  onOpenPeople?: () => void;
  peopleCount: number;
  projectName: string | null;
  providerAuthorityPubkey: string | null;
  routedSeats?: readonly CodingSessionRoutedSeatRow[];
}) {
  const [open, setOpen] = React.useState(false);
  const showCount = Boolean(onOpenPeople) && peopleCount > 0;
  const peopleNoun = peopleCount === 1 ? "person" : "people";
  const continuity = useCodingSessionDetailsContinuity();
  const contextLost = continuity[0]?.lost === true;
  const triggerLabel = [
    showCount
      ? `Show session details: ${peopleCount} ${peopleNoun} with access`
      : "Show session details",
    contextLost ? "the latest start lost prior context" : null,
  ]
    .filter(Boolean)
    .join("; ");
  return (
    <Popover onOpenChange={setOpen} open={open}>
      <PopoverTrigger asChild>
        <Button
          aria-label={triggerLabel}
          data-testid="coding-session-provenance-toggle"
          className={contextLost ? "relative" : undefined}
          size={compact && !showCount ? "icon" : "sm"}
          title={
            contextLost
              ? "This agent restarted without its prior context. Details has the continuity record, people with access, and provenance."
              : "People with access, session continuity, and where this session came from"
          }
          type="button"
          variant={open ? "secondary" : "ghost"}
        >
          <Info />
          <span className={compact ? "sr-only" : undefined}>Details</span>
          {showCount ? (
            <span
              aria-hidden
              className="inline-flex items-center gap-1 rounded-full bg-background/70 px-1.5 text-xs"
              data-testid="coding-session-details-people-count"
            >
              <Users className="size-3" />
              {peopleCount}
            </span>
          ) : null}
          {contextLost ? (
            <span
              aria-hidden
              className="absolute top-1 right-1 size-1.5 rounded-full bg-amber-500"
              data-testid="coding-session-details-continuity-dot"
            />
          ) : null}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-72">
        {onOpenPeople ? (
          <button
            aria-label="Show session people"
            className="-mx-1 mb-3 flex w-[calc(100%+0.5rem)] items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            data-testid="coding-session-people-toggle"
            onClick={() => {
              // The People dialog takes over; leaving this popover open
              // behind it would be two surfaces answering one click.
              setOpen(false);
              onOpenPeople();
            }}
            title="People with access to this session"
            type="button"
          >
            <Users aria-hidden className="size-4 shrink-0" />
            <span className="min-w-0 flex-1">
              <span className="block font-medium">People</span>
              <span className="block text-xs text-muted-foreground">
                {peopleCount > 0
                  ? `${peopleCount} ${peopleNoun} with access · manage`
                  : "Who has access · manage"}
              </span>
            </span>
            <ChevronRight
              aria-hidden
              className="size-3.5 shrink-0 text-muted-foreground"
            />
          </button>
        ) : null}
        <CodingSessionDetailsContinuity rows={continuity} />
        <CodingSessionProvenanceDetails
          channelName={channelName}
          contextLoads={contextLoads}
          founderDetails={founderDetails}
          generationLabel={generationLabel}
          projectName={projectName}
          providerAuthorityPubkey={providerAuthorityPubkey}
          routedSeats={routedSeats}
        />
      </PopoverContent>
    </Popover>
  );
}
