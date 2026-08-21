import { isTauri } from "@tauri-apps/api/core";
import {
  createFileRoute,
  useCanGoBack,
  useNavigate,
  useRouter,
} from "@tanstack/react-router";
import * as React from "react";

import {
  loadCodingSessionPopoutBootstrap,
  type CodingSessionPopoutBootstrap,
} from "@/features/coding-sessions/lib/codingSessionBootstrap";
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
  const [bootstrapState, setBootstrapState] = React.useState<
    | { kind: "not-needed" }
    | { kind: "loading" }
    | { kind: "ready"; bootstrap: CodingSessionPopoutBootstrap | null }
  >(() =>
    surface === "popout" && isTauri()
      ? { kind: "loading" }
      : { kind: "not-needed" },
  );

  React.useEffect(() => {
    if (surface !== "popout" || !isTauri()) {
      setBootstrapState({ kind: "not-needed" });
      return;
    }
    let cancelled = false;
    void import("@tauri-apps/api/webviewWindow")
      .then(({ getCurrentWebviewWindow }) =>
        loadCodingSessionPopoutBootstrap(getCurrentWebviewWindow().label, {
          channelId,
          generationId,
        }),
      )
      .then((bootstrap) => {
        if (!cancelled) setBootstrapState({ kind: "ready", bootstrap });
      })
      .catch(() => {
        if (!cancelled) setBootstrapState({ kind: "ready", bootstrap: null });
      });
    return () => {
      cancelled = true;
    };
  }, [channelId, generationId, surface]);

  const handleBack = React.useCallback(async () => {
    if (surface === "popout" && isTauri()) {
      const { getCurrentWebviewWindow } = await import(
        "@tauri-apps/api/webviewWindow"
      );
      await getCurrentWebviewWindow().close();
      return;
    }
    if (canGoBack) {
      router.history.back();
      return;
    }
    await navigate({
      to: "/channels/$channelId",
      params: { channelId },
    });
  }, [canGoBack, channelId, navigate, router.history, surface]);

  if (bootstrapState.kind === "loading") {
    return (
      <main
        className="flex h-full min-h-0 flex-1 items-center justify-center bg-background text-sm text-muted-foreground"
        data-testid="coding-session-popout-bootstrap-loading"
      >
        Opening exact signed session…
      </main>
    );
  }

  return (
    <CodingSessionWorkspace
      bootstrap={
        bootstrapState.kind === "ready" ? bootstrapState.bootstrap : null
      }
      channelId={channelId}
      generationId={generationId}
      onBack={() => void handleBack()}
      requireBootstrap={surface === "popout" && isTauri()}
      surface={surface}
    />
  );
}
