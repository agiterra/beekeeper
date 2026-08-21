import { index, rootRoute, route } from "@tanstack/virtual-file-routes";

export const routes = rootRoute("root.tsx", [
  index("index.tsx"),
  route("/agents", "agents.tsx"),
  route("/pulse", "pulse.tsx"),
  route("/reminders", "reminders.tsx"),
  route("/shell/$sessionId", "shell.$sessionId.tsx"),
  route("/coding-sessions/new", "coding-sessions.new.tsx"),
  // Nested (not flat) on purpose: the route generator silently drops flat
  // multi-segment param paths from virtual configs, so a regen by any dev
  // server would erase this route in its flat form.
  route("/observe/$owner", [
    route("/$sessionId", "observe.$owner.$sessionId.tsx"),
  ]),
  route("/coding-sessions/$channelId", [
    route("/$generationId", "coding-sessions.$channelId.$generationId.tsx"),
  ]),
  route("/settings", "settings.tsx"),
  route("/workflows", "workflows.tsx"),
  route("/workflows/$workflowId", "workflows.$workflowId.tsx"),
  route("/projects", "projects.tsx"),
  route("/projects/$projectId", "projects.$projectId.tsx", [
    index("projects.$projectId.index.tsx"),
    route("/code/$repoId", "projects.$projectId.code.$repoId.tsx"),
  ]),
  route("/messages/new", "messages.new.tsx"),
  route("/channels/$channelId", "channels.$channelId.tsx"),
  route(
    "/channels/$channelId/posts/$postId",
    "channels.$channelId.posts.$postId.tsx",
  ),
]);
