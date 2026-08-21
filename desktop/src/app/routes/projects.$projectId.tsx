import { Outlet, createFileRoute } from "@tanstack/react-router";

// Layout for a project container: the index child renders the project home
// (ProjectContainerScreen) and /code/$repoId renders repo detail.
export const Route = createFileRoute("/projects/$projectId")({
  component: Outlet,
});
