import { createFileRoute } from "@tanstack/react-router";

import { NewCodingSessionScreen } from "@/features/coding-sessions/ui/NewCodingSessionScreen";

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

function NewCodingSessionRouteComponent() {
  const { channelId } = Route.useSearch();
  return <NewCodingSessionScreen channelId={channelId} />;
}
