import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const PlanningScreen = React.lazy(async () => {
  const module = await import("@/features/planning/ui/PlanningScreen");
  return { default: module.PlanningScreen };
});

/**
 * An issue key, `<containerId>/<issueId>`, in any provider's id alphabet: a
 * native list and item id are 32 hex, a Linear team and issue id are UUIDs.
 * The screen looks the key up in the read and says "not found" when no issue
 * has it, so the route only bounds the shape.
 */
const ISSUE_KEY = /^[A-Za-z0-9._~-]{1,128}\/[A-Za-z0-9._~-]{1,128}$/;

/**
 * The project's Planning tab: its issues as a list or a board. `issue`
 * names the open issue by its key, so a reload or a shared link lands on
 * it; anything that is not key-shaped is dropped.
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
