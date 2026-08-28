import type { ProjectContainer } from "@/features/projects-container/hooks";
import { ProjectsListScopeDropdown } from "@/features/projects/ui/ProjectsListScopeDropdown";

import {
  ROLE_PACKS_PROJECT_SELECTOR_ARIA,
  ROLE_PACKS_PROJECT_SELECTOR_LABEL,
} from "./installCrewRolesCopy";

type RolePacksProjectSelectorProps = {
  /** The project the installer would read packs from right now. */
  project: ProjectContainer | null;
  /** Every project the viewer can choose between, in the app's own order. */
  projects: readonly ProjectContainer[];
  /** The project id the operator picked. */
  onSelect: (projectId: string) => void;
};

/**
 * "Role packs for: <project> ▾" — the Agents tab saying which project's role
 * packs its installer will open on.
 *
 * The tab's route names no project (ledger 85's reachability finding), so the
 * installer resolves one. With more than one to resolve between, the resolved
 * one is on the surface and switchable: an operator who sees the wrong
 * project can say so before installing, instead of discovering it from the
 * identities that appeared afterwards.
 *
 * With nothing to choose between — one project, or none — this renders
 * nothing. The single project is still named, on the installer's own folder
 * label; a control offering one option would only be furniture.
 */
export function RolePacksProjectSelector({
  project,
  projects,
  onSelect,
}: RolePacksProjectSelectorProps) {
  if (!project || projects.length < 2) return null;

  return (
    <div
      className="flex items-center gap-1 text-xs text-muted-foreground"
      data-testid="role-packs-project-selector"
    >
      <span>{ROLE_PACKS_PROJECT_SELECTOR_LABEL}</span>
      <ProjectsListScopeDropdown
        label={ROLE_PACKS_PROJECT_SELECTOR_ARIA}
        onChange={onSelect}
        options={projects.map((candidate) => ({
          label: candidate.name,
          value: candidate.id,
        }))}
        triggerTestId="role-packs-project-trigger"
        value={project.id}
      />
    </div>
  );
}
