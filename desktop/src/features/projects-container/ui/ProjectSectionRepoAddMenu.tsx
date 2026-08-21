import { FolderInput, FolderPlus, Link, Plus } from "lucide-react";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

/**
 * The Code section's `+` menu: create a new repository in the project or
 * move an existing one in. Trigger styling matches the sibling sections'
 * plain `+` buttons. The attach entry is hidden while no repo outside the
 * project exists to move in.
 */
export function ProjectSectionRepoAddMenu({
  attachAvailable,
  onAttachExisting,
  onCreateNew,
  onImportLocal,
}: {
  attachAvailable: boolean;
  onAttachExisting: () => void;
  onCreateNew: () => void;
  onImportLocal: () => void;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label="Add repository"
          data-testid="project-section-create-repo"
          className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
        >
          <Plus className="size-4" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem
          data-testid="project-section-create-repo-new"
          onSelect={onCreateNew}
        >
          <FolderPlus className="h-4 w-4" />
          Create new repository
        </DropdownMenuItem>
        {attachAvailable ? (
          <DropdownMenuItem
            data-testid="project-section-create-repo-attach"
            onSelect={onAttachExisting}
          >
            <Link className="h-4 w-4" />
            Add existing repository
          </DropdownMenuItem>
        ) : null}
        <DropdownMenuItem
          data-testid="project-section-create-repo-import"
          onSelect={onImportLocal}
        >
          <FolderInput className="h-4 w-4" />
          Import local repository
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
