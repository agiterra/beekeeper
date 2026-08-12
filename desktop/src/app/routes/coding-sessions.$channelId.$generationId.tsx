import {
  createFileRoute,
  useCanGoBack,
  useNavigate,
  useRouter,
} from "@tanstack/react-router";
import * as React from "react";

import {
  parseCodingSessionSurface,
  type CodingSessionSurface,
} from "@/features/coding-sessions/lib/codingSessionRoute";
import { CodingSessionWorkspace } from "@/features/coding-sessions/ui/CodingSessionWorkspace";

type CodingSessionRouteSearch = {
  surface: CodingSessionSurface;
};

function validateCodingSessionSearch(
  search: Record<string, unknown>,
): CodingSessionRouteSearch {
  return { surface: parseCodingSessionSurface(search.surface) };
}

export const Route = createFileRoute(
  "/coding-sessions/$channelId/$generationId",
)({
  validateSearch: validateCodingSessionSearch,
  component: CodingSessionRouteComponent,
});

function CodingSessionRouteComponent() {
  const { channelId, generationId } = Route.useParams();
  const { surface } = Route.useSearch();
  const canGoBack = useCanGoBack();
  const navigate = useNavigate();
  const router = useRouter();

  const handleBack = React.useCallback(async () => {
    if (canGoBack) {
      router.history.back();
      return;
    }
    await navigate({
      to: "/channels/$channelId",
      params: { channelId },
    });
  }, [canGoBack, channelId, navigate, router.history]);

  return (
    <CodingSessionWorkspace
      channelId={channelId}
      generationId={generationId}
      onBack={() => void handleBack()}
      surface={surface}
    />
  );
}
