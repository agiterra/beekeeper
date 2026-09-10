import * as React from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { toast } from "sonner";

import { requestCodingSessionFounding } from "@/features/coding-sessions/newCodingSessionDialogStore";
import { CODING_SESSION_FOUNDING_NO_DESTINATION_SENTENCE } from "@/features/coding-sessions/ui/CodingSessionFoundingHost";

type NewCodingSessionRouteSearch = {
  channelId?: string;
};

function validateNewCodingSessionSearch(
  search: Record<string, unknown>,
): NewCodingSessionRouteSearch {
  return typeof search.channelId === "string" && search.channelId.length > 0
    ? { channelId: search.channelId }
    : {};
}

export const Route = createFileRoute("/coding-sessions/new")({
  validateSearch: validateNewCodingSessionSearch,
  component: NewCodingSessionRouteComponent,
});

/**
 * Creating a session is founded on the click now, not a page — but this URL
 * is in people's history and in deep links, so it keeps working: with a
 * channel it writes the founding request (the host in the app shell founds
 * the session and opens its page) and hands the window back to whatever was
 * underneath. Without a channel there is nowhere to found anything and no
 * honest way to guess one, so it goes home and says so.
 */
function NewCodingSessionRouteComponent() {
  const { channelId } = Route.useSearch();
  const navigate = useNavigate();
  React.useEffect(() => {
    if (channelId) requestCodingSessionFounding(channelId);
    else toast.info(CODING_SESSION_FOUNDING_NO_DESTINATION_SENTENCE);
    void navigate({ to: "/", replace: true });
  }, [channelId, navigate]);
  return null;
}
