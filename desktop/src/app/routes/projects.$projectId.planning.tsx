import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const PlanningScreen = React.lazy(async () => {
  const module = await import("@/features/planning/ui/PlanningScreen");
  return { default: module.PlanningScreen };
});

const ISSUE_KEY = /^[0-9a-f]{32}\/[0-9a-f]{32}$/;

/**
 * The project's Planning tab: its issues as a list or a board. `issue`
 * names the open issue as `<listId>/<itemId>`, so a reload or a shared link
 * lands on it; anything else is dropped.
 */
export const Route = createFileRoute("/projects/$projectId/planning")({
  component: PlanningRouteComponent,
  validateSearch: (search: Record<string, unknown>): { issue?: string } =>
    typeof search.issue === "string" && ISSUE_KEY.test(search.issue)
      ? { issue: search.issue }
      : {},
});

function PlanningRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();
  const { issue } = Route.useSearch();
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <PlanningScreen issueKey={issue ?? null} projectId={projectId} />
    </React.Suspense>
  );
}
