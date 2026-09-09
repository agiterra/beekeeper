import {
  GENERAL_PROJECT_DTAG,
  LOCAL_GENERAL_ID,
  type ProjectContainer,
} from "@/features/projects-container/lib/projectContainerModel";

/** Keep the General placeholder selected when its published identity arrives. */
export function resolveProjectScope(
  selected: string,
  projects: readonly Pick<ProjectContainer, "id" | "dtag">[],
): string {
  if (
    selected === "all" ||
    projects.some((project) => project.id === selected)
  ) {
    return selected;
  }
  if (selected === LOCAL_GENERAL_ID) {
    // The display list normally contains one canonical General. Do not guess
    // a target if an unresolved list contains more than one.
    const generals = projects.filter(
      (project) => project.dtag === GENERAL_PROJECT_DTAG,
    );
    if (generals.length === 1) return generals[0].id;
  }
  return "all";
}
