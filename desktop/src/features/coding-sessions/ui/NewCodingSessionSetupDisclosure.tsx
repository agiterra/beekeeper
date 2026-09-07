import type * as React from "react";

import type { NewCodingSessionTarget } from "../lib/newCodingSessionModel";

/** Summarize the selected runtime without claiming foreign targets are local. */
export function formatNewCodingSessionSetupProvider(
  target: NewCodingSessionTarget | null,
) {
  if (!target) return "No provider selected";
  const name = target.availability?.label ?? target.provider.runtime;
  if (target.availability?.state === "missing")
    return `${name} · not installed`;
  if (target.availability?.state === "needs_auth") {
    return `${name} · sign-in needed`;
  }
  return name;
}

/**
 * The launcher's advanced controls behind one disclosure. The compact
 * "Current setup" summary that used to sit above it repeated what the
 * disclosure says and was removed (Andy, 2026-09-07); the shell stays
 * separate so the form keeps the existing state, readiness and submit
 * behavior.
 */
export function NewCodingSessionSetupDisclosure({
  children,
  configurationOpen,
  governed,
  onConfigurationOpenChange,
}: {
  children: React.ReactNode;
  configurationOpen: boolean;
  governed: boolean;
  onConfigurationOpenChange: (open: boolean) => void;
}) {
  return (
    <details
      className="rounded-lg border border-border/60 bg-muted/20 px-3 py-2.5"
      data-testid="new-coding-session-configuration"
      onToggle={(event) => onConfigurationOpenChange(event.currentTarget.open)}
      open={configurationOpen}
    >
      <summary className="cursor-pointer text-xs font-medium text-muted-foreground">
        Setup and advanced options
        {governed
          ? " · lead, runtime, bench and policy"
          : " · lead and runtime"}
      </summary>
      <div className="mt-3 flex flex-col gap-5">{children}</div>
    </details>
  );
}
