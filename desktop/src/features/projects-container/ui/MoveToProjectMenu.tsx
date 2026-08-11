import { EllipsisVertical, FolderKanban } from "lucide-react";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

import type { ProjectContainer } from "../hooks";

/**
 * Hover-revealed "Move to project" menu for an item row. The trigger relies
 * on a `group/manage-row` class on an ancestor row for its hover reveal.
 * Shared by the all-projects manage panel and the per-project screen.
 */
export function MoveToProjectMenu({
  currentId,
  projects,
  onMove,
}: {
  currentId: string | null;
  projects: ProjectContainer[];
  onMove: (target: ProjectContainer) => void;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label="Move to project"
          data-testid="manage-item-move"
          className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground/60 opacity-0 transition-colors hover:text-foreground focus-visible:opacity-100 group-hover/manage-row:opacity-100 data-[state=open]:opacity-100"
          onPointerDown={(event) => event.stopPropagation()}
        >
          <EllipsisVertical className="size-4" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuLabel>Move to project</DropdownMenuLabel>
        {projects
          .filter((project) => project.id !== currentId)
          .map((project) => (
            <DropdownMenuItem key={project.id} onSelect={() => onMove(project)}>
              <FolderKanban />
              {project.name}
            </DropdownMenuItem>
          ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
