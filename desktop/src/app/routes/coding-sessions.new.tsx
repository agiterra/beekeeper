import * as React from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { openNewCodingSessionDialog } from "@/features/coding-sessions/newCodingSessionDialogStore";

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
 * Creating a session is a dialog now, not a page — but this URL is in
 * people's history and in deep links, so it keeps working: it opens the
 * dialog and hands the window back to whatever was underneath.
 */
function NewCodingSessionRouteComponent() {
  const { channelId } = Route.useSearch();
  const navigate = useNavigate();
  React.useEffect(() => {
    openNewCodingSessionDialog(channelId ?? null);
    void navigate({ to: "/", replace: true });
  }, [channelId, navigate]);
  return null;
}
