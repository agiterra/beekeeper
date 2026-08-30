import {
  DndContext,
  DragOverlay,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import type { DragEndEvent, DragStartEvent } from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  verticalListSortingStrategy,
  useSortable,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { FolderKanban } from "lucide-react";
import * as React from "react";

/**
 * Drag-to-reorder for the per-project sidebar groups. The projects-side
 * counterpart to the channel sidebar's `SidebarDnd`, kept in its own file so
 * `ProjectSidebarSections` stays well clear of the 1000-line file gate.
 *
 * The General group is never passed through here: it is the fallback bucket
 * for unclaimed items, so it stays pinned above the sortable list.
 */

export type DndProjectData = { type: "project"; projectId: string };

export function SortableProjectShell({
  projectId,
  children,
}: {
  projectId: string;
  children: (props: {
    dragHandleProps: React.HTMLAttributes<HTMLElement>;
    isDragging: boolean;
  }) => React.ReactNode;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({
    id: projectId,
    data: { type: "project", projectId } satisfies DndProjectData,
  });

  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  return (
    <div ref={setNodeRef} style={style}>
      {children({
        // Handle-only: the group header is a button that opens the project, so
        // making the whole row draggable would fight the click.
        dragHandleProps: { ...attributes, ...listeners },
        isDragging,
      })}
    </div>
  );
}

export function DragOverlayProject({ name }: { name: string }) {
  return (
    <div
      data-buzz-flat
      className="flex cursor-grabbing items-center gap-2 rounded-md bg-sidebar px-2 py-1.5 text-sm text-sidebar-foreground opacity-90 shadow-lg ring-1 ring-sidebar-border"
      data-sidebar-drag-overlay
      data-testid="sidebar-project-drag-overlay"
    >
      <FolderKanban className="h-4 w-4 shrink-0 text-sidebar-foreground/60" />
      <span className="truncate">{name}</span>
    </div>
  );
}

/**
 * Wraps the sortable project groups.
 *
 * `projectIds` is the currently displayed order; `onReorderProjects` receives
 * the full permuted list, which becomes the user's saved order verbatim.
 */
export function ProjectSidebarDndContext({
  projects,
  children,
  onReorderProjects,
}: {
  projects: { id: string; name: string }[];
  children: React.ReactNode;
  onReorderProjects: (orderedIds: string[]) => void;
}) {
  const [activeProject, setActiveProject] = React.useState<{
    id: string;
    name: string;
  } | null>(null);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
  );

  const projectIds = React.useMemo(
    () => projects.map((project) => project.id),
    [projects],
  );

  const handleDragStart = React.useCallback(
    (event: DragStartEvent) => {
      const data = event.active.data.current;
      if (data?.type !== "project") return;
      const project = projects.find(
        (candidate) => candidate.id === data.projectId,
      );
      if (project) setActiveProject({ id: project.id, name: project.name });
    },
    [projects],
  );

  const handleDragEnd = React.useCallback(
    (event: DragEndEvent) => {
      setActiveProject(null);
      const { active, over } = event;
      if (!over) return;
      const overId =
        (over.data.current?.projectId as string | undefined) ??
        (over.id as string);
      const oldIdx = projectIds.indexOf(active.id as string);
      const newIdx = projectIds.indexOf(overId);
      if (oldIdx === -1 || newIdx === -1 || oldIdx === newIdx) return;
      onReorderProjects(arrayMove(projectIds, oldIdx, newIdx));
    },
    [projectIds, onReorderProjects],
  );

  return (
    <DndContext
      // Groups vary wildly in height (a project with ten channels dwarfs an
      // empty one), so rect intersection drops the drop far too often here.
      collisionDetection={closestCenter}
      onDragCancel={() => setActiveProject(null)}
      onDragEnd={handleDragEnd}
      onDragStart={handleDragStart}
      sensors={sensors}
    >
      <SortableContext
        items={projectIds}
        strategy={verticalListSortingStrategy}
      >
        {children}
      </SortableContext>
      <DragOverlay>
        {activeProject ? (
          <DragOverlayProject name={activeProject.name} />
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}
