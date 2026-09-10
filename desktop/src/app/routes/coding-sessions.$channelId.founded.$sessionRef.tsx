import { createFileRoute } from "@tanstack/react-router";

import { CodingSessionFoundedWorkspace } from "@/features/coding-sessions/ui/founded/CodingSessionFoundedWorkspace";

/**
 * A founded umbrella that has no execution yet: a genesis with a goal and a
 * name, waiting for somebody to pick who leads and start it. Keyed by
 * `sessionRef` — the only identity such a session has — and handing off to
 * the generation route the moment one exists.
 */
export const Route = createFileRoute(
  "/coding-sessions/$channelId/founded/$sessionRef",
)({
  component: FoundedCodingSessionRouteComponent,
});

function FoundedCodingSessionRouteComponent() {
  const { channelId, sessionRef } = Route.useParams();
  return (
    <CodingSessionFoundedWorkspace
      channelId={channelId}
      sessionRef={sessionRef}
    />
  );
}
