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
  /**
   * Set when nobody chose `project` — it was resolved as a fallback — and
   * rendered beside it, so the fallback never reads as the selection.
   */
  fallbackNote?: string | null;
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
 * With nothing to choose between — a single selectable project — the
 * dropdown is left out, but the project is still named in plain text
 * (`role-packs-project-static`). That case is not rare: with General plus one
 * real project and the directory's filter on General, role packs resolve to
 * the other project, and a surface that went silent would show General while
 * Install reads another project's packs. So whenever a fallback note is
 * produced it is shown too. With no project at all this renders nothing.
 */
export function RolePacksProjectSelector({
  project,
  projects,
  onSelect,
  fallbackNote,
}: RolePacksProjectSelectorProps) {
  if (!project) return null;

  const note = fallbackNote ? (
    <span data-testid="role-packs-project-fallback">{fallbackNote}</span>
  ) : null;

  if (projects.length < 2) {
    return (
      <div
        className="flex flex-wrap items-center gap-1 text-xs text-muted-foreground"
        data-testid="role-packs-project-static"
      >
        <span>{ROLE_PACKS_PROJECT_SELECTOR_LABEL}</span>
        <span
          className="font-medium text-foreground"
          data-testid="role-packs-project-name"
        >
          {project.name}
        </span>
        {note}
      </div>
    );
  }

  return (
    <div
      className="flex flex-wrap items-center gap-1 text-xs text-muted-foreground"
      data-testid="role-packs-project-selector"
    >
      <span>{ROLE_PACKS_PROJECT_SELECTOR_LABEL}</span>
      <ProjectsListScopeDropdown
        label={ROLE_PACKS_PROJECT_SELECTOR_ARIA}
        modal={false}
        onChange={onSelect}
        options={projects.map((candidate) => ({
          label: candidate.name,
          value: candidate.id,
        }))}
        triggerTestId="role-packs-project-trigger"
        value={project.id}
      />
      {note}
    </div>
  );
}
