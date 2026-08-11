import type { ProjectsFilter } from "@/features/projects/lib/projectsViewHelpers";
import { ProjectsView } from "@/features/projects/ui/ProjectsView";

export function ProjectsScreen({
  initialFilter,
}: {
  initialFilter?: ProjectsFilter;
} = {}) {
  return (
    <div className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
      <ProjectsView initialFilter={initialFilter} />
    </div>
  );
}
