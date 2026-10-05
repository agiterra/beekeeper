import { Users } from "lucide-react";

import { CodingSessionSurfaceSubheader } from "../CodingSessionChangesRailSubheader";
import {
  CodingSessionPeopleBody,
  codingSessionPeopleIntro,
  useCodingSessionPeopleIsOwner,
} from "../CodingSessionPeopleBody";
import type {
  CodingSessionSurfaceAvailability,
  CodingSessionSurfaceCtx,
} from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";
import { CodingSessionSurfacePlaceholder } from "./CodingSessionSurfaceDevicePlaceholder";

/** People: when it can open, or the sentence why not (§3, SV-23). */
export function codingSessionSurfacePeopleAvailability(
  ctx: CodingSessionSurfaceCtx,
): CodingSessionSurfaceAvailability {
  return ctx.genesisRef !== null
    ? { available: true }
    : {
        available: false,
        reason: "This session predates sharing and has no roster.",
      };
}

/**
 * The People surface: the same roster and invite the People dialog shows
 * (`CodingSessionPeopleBody`), in the right panel. Inviting works from here.
 */
export function CodingSessionSurfacePeoplePanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const availability = codingSessionSurfacePeopleAvailability(ctx);
  const isOwner = useCodingSessionPeopleIsOwner(ctx.founderPubkey);
  if (!availability.available) {
    return (
      <CodingSessionSurfacePlaceholder
        icon={Users}
        id="people"
        label="People"
        reason={availability.reason}
      />
    );
  }
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-available="true"
      data-testid="coding-session-surface-panel-people"
    >
      <CodingSessionSurfaceSubheader
        meta="session access, from its signed authority chain"
        title="People"
      />
      <div
        className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto overscroll-contain p-3"
        data-testid="coding-session-people-surface"
      >
        <p className="text-xs text-muted-foreground">
          {codingSessionPeopleIntro(isOwner)}
        </p>
        <CodingSessionPeopleBody
          active
          channelId={ctx.channelId}
          founderPubkey={ctx.founderPubkey}
          genesisRef={ctx.genesisRef}
          variant="surface"
        />
      </div>
    </div>
  );
}

export const codingSessionSurfacePeople: CodingSessionSurfaceDefinition = {
  id: "people",
  label: "People",
  icon: Users,
  shortcut: "E",
  order: 70,
  placement: "right",
  lenses: ["conversation", "mission"],
  availability: codingSessionSurfacePeopleAvailability,
  Panel: CodingSessionSurfacePeoplePanel,
};
