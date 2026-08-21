import * as React from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { useAgentProgress } from "@/app/agentProgressComposition";
import { FeatureGate, usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const AgentProgressScreen = React.lazy(async () => {
  const module = await import(
    "@/features/agent-progress/ui/AgentProgressScreen"
  );
  return { default: module.AgentProgressScreen };
});

export const Route = createFileRoute("/agent-progress")({
  component: AgentProgressRouteComponent,
});

function AgentProgressRouteComponent() {
  usePreviewFeatureWarning("agent-progress");
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="agents" />}>
      <FeatureGate feature="agent-progress">
        <AgentProgressContent />
      </FeatureGate>
    </React.Suspense>
  );
}

function AgentProgressContent() {
  const navigate = useNavigate();
  const state = useAgentProgress();
  return (
    <AgentProgressScreen
      state={state}
      onOpenLane={(lane) => {
        if (!lane.openTarget) return;
        void navigate({
          to: "/coding-sessions/$channelId/$generationId",
          params: {
            channelId: lane.openTarget.channelId,
            generationId: lane.openTarget.generationId,
          },
          search: { surface: "main" },
        });
      }}
    />
  );
}
