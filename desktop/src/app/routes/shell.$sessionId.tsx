import { createFileRoute } from "@tanstack/react-router";

import { ShellSessionScreen } from "@/features/builtin-shell/ui/ShellSessionScreen";

export const Route = createFileRoute("/shell/$sessionId")({
  component: ShellRouteComponent,
});

function ShellRouteComponent() {
  const { sessionId } = Route.useParams();
  return <ShellSessionScreen sessionId={sessionId} />;
}
