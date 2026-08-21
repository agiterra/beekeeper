import { createFileRoute } from "@tanstack/react-router";

import { ShellObserveScreen } from "@/features/builtin-shell/ui/ShellObserveScreen";

type ObserveRouteSearch = {
  /** The `30621:<owner>:<dtag>` coordinate the session is shared under. */
  project: string;
};

function validateObserveSearch(
  search: Record<string, unknown>,
): ObserveRouteSearch {
  return {
    project: typeof search.project === "string" ? search.project : "",
  };
}

export const Route = createFileRoute("/observe/$owner/$sessionId")({
  validateSearch: validateObserveSearch,
  component: ObserveRouteComponent,
});

function ObserveRouteComponent() {
  const { owner, sessionId } = Route.useParams();
  const { project } = Route.useSearch();
  return (
    <ShellObserveScreen
      ownerPubkey={owner}
      sessionId={sessionId}
      projectRef={project}
    />
  );
}
