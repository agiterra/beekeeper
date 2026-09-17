import { ListChecks } from "lucide-react";
import { Link } from "@tanstack/react-router";

import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";
import { LOCAL_GENERAL_ID } from "@/features/projects-container/lib/projectContainerModel";
import {
  EmptyHint,
  SectionCard,
} from "@/features/projects-container/ui/SectionCard";

import type { TodoList } from "../lib/todoFold";
import { useProjectTodos } from "../lib/todoQueries";

/** One list's line on the overview card. */
export function todoListSummary(list: TodoList): string {
  const total = list.open.length + list.completed.length;
  if (total === 0) return "empty";
  return `${list.completed.length} of ${total} done`;
}

/**
 * The project home's To-Do summary: each unarchived list with its progress,
 * linking into the tab. Reads the same query the tab does, so the two never
 * disagree about a count.
 */
export function ProjectTodosCard({ project }: { project: ProjectContainer }) {
  const coordinate =
    project.id !== LOCAL_GENERAL_ID && project.owner.length > 0
      ? project.address
      : null;
  const state = useProjectTodos(coordinate);
  const lists = (state.read?.digest.lists ?? []).filter(
    (list) => !list.archived,
  );
  return (
    <SectionCard
      action={
        coordinate ? (
          <Link
            className="text-xs text-primary hover:underline"
            data-testid="project-todos-open"
            params={{ projectId: project.id }}
            to="/projects/$projectId/todos"
          >
            Open
          </Link>
        ) : undefined
      }
      count={lists.length}
      icon={<ListChecks className="size-4" />}
      testId="project-todos-card"
      title="To-do lists"
    >
      {coordinate === null ? (
        <EmptyHint>Publish the project to hold to-do lists.</EmptyHint>
      ) : state.kind === "loading" && !state.read ? (
        <EmptyHint>Reading lists…</EmptyHint>
      ) : state.kind === "error" && !state.read ? (
        <EmptyHint>Could not read the lists: {state.message}</EmptyHint>
      ) : lists.length === 0 ? (
        <EmptyHint>No to-do lists yet.</EmptyHint>
      ) : (
        <ul className="flex flex-col gap-1">
          {lists.map((list) => (
            <li className="flex items-center gap-2 text-sm" key={list.id}>
              <Link
                className="min-w-0 flex-1 truncate hover:underline"
                params={{ projectId: project.id }}
                to="/projects/$projectId/todos"
              >
                {list.title}
              </Link>
              <span className="text-2xs text-muted-foreground">
                {todoListSummary(list)}
              </span>
            </li>
          ))}
          {state.read?.truncated ? (
            <li className="text-2xs text-muted-foreground">
              Older history was not fetched; counts may be incomplete.
            </li>
          ) : null}
        </ul>
      )}
    </SectionCard>
  );
}
